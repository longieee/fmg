use std::collections::HashMap;

use petgraph::graph::NodeIndex;
use serde::Serialize;

use crate::graph::VaultGraph;
use crate::query::{QueryResult, TraversalDirection};

/// Output format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
    Mermaid,
    Paths,
}

impl Format {
    pub fn from_str(s: &str) -> Self {
        match s {
            "json" => Format::Json,
            "mermaid" => Format::Mermaid,
            "paths" => Format::Paths,
            _ => Format::Text,
        }
    }
}

// ─── Describe ───────────────────────────────────────────────────────────────

pub fn format_describe(vg: &VaultGraph, format: Format) -> String {
    match format {
        Format::Json => format_describe_json(vg),
        _ => format_describe_text(vg),
    }
}

fn format_describe_text(vg: &VaultGraph) -> String {
    let total_nodes = vg.graph.node_count();
    let internal = vg
        .graph
        .node_indices()
        .filter(|&i| !vg.graph[i].external)
        .count();
    let external = total_nodes - internal;
    let edge_count = vg.graph.edge_count();
    let coverage = if vg.total_pages > 0 {
        (vg.pages_with_frontmatter as f64 / vg.total_pages as f64) * 100.0
    } else {
        0.0
    };

    let mut fields: Vec<&String> = vg.fields.iter().collect();
    fields.sort();

    let mut out = String::new();
    out.push_str("═══════════════════════════════════════\n");
    out.push_str("  Vault Statistics\n");
    out.push_str("═══════════════════════════════════════\n\n");
    out.push_str(&format!("  Pages scanned:      {}\n", vg.total_pages));
    out.push_str(&format!("  Frontmatter coverage: {coverage:.0}%\n"));
    out.push_str(&format!("  Graph nodes:        {internal} internal + {external} external\n"));
    out.push_str(&format!("  Graph edges:        {edge_count}\n"));
    out.push_str(&format!(
        "  Relationship fields: {}\n",
        if fields.is_empty() {
            "(none)".to_string()
        } else {
            fields.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        }
    ));
    out
}

#[derive(Serialize)]
struct DescribeJson {
    total_pages: usize,
    frontmatter_coverage: f64,
    internal_nodes: usize,
    external_nodes: usize,
    edges: usize,
    fields: Vec<String>,
}

fn format_describe_json(vg: &VaultGraph) -> String {
    let internal = vg
        .graph
        .node_indices()
        .filter(|&i| !vg.graph[i].external)
        .count();
    let external = vg.graph.node_count() - internal;
    let coverage = if vg.total_pages > 0 {
        (vg.pages_with_frontmatter as f64 / vg.total_pages as f64) * 100.0
    } else {
        0.0
    };
    let mut fields: Vec<String> = vg.fields.iter().cloned().collect();
    fields.sort();

    let data = DescribeJson {
        total_pages: vg.total_pages,
        frontmatter_coverage: (coverage * 10.0).round() / 10.0,
        internal_nodes: internal,
        external_nodes: external,
        edges: vg.graph.edge_count(),
        fields,
    };
    serde_json::to_string_pretty(&data).unwrap()
}

// ─── Query ──────────────────────────────────────────────────────────────────

pub fn format_query(vg: &VaultGraph, result: &QueryResult, format: Format) -> String {
    match format {
        Format::Json => format_query_json(vg, result),
        Format::Mermaid => format_query_mermaid(vg, result),
        Format::Paths => format_query_paths(vg, result),
        Format::Text => format_query_text(vg, result),
    }
}

fn format_query_text(vg: &VaultGraph, result: &QueryResult) -> String {
    let center_node = vg.node(result.center);
    let dir_str = match result.direction {
        TraversalDirection::Up => "up",
        TraversalDirection::Down => "down",
        TraversalDirection::Both => "both",
    };

    let mut out = String::new();
    out.push_str("═══════════════════════════════════════\n");
    out.push_str(&format!("  {}\n", center_node.title));
    out.push_str(&format!(
        "  depth={}  direction={}  nodes={}\n",
        result.depth,
        dir_str,
        result.nodes.len()
    ));
    out.push_str("═══════════════════════════════════════\n");

    // Group edges by hop
    let max_hop = result.edges.iter().map(|e| e.hop).max().unwrap_or(0);
    for hop in 1..=max_hop {
        out.push_str(&format!("\n── Hop {hop} ─────────────────────────────\n"));
        for edge in result.edges.iter().filter(|e| e.hop == hop) {
            let from_node = vg.node(edge.from);
            let to_node = vg.node(edge.to);
            let external_marker = if to_node.external { "  ⚠ external" } else { "" };
            out.push_str(&format!(
                "  {} ──[{}]──▶ {}{}\n",
                from_node.title, edge.field, to_node.title, external_marker
            ));
        }
    }
    out
}

#[derive(Serialize)]
struct QueryJson {
    center: String,
    depth: u32,
    nodes: Vec<QueryNodeJson>,
    edges: Vec<QueryEdgeJson>,
}

#[derive(Serialize)]
struct QueryNodeJson {
    title: String,
    hop: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

#[derive(Serialize)]
struct QueryEdgeJson {
    from: String,
    to: String,
    field: String,
    hop: u32,
}

fn format_query_json(vg: &VaultGraph, result: &QueryResult) -> String {
    let center_node = vg.node(result.center);
    let nodes: Vec<QueryNodeJson> = result
        .nodes
        .iter()
        .map(|&(idx, hop)| {
            let node = vg.node(idx);
            QueryNodeJson {
                title: node.title.clone(),
                hop,
                r#type: node.node_type.clone(),
                path: node.path.as_ref().map(|p| p.to_string_lossy().to_string()),
            }
        })
        .collect();

    let edges: Vec<QueryEdgeJson> = result
        .edges
        .iter()
        .map(|e| QueryEdgeJson {
            from: vg.node(e.from).title.clone(),
            to: vg.node(e.to).title.clone(),
            field: e.field.clone(),
            hop: e.hop,
        })
        .collect();

    let data = QueryJson {
        center: center_node.title.clone(),
        depth: result.depth,
        nodes,
        edges,
    };
    serde_json::to_string_pretty(&data).unwrap()
}

// ─── Cross-service (runtime) edges ────────────────────────────────────────────

/// Format the typed runtime-edge layer (type + endpoint + condition + provenance).
pub fn format_xedges(edges: &[&crate::graph::RuntimeEdge], format: Format) -> String {
    match format {
        Format::Json => serde_json::to_string_pretty(&edges).unwrap(),
        _ => {
            let mut out = String::new();
            out.push_str("═══════════════════════════════════════\n");
            out.push_str(&format!("  Cross-service (runtime) edges: {}\n", edges.len()));
            out.push_str("═══════════════════════════════════════\n\n");
            for e in edges {
                let ext = if e.external_target { "  ⚠ external" } else { "" };
                out.push_str(&format!(
                    "  {} ──[{}]──▶ {}{}\n",
                    e.from, e.edge_type, e.to, ext
                ));
                if let Some(ep) = &e.endpoint {
                    out.push_str(&format!("      endpoint:   {ep}\n"));
                }
                out.push_str(&format!(
                    "      condition:  {}\n",
                    e.condition.as_deref().unwrap_or("(unconditional)")
                ));
                if let Some(p) = &e.provenance {
                    out.push_str(&format!("      provenance: {p}\n"));
                }
            }
            out
        }
    }
}

fn format_query_mermaid(vg: &VaultGraph, result: &QueryResult) -> String {
    let mut out = String::from("graph LR\n");
    let mut node_ids: HashMap<NodeIndex, String> = HashMap::new();
    // Assign center node first
    node_ids.insert(result.center, "A".to_string());
    let mut next_id: u32 = 1;

    // Collect all unique nodes from edges
    for edge in &result.edges {
        for idx in [edge.from, edge.to] {
            if let std::collections::hash_map::Entry::Vacant(e) = node_ids.entry(idx) {
                let id = if next_id < 26 {
                    ((b'A' + next_id as u8) as char).to_string()
                } else {
                    format!("N{next_id}")
                };
                e.insert(id);
                next_id += 1;
            }
        }
    }

    for edge in &result.edges {
        let from_id = &node_ids[&edge.from];
        let to_id = &node_ids[&edge.to];
        let from_title = &vg.node(edge.from).title;
        let to_title = &vg.node(edge.to).title;
        out.push_str(&format!(
            "  {from_id}[\"{from_title}\"] -->|{field}| {to_id}[\"{to_title}\"]\n",
            field = edge.field,
        ));
    }
    out
}

fn format_query_paths(vg: &VaultGraph, result: &QueryResult) -> String {
    let mut out = String::new();
    for &(idx, _) in &result.nodes {
        let node = vg.node(idx);
        if let Some(ref path) = node.path {
            out.push_str(&path.to_string_lossy());
            out.push('\n');
        }
    }
    out
}

// ─── Bridge ─────────────────────────────────────────────────────────────────

pub fn format_bridge(
    vg: &VaultGraph,
    from: NodeIndex,
    to: NodeIndex,
    path: &[(NodeIndex, NodeIndex, String)],
    format: Format,
) -> String {
    match format {
        Format::Json => format_bridge_json(vg, from, to, path),
        _ => format_bridge_text(vg, from, to, path),
    }
}

fn format_bridge_text(
    vg: &VaultGraph,
    from: NodeIndex,
    to: NodeIndex,
    path: &[(NodeIndex, NodeIndex, String)],
) -> String {
    let from_title = &vg.node(from).title;
    let to_title = &vg.node(to).title;

    if path.is_empty() {
        return format!("{from_title} and {to_title} are the same node.\n");
    }

    let mut out = String::new();
    out.push_str(&format!(
        "Path from \"{from_title}\" to \"{to_title}\" ({} hops):\n\n",
        path.len()
    ));
    for (i, (f, t, field)) in path.iter().enumerate() {
        let ft = &vg.node(*f).title;
        let tt = &vg.node(*t).title;
        out.push_str(&format!("  {}. {} ──[{}]──▶ {}\n", i + 1, ft, field, tt));
    }
    out
}

#[derive(Serialize)]
struct BridgeJson {
    from: String,
    to: String,
    hops: usize,
    path: Vec<BridgeStepJson>,
}

#[derive(Serialize)]
struct BridgeStepJson {
    from: String,
    to: String,
    field: String,
}

fn format_bridge_json(
    vg: &VaultGraph,
    from: NodeIndex,
    to: NodeIndex,
    path: &[(NodeIndex, NodeIndex, String)],
) -> String {
    let data = BridgeJson {
        from: vg.node(from).title.clone(),
        to: vg.node(to).title.clone(),
        hops: path.len(),
        path: path
            .iter()
            .map(|(f, t, field)| BridgeStepJson {
                from: vg.node(*f).title.clone(),
                to: vg.node(*t).title.clone(),
                field: field.clone(),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&data).unwrap()
}

// ─── Centrality ─────────────────────────────────────────────────────────────

pub fn format_centrality(
    vg: &VaultGraph,
    results: &[(NodeIndex, usize)],
    format: Format,
) -> String {
    match format {
        Format::Json => format_centrality_json(vg, results),
        _ => format_centrality_text(vg, results),
    }
}

fn format_centrality_text(vg: &VaultGraph, results: &[(NodeIndex, usize)]) -> String {
    let mut out = String::new();
    out.push_str("═══════════════════════════════════════\n");
    out.push_str("  Most Connected Nodes\n");
    out.push_str("═══════════════════════════════════════\n\n");
    for (i, &(idx, degree)) in results.iter().enumerate() {
        let node = vg.node(idx);
        let ext = if node.external { " ⚠ external" } else { "" };
        out.push_str(&format!(
            "  {:>2}. {} (degree: {}){}\n",
            i + 1,
            node.title,
            degree,
            ext
        ));
    }
    out
}

#[derive(Serialize)]
struct CentralityEntryJson {
    title: String,
    degree: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

fn format_centrality_json(vg: &VaultGraph, results: &[(NodeIndex, usize)]) -> String {
    let entries: Vec<CentralityEntryJson> = results
        .iter()
        .map(|&(idx, degree)| {
            let node = vg.node(idx);
            CentralityEntryJson {
                title: node.title.clone(),
                degree,
                path: node.path.as_ref().map(|p| p.to_string_lossy().to_string()),
            }
        })
        .collect();
    serde_json::to_string_pretty(&entries).unwrap()
}

// ─── Orphans ────────────────────────────────────────────────────────────────

pub fn format_orphans(vg: &VaultGraph, orphans: &[NodeIndex], format: Format) -> String {
    match format {
        Format::Json => format_orphans_json(vg, orphans),
        Format::Paths => format_orphans_paths(vg, orphans),
        _ => format_orphans_text(vg, orphans),
    }
}

fn format_orphans_text(vg: &VaultGraph, orphans: &[NodeIndex]) -> String {
    if orphans.is_empty() {
        return "No orphan pages found.\n".to_string();
    }
    let mut out = format!("Orphan pages ({} total):\n\n", orphans.len());
    for &idx in orphans {
        let node = vg.node(idx);
        let path_str = node
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        out.push_str(&format!("  {} ({})\n", node.title, path_str));
    }
    out
}

fn format_orphans_json(vg: &VaultGraph, orphans: &[NodeIndex]) -> String {
    #[derive(Serialize)]
    struct OrphanJson {
        title: String,
        path: Option<String>,
    }
    let entries: Vec<OrphanJson> = orphans
        .iter()
        .map(|&idx| {
            let node = vg.node(idx);
            OrphanJson {
                title: node.title.clone(),
                path: node.path.as_ref().map(|p| p.to_string_lossy().to_string()),
            }
        })
        .collect();
    serde_json::to_string_pretty(&entries).unwrap()
}

fn format_orphans_paths(vg: &VaultGraph, orphans: &[NodeIndex]) -> String {
    let mut out = String::new();
    for &idx in orphans {
        if let Some(ref path) = vg.node(idx).path {
            out.push_str(&path.to_string_lossy());
            out.push('\n');
        }
    }
    out
}

// ─── Broken Links ───────────────────────────────────────────────────────────

pub fn format_broken(
    vg: &VaultGraph,
    broken: &[(NodeIndex, Vec<(NodeIndex, String)>)],
    format: Format,
) -> String {
    match format {
        Format::Json => format_broken_json(vg, broken),
        _ => format_broken_text(vg, broken),
    }
}

fn format_broken_text(
    vg: &VaultGraph,
    broken: &[(NodeIndex, Vec<(NodeIndex, String)>)],
) -> String {
    if broken.is_empty() {
        return "No broken links found.\n".to_string();
    }
    let mut out = format!("Broken links ({} unresolved targets):\n\n", broken.len());
    for (target_idx, sources) in broken {
        let target = &vg.node(*target_idx).title;
        out.push_str(&format!("  ⚠ \"{}\" referenced by:\n", target));
        for (src_idx, field) in sources {
            let src = &vg.node(*src_idx).title;
            out.push_str(&format!("      {} (field: {})\n", src, field));
        }
    }
    out
}

#[derive(Serialize)]
struct BrokenJson {
    target: String,
    referenced_by: Vec<BrokenRefJson>,
}

#[derive(Serialize)]
struct BrokenRefJson {
    title: String,
    field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}

fn format_broken_json(
    vg: &VaultGraph,
    broken: &[(NodeIndex, Vec<(NodeIndex, String)>)],
) -> String {
    let entries: Vec<BrokenJson> = broken
        .iter()
        .map(|(target_idx, sources)| {
            let target = &vg.node(*target_idx).title;
            BrokenJson {
                target: target.clone(),
                referenced_by: sources
                    .iter()
                    .map(|(src_idx, field)| {
                        let src = vg.node(*src_idx);
                        BrokenRefJson {
                            title: src.title.clone(),
                            field: field.clone(),
                            path: src.path.as_ref().map(|p| p.to_string_lossy().to_string()),
                        }
                    })
                    .collect(),
            }
        })
        .collect();
    serde_json::to_string_pretty(&entries).unwrap()
}
