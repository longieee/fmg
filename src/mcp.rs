/// MCP (Model Context Protocol) server over stdio.
///
/// Implements JSON-RPC 2.0 with newline-delimited messages.
/// Each line of stdin is a complete JSON-RPC request; each response is written
/// as a single JSON line to stdout followed by '\n'.
///
/// Usage:
///   fmg serve -w /path/to/vault
///
/// Claude Desktop config:
///   { "mcpServers": { "fmg": { "command": "/path/to/fmg", "args": ["serve", "-w", "/path/to/vault"] } } }
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::config::Config;
use crate::graph::VaultGraph;
use crate::output::{self, Format};
use crate::query::{self, TraversalDirection};
use crate::vault;

// ─── Tool definitions ────────────────────────────────────────────────────────

fn tool_list() -> Value {
    json!([
        {
            "name": "describe",
            "description": "Vault statistics: page count, frontmatter coverage, field names, node/edge counts.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        },
        {
            "name": "query",
            "description": "Multi-hop BFS traversal from a node. Returns nodes and edges reachable within `depth` hops.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "node": {
                        "type": "string",
                        "description": "The node title to start from (fuzzy-matched)"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Max hops (default: 1)",
                        "default": 1
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["up", "down", "both"],
                        "description": "up = outgoing edges, down = incoming, both = all (default: both)",
                        "default": "both"
                    },
                    "fields": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Restrict traversal to these frontmatter relationship fields (default: all)"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["text", "json", "mermaid", "paths"],
                        "description": "Output format (default: json)",
                        "default": "json"
                    }
                },
                "required": ["node"]
            }
        },
        {
            "name": "orphans",
            "description": "Lists pages with no inbound or outbound relationships.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "enum": ["text", "json"],
                        "description": "Output format (default: json)",
                        "default": "json"
                    }
                },
                "required": []
            }
        },
        {
            "name": "broken",
            "description": "Lists unresolved [[WikiLinks]] that point to no backing file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "enum": ["text", "json"],
                        "description": "Output format (default: json)",
                        "default": "json"
                    }
                },
                "required": []
            }
        },
        {
            "name": "bridge",
            "description": "Finds the shortest path between two nodes in the graph.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "a": {
                        "type": "string",
                        "description": "Source node title (fuzzy-matched)"
                    },
                    "b": {
                        "type": "string",
                        "description": "Target node title (fuzzy-matched)"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["text", "json", "paths"],
                        "description": "Output format (default: json)",
                        "default": "json"
                    }
                },
                "required": ["a", "b"]
            }
        },
        {
            "name": "centrality",
            "description": "Returns the most-connected nodes (hub discovery) by degree.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Number of results (default: 10)",
                        "default": 10
                    },
                    "format": {
                        "type": "string",
                        "enum": ["text", "json"],
                        "description": "Output format (default: json)",
                        "default": "json"
                    }
                },
                "required": []
            }
        },
        {
            "name": "subgraph",
            "description": "Returns an exportable neighborhood graph centered on a node.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "node": {
                        "type": "string",
                        "description": "Center node title (fuzzy-matched)"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Radius in hops (default: 2)",
                        "default": 2
                    },
                    "format": {
                        "type": "string",
                        "enum": ["text", "json", "mermaid"],
                        "description": "Output format (default: json)",
                        "default": "json"
                    }
                },
                "required": ["node"]
            }
        }
    ])
}

// ─── Tool dispatch ────────────────────────────────────────────────────────────

/// Returns (content_text, is_error).
fn call_tool(vg: &VaultGraph, config: &Config, name: &str, args: &Value) -> (String, bool) {
    match name {
        "describe" => {
            let fmt = parse_format(args, Format::Json);
            (output::format_describe(vg, fmt), false)
        }

        "query" => {
            let Some(node_title) = args["node"].as_str() else {
                return (r#"{"error": "missing required argument: node"}"#.into(), true);
            };
            let depth = args["depth"].as_u64().unwrap_or(1) as u32;
            let depth = depth.min(config.display.max_depth);
            let direction = parse_direction(args);
            let fields: Option<Vec<String>> = args["fields"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
            let fmt = parse_format(args, Format::Json);

            match vg.fuzzy_find_node(node_title) {
                Ok(center) => {
                    let result = query::query(vg, center, depth, direction, fields.as_deref());
                    (output::format_query(vg, &result, fmt), false)
                }
                Err(candidates) if candidates.is_empty() => {
                    (format!(r#"{{"error": "node '{}' not found"}}"#, node_title), true)
                }
                Err(candidates) => {
                    let list = candidates.join(", ");
                    (format!(r#"{{"error": "ambiguous node '{}'. Did you mean: {}}}"#, node_title, list), true)
                }
            }
        }

        "orphans" => {
            let fmt = parse_format(args, Format::Json);
            let orphans = query::orphans(vg);
            (output::format_orphans(vg, &orphans, fmt), false)
        }

        "broken" => {
            let fmt = parse_format(args, Format::Json);
            let broken = query::broken_links(vg);
            (output::format_broken(vg, &broken, fmt), false)
        }

        "bridge" => {
            let Some(a) = args["a"].as_str() else {
                return (r#"{"error": "missing required argument: a"}"#.into(), true);
            };
            let Some(b) = args["b"].as_str() else {
                return (r#"{"error": "missing required argument: b"}"#.into(), true);
            };
            let fmt = parse_format(args, Format::Json);

            let from = match vg.fuzzy_find_node(a) {
                Ok(idx) => idx,
                Err(candidates) if candidates.is_empty() => {
                    return (format!(r#"{{"error": "node '{}' not found"}}"#, a), true);
                }
                Err(candidates) => {
                    return (format!(r#"{{"error": "ambiguous node '{}': {}}}"#, a, candidates.join(", ")), true);
                }
            };
            let to = match vg.fuzzy_find_node(b) {
                Ok(idx) => idx,
                Err(candidates) if candidates.is_empty() => {
                    return (format!(r#"{{"error": "node '{}' not found"}}"#, b), true);
                }
                Err(candidates) => {
                    return (format!(r#"{{"error": "ambiguous node '{}': {}}}"#, b, candidates.join(", ")), true);
                }
            };

            match query::bridge(vg, from, to) {
                Some(path) => (output::format_bridge(vg, from, to, &path, fmt), false),
                None => (format!(r#"{{"error": "no path found between '{}' and '{}'}}"#, a, b), true),
            }
        }

        "centrality" => {
            let limit = args["limit"].as_u64().unwrap_or(10) as usize;
            let fmt = parse_format(args, Format::Json);
            let results = query::centrality(vg, limit);
            (output::format_centrality(vg, &results, fmt), false)
        }

        "subgraph" => {
            let Some(node_title) = args["node"].as_str() else {
                return (r#"{"error": "missing required argument: node"}"#.into(), true);
            };
            let depth = args["depth"].as_u64().unwrap_or(2) as u32;
            let depth = depth.min(config.display.max_depth);
            let fmt = parse_format(args, Format::Json);

            match vg.fuzzy_find_node(node_title) {
                Ok(center) => {
                    let result = query::query(vg, center, depth, TraversalDirection::Both, None);
                    (output::format_query(vg, &result, fmt), false)
                }
                Err(candidates) if candidates.is_empty() => {
                    (format!(r#"{{"error": "node '{}' not found"}}"#, node_title), true)
                }
                Err(candidates) => {
                    (format!(r#"{{"error": "ambiguous node '{}': {}}}"#, node_title, candidates.join(", ")), true)
                }
            }
        }

        unknown => (format!(r#"{{"error": "unknown tool: {}}}"#, unknown), true),
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn parse_format(args: &Value, default: Format) -> Format {
    args["format"]
        .as_str()
        .map(Format::from_str)
        .unwrap_or(default)
}

fn parse_direction(args: &Value) -> TraversalDirection {
    match args["direction"].as_str().unwrap_or("both") {
        "up" => TraversalDirection::Up,
        "down" => TraversalDirection::Down,
        _ => TraversalDirection::Both,
    }
}

// ─── JSON-RPC helpers ─────────────────────────────────────────────────────────

fn write_response(id: &Value, result: Value) {
    let response = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    });
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &response).ok();
    stdout.write_all(b"\n").ok();
    stdout.flush().ok();
}

fn write_error(id: &Value, code: i64, message: &str) {
    let response = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    });
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &response).ok();
    stdout.write_all(b"\n").ok();
    stdout.flush().ok();
}

// ─── Server entry point ───────────────────────────────────────────────────────

pub fn run(vault_root: PathBuf) {
    let config = Config::load(&vault_root);
    let pages = vault::scan_vault(&vault_root);
    let vg = VaultGraph::build(&pages, &config, false);

    let stdin = io::stdin().lock();
    for line in stdin.lines() {
        let line = match line {
            Ok(l) if l.trim().is_empty() => continue,
            Ok(l) => l,
            Err(_) => break,
        };

        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                // id is unknown; use null
                write_error(&Value::Null, -32700, &format!("parse error: {e}"));
                continue;
            }
        };

        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg["method"].as_str().unwrap_or("");

        // Notifications have no id — do not respond.
        let is_notification = msg.get("id").is_none();

        match method {
            "initialize" => {
                write_response(
                    &id,
                    json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": { "tools": {} },
                        "serverInfo": {
                            "name": "fmg",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }),
                );
            }

            "notifications/initialized" | "initialized" => {
                // Client notification — no response needed.
            }

            "tools/list" => {
                write_response(&id, json!({ "tools": tool_list() }));
            }

            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                let tool_name = params["name"].as_str().unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                let (text, is_error) = call_tool(&vg, &config, tool_name, &arguments);
                write_response(
                    &id,
                    json!({
                        "content": [{ "type": "text", "text": text }],
                        "isError": is_error
                    }),
                );
            }

            "ping" => {
                write_response(&id, json!({}));
            }

            _ if is_notification => {
                // Unknown notification — ignore silently.
            }

            _ => {
                write_error(&id, -32601, &format!("method not found: {method}"));
            }
        }
    }
}
