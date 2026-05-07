# fmg — Frontmatter Graph

A fast, Obsidian-native CLI for traversing `[[WikiLink]]` relationships in frontmatter markdown vaults. Single binary, zero config, zero runtime dependencies.

```
fmg query "LibreChat" --depth 2 --format json
fmg bridge "LibreChat" "TensorZero"
fmg centrality --limit 10
fmg subgraph "MongoDB" --depth 2 --format mermaid
```

## Problem

Obsidian vaults define typed relationships via `[[WikiLink]]` references in YAML frontmatter. Obsidian's Graph View and ExcaliBrain visualize these for humans, but **no tool exists for agents or scripts to programmatically traverse multi-hop relationships**.

`fmg` is that tool. Point it at any directory of markdown files — it parses frontmatter, builds a graph in memory, and answers queries. Every run is fresh from disk. No indexing step, no config file, no schema definition required.

## Installation

```bash
# From source
cargo install --path .

# Or build and copy
cargo build --release
cp target/release/fmg ~/.local/bin/
```

## Usage

```
fmg [-w PATH] [-f FORMAT] <COMMAND>
```

### Commands

| Command | Description |
|---------|-------------|
| `describe` | Vault statistics: page count, frontmatter coverage, field names, node/edge counts |
| `query <NODE>` | Multi-hop BFS traversal from a node |
| `orphans` | Pages with no inbound or outbound relationships |
| `broken` | Unresolved `[[WikiLinks]]` that point to no backing file |
| `bridge <A> <B>` | Shortest path between two nodes |
| `centrality` | Most-connected nodes (hub discovery) |
| `subgraph <NODE>` | Exportable neighborhood graph |

### Global flags

| Flag | Default | Description |
|------|---------|-------------|
| `-w, --workspace PATH` | `.` (current dir) | Vault root directory |
| `-f, --format FORMAT` | `text` | Output format: `text`, `json`, `mermaid`, `paths` |

### `query` flags

| Flag | Default | Description |
|------|---------|-------------|
| `--depth N` | `1` | Max hops (capped at `max_depth` from config) |
| `--direction up\|down\|both` | `both` | `up` = outgoing edges, `down` = incoming, `both` = all |
| `--fields F1,F2,...` | all fields | Restrict traversal to specific relationship fields |

### `centrality` flags

| Flag | Default | Description |
|------|---------|-------------|
| `--limit N` | `10` | Number of results to return |

### `subgraph` flags

| Flag | Default | Description |
|------|---------|-------------|
| `--depth N` | `2` | Neighborhood radius in hops |

## Examples

```bash
# Vault overview
fmg describe
fmg -f json describe

# What does LibreChat depend on, 2 hops deep?
fmg query "LibreChat" --depth 2 --direction up

# What depends on MongoDB? (blast radius)
fmg query "MongoDB" --direction down --depth 3

# Only traverse specific relationship fields
fmg query "LibreChat" --depth 3 --fields depends_on,part_of

# Shortest path between two nodes
fmg bridge "LibreChat" "TensorZero"

# Find hub concepts
fmg centrality --limit 10

# Machine-readable output for agents
fmg -f json query "Topology" --depth 2

# Generate Mermaid diagram
fmg -f mermaid subgraph "LibreChat" --depth 2

# File paths only (for xargs, etc.)
fmg -f paths query "LibreChat" --depth 1

# Find quality issues
fmg orphans
fmg broken
```

## Output Formats

### `text` (default)

Human-readable, paste-into-agent-context-ready:

```
═══════════════════════════════════════
  LibreChat (HelperAI)
  depth=2  direction=both  nodes=5
═══════════════════════════════════════

── Hop 1 ─────────────────────────────
  LibreChat (HelperAI) ──[depends_on]──▶ LibreChat MongoDB
  LibreChat (HelperAI) ──[depends_on]──▶ LiteLLM Proxy  ⚠ external
  LibreChat (HelperAI) ──[related_to]──▶ TensorZero

── Hop 2 ─────────────────────────────
  TensorZero ──[depends_on]──▶ ClickHouse  ⚠ external
```

### `json`

```json
{
  "center": "LibreChat (HelperAI)",
  "depth": 2,
  "nodes": [
    {"title": "LibreChat (HelperAI)", "hop": 0, "type": "service", "path": "services/librechat.md"},
    {"title": "LibreChat MongoDB",    "hop": 1, "type": "data-store", "path": "data-stores/librechat-mongodb.md"}
  ],
  "edges": [
    {"from": "LibreChat (HelperAI)", "to": "LibreChat MongoDB", "field": "depends_on", "hop": 1}
  ]
}
```

### `mermaid`

```
graph LR
  A["LibreChat (HelperAI)"] -->|depends_on| B["LibreChat MongoDB"]
  A["LibreChat (HelperAI)"] -->|depends_on| C["LiteLLM Proxy"]
```

### `paths`

```
services/librechat.md
data-stores/librechat-mongodb.md
```

Useful with `xargs`: `fmg -f paths query "LibreChat" | xargs grep "TODO"`.

## WikiLink Resolution

Frontmatter values like `depends_on: ["[[LibreChat MongoDB]]", "[[LiteLLM Proxy]]"]` are resolved by matching against each page in priority order:

1. `title` frontmatter field (exact, case-insensitive)
2. `aliases` frontmatter array
3. Filename stem (without `.md`)

Unresolved links become **external nodes** — present in the graph, flagged with `⚠ external`.

Nodes can be looked up by any of the above (title, alias, or stem) when specified on the CLI.

## Relationship Fields

### Auto-detection

`fmg` scans all frontmatter and identifies fields whose values are arrays of `[[WikiLink]]` strings. These become edge types automatically — no schema required.

### Default directionality

| Field | Direction |
|-------|-----------|
| `related_to`, `seeAlso` | bidirectional (A ↔ B) |
| everything else | forward (A → B) |

## Configuration (optional)

Place `.fmg.toml` at the vault root. Everything is optional.

```toml
[resolve]
title_field = "title"        # frontmatter field for page title
alias_field = "aliases"      # frontmatter field for alternate names
fallback    = "filename_stem"

[fields]
# Only these fields become graph edges (default: auto-detect all)
edges = ["depends_on", "part_of", "related_to"]

[fields.direction]
related_to = "bidirectional"
depends_on = "forward"

[display]
default_depth = 1
max_depth     = 10
```

## Design

**The vault IS the database.** No indexing, no daemon, no schema. `fmg` parses on every run and exits. Fast enough for tens of thousands of files.

Performance targets:
- 500 files → < 50 ms
- 5,000 files → < 200 ms
- 50,000 files → < 2 s
