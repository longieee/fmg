# fmg Architecture

## Overview

`fmg` is a pure-computation Rust CLI. On every invocation it:

1. Reads `.fmg.toml` (optional config)
2. Walks the vault directory, parsing YAML frontmatter from every `.md` file
3. Builds a directed graph in memory
4. Runs the requested query
5. Formats and prints output
6. Exits

No indexing, no caching, no daemon, no database. The vault is the database.

---

## Module Map

```
src/
├── main.rs      CLI entry point — argument parsing, command dispatch
├── lib.rs       Re-exports all public modules
├── config.rs    .fmg.toml loading and field-direction semantics
├── vault.rs     File discovery and YAML frontmatter parsing
├── resolve.rs   WikiLink-to-page resolution index
├── graph.rs     In-memory petgraph construction
├── query.rs     BFS traversal, bridge (shortest path), centrality, orphans, broken links
└── output.rs    Formatters: text, json, mermaid, paths
```

---

## Data Flow

```
                          CLI args (clap)
                               │
                          main.rs
                          │        │
                    Config::load   vault::scan_vault
                    (.fmg.toml)    (walkdir + serde_yaml)
                          │        │
                          └──┬─────┘
                             │
                    VaultGraph::build
                    (resolve.rs + graph.rs)
                             │
                             ├── query::query         (BFS)
                             ├── query::bridge        (BFS shortest path)
                             ├── query::centrality    (degree sort)
                             ├── query::orphans       (zero-degree filter)
                             └── query::broken_links  (external node scan)
                                          │
                                   output::format_*
                                   (text / json / mermaid / paths)
                                          │
                                        stdout
```

---

## Module Details

### `config.rs` — Configuration

**Types:** `Config`, `ResolveConfig`, `FieldsConfig`, `DisplayConfig`, `Direction`

Loads `.fmg.toml` via `toml::from_str`. All fields are optional with defaults:

| Setting | Default |
|---------|---------|
| `resolve.title_field` | `"title"` |
| `resolve.alias_field` | `"aliases"` |
| `resolve.fallback` | `"filename_stem"` |
| `display.default_depth` | `1` |
| `display.max_depth` | `10` |

`Config::direction_for(field)` returns `Direction::Bidirectional` for `related_to` / `seeAlso`, `Direction::Forward` for everything else. Per-field overrides in `[fields.direction]` take precedence.

---

### `vault.rs` — File Discovery + Frontmatter Parsing

**Types:** `Page`

**`scan_vault(vault_root) → Vec<Page>`**

Uses `walkdir` to recurse the vault root. Skips:
- Non-`.md` files
- Hidden paths (any component starting with `.`, e.g., `.obsidian/`, `.trash/`)

Each `.md` file is parsed by `parse_frontmatter`, which:
1. Checks for a `---` fence at the start of the file
2. Extracts the YAML block between the opening and closing `---`
3. Deserializes it as `HashMap<String, serde_yaml::Value>`

**`Page::wikilink_fields() → HashMap<String, Vec<String>>`**

Scans all frontmatter fields and returns those whose values contain `[[WikiLink]]` patterns. The WikiLink extractor handles:
- Single strings: `depends_on: "[[Foo]]"`
- Arrays of strings: `depends_on: ["[[Foo]]", "[[Bar]]"]`
- Nested arrays (sequences of sequences)

---

### `resolve.rs` — WikiLink Resolution

**Types:** `Resolver`

Built once from `Vec<Page>`. Maintains three hash maps (all keys lowercased):

| Map | Key | Priority |
|-----|-----|----------|
| `title_map` | frontmatter `title` field | 1st |
| `alias_map` | entries in frontmatter `aliases` array | 2nd |
| `stem_map` | filename without `.md` extension | 3rd |

**`Resolver::resolve(link) → Option<usize>`**

Looks up the link in priority order. Returns `Some(page_index)` on first match, `None` for unresolved (external) links. All comparisons are case-insensitive.

---

### `graph.rs` — Graph Construction

**Types:** `VaultGraph`, `VaultNode`, `VaultEdge`

Uses `petgraph::graph::DiGraph<VaultNode, VaultEdge>` (directed, multi-edge).

**`VaultGraph::build(pages, config) → VaultGraph`**

Two-pass construction:

**Pass 1 — create internal nodes**  
One `VaultNode` per `Page`. Indexed in `title_index` by:
- Lowercased title (primary)
- Lowercased aliases (fallback)
- Lowercased filename stem (fallback)

This `title_index` is also used for CLI node lookup (`find_node`).

**Pass 2 — create edges**  
For each page, calls `page.wikilink_fields()`. For each link in each field:
- Resolves the link via `Resolver`
- If resolved: adds a forward edge to the internal node
- If unresolved: creates an external `VaultNode` (lazily, keyed by lowercased title) and adds a forward edge to it
- For bidirectional fields: adds a second reverse edge

**`VaultGraph::find_node(title) → Option<NodeIndex>`**  
Case-insensitive lookup in `title_index`. Matches title, alias, or stem.

---

### `query.rs` — Graph Queries

**`query(vg, center, depth, direction, fields) → QueryResult`**

Standard BFS from `center`. The `direction` parameter controls which edge directions are followed:
- `Up` → `Outgoing` edges only (A depends on B, go toward B)
- `Down` → `Incoming` edges only (who depends on A, go toward sources)
- `Both` → both directions

Optional `fields` filter restricts traversal to specific edge types. Deduplicates result edges by `(from, to, field)`.

**Time complexity:** O(V_d + E_d) where V_d and E_d are the nodes and edges reachable within `depth` hops. Much less than the full graph for small depths. Worst case O(V + E) when depth is unbounded.

**`bridge(vg, from, to) → Option<Vec<(NodeIndex, NodeIndex, String)>>`**

Undirected BFS (follows both `Outgoing` and `Incoming`) from `from`, tracking a parent map. Reconstructs path on reaching `to`.

**Time complexity:** O(V + E) in the worst case (no path found — full graph explored).

**`centrality(vg, limit) → Vec<(NodeIndex, usize)>`**

Computes total degree (in + out) for every node, sorts descending, truncates to `limit`.

**Time complexity:** O(V + E) for degree counting + O(V log V) for the sort.

**`orphans(vg) → Vec<NodeIndex>`**

Returns internal nodes with zero in-degree and zero out-degree.

**`broken_links(vg) → Vec<(NodeIndex, Vec<(NodeIndex, String)>)>`**

Scans for external nodes (no backing file). For each, collects all incoming edges (source page + field name).

---

### `output.rs` — Formatters

One public function per command, each dispatching on `Format`:

| Function | Formats |
|----------|---------|
| `format_describe` | text, json |
| `format_query` | text, json, mermaid, paths |
| `format_bridge` | text, json |
| `format_centrality` | text, json |
| `format_orphans` | text, json, paths |
| `format_broken` | text, json |

**Mermaid output** (`format_query_mermaid`) assigns single-letter node IDs (`A`, `B`, ...) in order of first appearance, falling back to `N{n}` for > 26 nodes.

**JSON output** uses `serde_json::to_string_pretty`. Structs use `#[derive(Serialize)]` with `#[serde(skip_serializing_if = "Option::is_none")]` for optional fields.

---

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `clap` | 4 | CLI parsing (`derive` feature) |
| `serde` + `serde_yaml` | 1 / 0.9 | YAML frontmatter deserialization |
| `serde_json` | 1 | JSON output |
| `petgraph` | 0.7 | Directed graph + BFS |
| `walkdir` | 2 | Recursive file discovery |
| `toml` | 0.8 | `.fmg.toml` config parsing |
| `regex` | 1 | (Available; WikiLink extraction currently uses manual string scanning for zero-allocation) |

No async runtime. No web framework.

---

## Design Decisions

**Why parse-on-every-run?**  
Simplicity and correctness. For vaults up to ~50k files, cold parse is under 2 seconds. No staleness, no cache invalidation, no background process.

**Why petgraph DiGraph and not a HashMap-based adjacency list?**  
petgraph gives BFS, shortest-path, and centrality for free, with a mature API.

**Why bidirectional edges via two directed edges rather than an undirected graph?**  
Keeps the graph type uniform (`DiGraph`) and makes traversal direction explicit in query results. The output always shows the logical edge direction regardless of how the graph was traversed.

**Why `serde_yaml::Value` (untyped) rather than typed structs?**  
Frontmatter schemas are unknown at compile time. Auto-detecting WikiLink fields requires inspecting arbitrary keys and value shapes. Typed deserialization would require schema definition, which contradicts the design principle.

**Why `Option<usize>` page indices in the resolver rather than `NodeIndex`?**  
The resolver is built before the graph, so `NodeIndex` values don't exist yet. Indices are stable `Vec` positions; `NodeIndex` is assigned in the same order during graph construction.
