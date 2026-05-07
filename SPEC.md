# fmg — Frontmatter Graph

A fast, Obsidian-native CLI for traversing `[[WikiLink]]` relationships in frontmatter markdown vaults. Single binary, zero config, zero runtime dependencies.

## Problem

Obsidian vaults define typed relationships via `[[WikiLink]]` references in YAML frontmatter. Obsidian's Graph View and ExcaliBrain visualize these for humans, but **no tool exists for agents or scripts to programmatically traverse multi-hop relationships**.

- `fmql` (Python, 2026) — can't resolve `[[WikiLink]]` bracket syntax; requires Python runtime
- `obsidian-vault-graph` (Python, 2026) — too new, limited, also Python
- Obsidian Graph View — visual only, no CLI/API
- ExcaliBrain — visual only, requires Obsidian running

## Design Principle

**The vault IS the database.** No indexing step, no config file, no schema definition. Point `fmg` at a directory of markdown files, it parses frontmatter, builds a graph in memory, and answers queries. Every run is fresh from disk.

## Target Use Cases

| Vault | Files | Frontmatter | Relationship fields |
|---|---|---|---|
| LLM Wiki (engineering KB) | 57 pages | 87% coverage | `depends_on`, `part_of`, `related_to`, `documented_by`, `supersedes`, `owned_by` |
| Personal KB (math/AI) | 574 files | 22% coverage | `requires`, `hasTopic`, `specializes`, `seeAlso`, `source` |
| Any Obsidian vault | — | — | Auto-detected |

## WikiLink Resolution

Frontmatter values like `depends_on: ["[[LibreChat MongoDB]]", "[[LiteLLM Proxy]]"]` are resolved by:

1. Strip `[[` and `]]` brackets
2. Match against, in priority order:
   - `title` frontmatter field (exact)
   - `aliases` frontmatter array
   - Filename stem (without `.md`)
   - Case-insensitive title match
3. Unresolved links become "external" nodes — present in the graph but flagged as having no backing file

## Relationship Fields

### Auto-detection

fmg scans all frontmatter and identifies fields whose values are arrays of `[[WikiLink]]` strings. These become edge types automatically.

### Known field semantics

| Field | Direction | Meaning |
|---|---|---|
| `depends_on` / `requires` | forward | A needs B |
| `part_of` / `hasTopic` | forward | A belongs to B |
| `related_to` / `seeAlso` | bidirectional | A ↔ B |
| `specializes` | forward | A is a subtype of B |
| `documented_by` / `source` | forward | A is documented/sourced by B |
| `supersedes` | forward | A replaces B |
| `owned_by` | forward | A is owned by B |

Direction can be overridden in `.fmg.toml`.

## CLI

```
fmg [-w PATH] <COMMAND>

Commands:
  describe                     Vault statistics: fields, types, node counts, health
  query <NODE>                 Multi-hop graph traversal from a node
    --depth N                  Max hops (default: 1, max: 10)
    --direction up|down|both   Traversal direction (default: both)
    --fields F1,F2             Limit to specific relationship fields
  orphans                      Pages with no inbound or outbound relationships
  broken                       Unresolved [[WikiLinks]] (point to nothing)
  bridge <A> <B>               Shortest path between two nodes
  centrality                   Most-connected nodes (hub discovery)
    --limit N                  Number of results (default: 10)
  subgraph <NODE>              Exportable neighborhood graph
    --depth N                  Radius (default: 2)

Global flags:
  -w, --workspace PATH         Vault root (default: current directory)
  -f, --format text|json|mermaid|paths   Output format (default: text)
  --include-body               Also parse [[links]] from markdown body (slower)
```

### Example usage

```bash
# Vault overview
fmg describe

# What does LibreChat depend on, 2 hops deep?
fmg query "LibreChat" --depth 2 --direction up

# What depends on MongoDB? (blast radius)
fmg query "MongoDB" --direction down --depth 3

# Shortest path between two nodes
fmg bridge "LibreChat" "TensorZero"

# Find hub concepts in personal KB
fmg centrality --limit 10

# Machine-readable for agents
fmg query "Topology" --depth 2 --format json

# Generate Mermaid diagram
fmg subgraph "LibreChat" --depth 2 --format mermaid

# Find quality issues
fmg orphans
fmg broken
```

## Output Formats

### text (default)

Human-readable, paste-into-agent-context-ready:

```
═══════════════════════════════════════
  LibreChat (HelperAI)
  depth=2  direction=both  nodes=30
═══════════════════════════════════════

── Hop 1 ─────────────────────────────
  LibreChat (HelperAI) ──[depends_on]──▶ LibreChat MongoDB
  LibreChat (HelperAI) ──[depends_on]──▶ LiteLLM Proxy  ⚠ external
  ...

── Hop 2 ─────────────────────────────
  LibreChat MongoDB ──[part_of]──▶ LibreChat  ⚠ external
  ...
```

### json

```json
{
  "center": "LibreChat (HelperAI)",
  "depth": 2,
  "nodes": [
    {"title": "LibreChat (HelperAI)", "hop": 0, "type": "service", "path": "services/librechat.md"},
    {"title": "LibreChat MongoDB", "hop": 1, "type": "data-store", "path": "data-stores/librechat-mongodb.md"}
  ],
  "edges": [
    {"from": "LibreChat (HelperAI)", "to": "LibreChat MongoDB", "field": "depends_on", "hop": 1}
  ]
}
```

### mermaid

```
graph LR
  A["LibreChat (HelperAI)"] -->|depends_on| B["LibreChat MongoDB"]
  A -->|depends_on| C["LiteLLM Proxy"]
```

### paths

```
services/librechat.md
data-stores/librechat-mongodb.md
```

## Configuration (optional)

Place `.fmg.toml` at the vault root. **Everything is optional** — fmg works without any config.

```toml
[resolve]
title_field = "title"       # field to match titles against (default)
alias_field = "aliases"     # field containing alternate names (default)
fallback = "filename_stem"  # when no title/alias match: "filename_stem" or "none"

[fields]
# Override auto-detection. Only these fields become edges.
edges = ["depends_on", "part_of", "related_to"]

[fields.direction]
# Override default directionality
related_to = "bidirectional"
depends_on = "forward"

[display]
default_depth = 1
max_depth = 10
```

## Architecture

### Crate dependencies

| Crate | Purpose | Maturity |
|---|---|---|
| `clap` + `clap_derive` | CLI parsing | 14k stars, production |
| `serde` + `serde_yaml` | Frontmatter deserialization | De facto standard |
| `serde_json` | JSON output | De facto standard |
| `petgraph` | Graph data structure + BFS/Dijkstra/centrality | 2k stars, mature |
| `regex` | `[[WikiLink]]` extraction | Core Rust ecosystem |
| `walkdir` | Recursive file discovery | Standard choice |

No async runtime. No web framework. Pure computation.

### Module structure

```
src/
  main.rs           CLI entry point (clap)
  lib.rs            Public API re-exports
  vault.rs          File discovery + frontmatter parsing
  resolve.rs        WikiLink resolution (strip brackets, match title/alias/stem)
  graph.rs          Build petgraph from parsed vault, edge types
  query.rs          BFS traversal, bridge (shortest path), centrality
  output.rs         Formatters: text, json, mermaid, paths
  config.rs         Optional .fmg.toml loading
```

### Performance target

- 500 files: < 50ms
- 5,000 files: < 200ms
- 50,000 files: < 2s

No caching, no indexing. Parse-on-every-run is fast enough for vaults up to tens of thousands of files.

## Distribution (Phase 1: personal use)

```bash
# From the repo
cargo install --path .

# Or just build
cargo build --release
cp target/release/fmg ~/.local/bin/
```

## Future possibilities (not in MVP)

- `--include-body`: parse `[[WikiLinks]]` from markdown body text (not just frontmatter)
- Dataview inline field support (`Author:: [[Name]]`)
- Watch mode: rebuild graph on file changes
- MCP server mode: expose graph queries as MCP tools
- QMD integration: semantic search + graph expansion
- `fmg lint`: validate frontmatter against a schema
- Tag-based grouping and filtering
