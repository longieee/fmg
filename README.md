# fmg — Frontmatter Graph

A fast, Obsidian-native CLI for traversing `[[WikiLink]]` relationships in frontmatter markdown vaults. Single binary, zero config, zero runtime dependencies.

```
fmg query "My Concept" --depth 2 --format json
fmg bridge "Concept A" "Concept B"
fmg centrality --limit 10
fmg subgraph "My Concept" --depth 2 --format mermaid
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
| `xedges` | List typed cross-service (runtime) edges with attributes |
| `serve` | Start an MCP server over stdio (agent/Claude Desktop integration) |

### Global flags

Global flags are accepted **before or after** the subcommand
(`fmg -w vault query X` and `fmg query X -w vault` are equivalent).

| Flag | Default | Description |
|------|---------|-------------|
| `-w, --workspace PATH` | `.` (current dir) | Vault root directory |
| `-f, --format FORMAT` | `text` | Output format: `text`, `json`, `mermaid`, `paths` |
| `--include-body` | off | Also parse `[[WikiLinks]]` from markdown body text |

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

# What does a concept depend on, 2 hops deep?
fmg query "My Concept" --depth 2 --direction up

# What depends on a concept? (blast radius)
fmg query "Core Concept" --direction down --depth 3

# Only traverse specific relationship fields
fmg query "My Concept" --depth 3 --fields depends_on,part_of

# Shortest path between two nodes
fmg bridge "Concept A" "Concept B"

# Find hub concepts
fmg centrality --limit 10

# Machine-readable output for agents
fmg -f json query "My Topic" --depth 2

# Generate Mermaid diagram
fmg -f mermaid subgraph "My Concept" --depth 2

# File paths only (for xargs, etc.)
fmg -f paths query "My Concept" --depth 1

# Find quality issues
fmg orphans
fmg broken
```

## Output Formats

### `text` (default)

Human-readable, paste-into-agent-context-ready:

```
═══════════════════════════════════════
  Concept A
  depth=2  direction=both  nodes=5
═══════════════════════════════════════

── Hop 1 ─────────────────────────────
  Concept A ──[depends_on]──▶ Concept B
  Concept A ──[depends_on]──▶ External Tool  ⚠ external
  Concept A ──[related_to]──▶ Concept C

── Hop 2 ─────────────────────────────
  Concept C ──[depends_on]──▶ Concept D  ⚠ external
```

### `json`

```json
{
  "center": "Concept A",
  "depth": 2,
  "nodes": [
    {"title": "Concept A", "hop": 0, "type": "topic", "path": "topics/concept-a.md"},
    {"title": "Concept B", "hop": 1, "type": "topic", "path": "topics/concept-b.md"}
  ],
  "edges": [
    {"from": "Concept A", "to": "Concept B", "field": "depends_on", "hop": 1}
  ]
}
```

### `mermaid`

```
graph LR
  A["Concept A"] -->|depends_on| B["Concept B"]
  A["Concept A"] -->|related_to| C["Concept C"]
```

### `paths`

```
topics/concept-a.md
topics/concept-b.md
```

Useful with `xargs`: `fmg -f paths query "My Concept" | xargs grep "TODO"`.

## WikiLink Resolution

Frontmatter values like `depends_on: ["[[Concept B]]", "[[Concept C]]"]` are resolved by matching against each page in priority order:

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

## Cross-service (runtime) edges

Structural `[[WikiLink]]` edges name a relationship but carry no other data. Some edges need
**per-edge attributes** a wikilink array can't hold — e.g. a cross-service call with a type, an
endpoint, an enabling condition, and a provenance pointer. `fmg` supports these as a **separate
runtime-edge layer**, declared with a `cross_service:` frontmatter field on the edge's **source**
page (an array of objects):

```yaml
---
title: mcp-user-context-info
type: service
depends_on: ["[[Redis]]"]          # structural edge (unchanged)
cross_service:                      # runtime edges (typed, attributed)
  - target: "[[LibreChat]]"
    type: http-call
    endpoint: /api/agents/chat
    condition: "use_librechat_api==true"
    provenance: src/helperai_service.py:411
---
```

List them with `xedges`:

```bash
fmg xedges                    # all runtime edges
fmg xedges --from LibreChat   # only edges touching a node
fmg -f json xedges            # machine-readable, with all attributes
```

Only `target` is required; `type` defaults to `cross_service`; `endpoint`/`condition`/`provenance`
are optional. Targets resolve like wikilinks (title/alias/stem), falling back to external nodes.

**The runtime layer is namespaced away from the structural graph:** it lives in its own store, so
`describe`, `query`, `bridge`, and `centrality` behave **identically whether or not** `cross_service`
edges exist. Runtime edges surface only through `xedges` (and the `cross_service` MCP tool).

## MCP server

`fmg serve` starts a [Model Context Protocol](https://modelcontextprotocol.io) server over stdio
(JSON-RPC 2.0, newline-delimited), exposing every query as an MCP tool: `describe`, `query`,
`orphans`, `broken`, `bridge`, `centrality`, `subgraph`, and `cross_service`.

```bash
fmg -w /path/to/vault serve
```

Claude Desktop config (`-w` is global, so either arg order works):

```json
{ "mcpServers": { "fmg": { "command": "fmg", "args": ["-w", "/path/to/vault", "serve"] } } }
```

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

**Deterministic output.** Pages are sorted by path, relationship fields by name, and result nodes/edges by a stable key — so identical input yields byte-identical output across runs (no HashMap-order flake).

Performance targets:
- 500 files → < 50 ms
- 5,000 files → < 200 ms
- 50,000 files → < 2 s

### Query complexity

| Command | Time complexity |
|---------|----------------|
| `query` | O(V_d + E_d) — linear in the reachable subgraph within `--depth` hops |
| `bridge` | O(V + E) — BFS over the full graph in the worst case |
| `centrality` | O(V + E) for degree counting, O(V log V) for sorting |
| `orphans` | O(V + E) |
| `broken` | O(V + E) |

Node lookup by name (title/alias/stem) is O(1) average via the `title_index` HashMap.
