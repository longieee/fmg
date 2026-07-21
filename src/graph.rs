use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use petgraph::graph::{DiGraph, NodeIndex};
use serde::Serialize;

use crate::config::Config;
use crate::resolve::Resolver;
use crate::vault::Page;

/// A node in the vault graph.
#[derive(Debug, Clone, Serialize)]
pub struct VaultNode {
    /// Display title.
    pub title: String,
    /// Relative path from vault root (None for external/unresolved nodes).
    pub path: Option<PathBuf>,
    /// Whether this node has a backing file in the vault.
    pub external: bool,
    /// The `type` frontmatter field, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
}

/// An edge in the vault graph.
#[derive(Debug, Clone, Serialize)]
pub struct VaultEdge {
    /// The frontmatter field that created this edge (e.g., "depends_on").
    pub field: String,
}

/// A typed cross-service (runtime) edge — a SEPARATE layer from the structural
/// `[[WikiLink]]` graph. Kept out of the petgraph so every existing structural
/// query (`query`, `bridge`, `centrality`, `describe`, …) is byte-identical whether
/// or not runtime edges exist (§7.2 edge-namespacing; §9.3 "coarse queries unaffected").
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeEdge {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub edge_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
    /// True when the target has no backing page (external service/infra node).
    pub external_target: bool,
}

/// The complete vault graph.
pub struct VaultGraph {
    pub graph: DiGraph<VaultNode, VaultEdge>,
    /// Map from title (lowercase) → NodeIndex for quick lookup.
    pub title_index: HashMap<String, NodeIndex>,
    /// All relationship field names discovered.
    pub fields: HashSet<String>,
    /// Total pages scanned (including those without frontmatter links).
    pub total_pages: usize,
    /// Pages with at least one frontmatter field.
    pub pages_with_frontmatter: usize,
    /// Typed cross-service edges — the runtime layer, parallel to `graph`.
    pub runtime_edges: Vec<RuntimeEdge>,
}

impl VaultGraph {
    /// Build the graph from parsed pages and config.
    pub fn build(pages: &[Page], config: &Config, include_body: bool) -> Self {
        let resolver = Resolver::new(
            pages,
            &config.resolve.title_field,
            &config.resolve.alias_field,
        );

        let mut graph = DiGraph::new();
        let mut title_index: HashMap<String, NodeIndex> = HashMap::new();
        let mut fields: HashSet<String> = HashSet::new();
        let mut pages_with_frontmatter = 0;

        // First pass: create a node for each page.
        let mut page_nodes: Vec<NodeIndex> = Vec::with_capacity(pages.len());
        for page in pages {
            let title = page.title(&config.resolve.title_field);
            let node_type = page
                .frontmatter
                .get("type")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if !page.frontmatter.is_empty() {
                pages_with_frontmatter += 1;
            }

            let node = VaultNode {
                title: title.clone(),
                path: Some(page.rel_path.clone()),
                external: false,
                node_type,
            };
            let idx = graph.add_node(node);
            title_index.insert(title.to_lowercase(), idx);

            // Also index by aliases and stem for CLI lookup
            for alias in page.aliases(&config.resolve.alias_field) {
                title_index.entry(alias.to_lowercase()).or_insert(idx);
            }
            title_index.entry(page.stem.to_lowercase()).or_insert(idx);
            page_nodes.push(idx);
        }

        // Second pass: create edges from wikilink fields.
        for (page_idx, page) in pages.iter().enumerate() {
            let wikilink_fields = page.wikilink_fields();
            let from_node = page_nodes[page_idx];

            for (field, links) in &wikilink_fields {
                // Check if this field is allowed by config
                if let Some(ref allowed) = config.fields.edges
                    && !allowed.contains(field) {
                        continue;
                    }

                fields.insert(field.clone());

                for link in links {
                    let to_node = if let Some(resolved_idx) = resolver.resolve(link) {
                        page_nodes[resolved_idx]
                    } else {
                        // External/unresolved node
                        let lower = link.to_lowercase();
                        *title_index.entry(lower).or_insert_with(|| {
                            graph.add_node(VaultNode {
                                title: link.clone(),
                                path: None,
                                external: true,
                                node_type: None,
                            })
                        })
                    };

                    // Add forward edge
                    graph.add_edge(
                        from_node,
                        to_node,
                        VaultEdge {
                            field: field.clone(),
                        },
                    );

                    // Add reverse edge for bidirectional fields
                    if config.direction_for(field)
                        == crate::config::Direction::Bidirectional
                    {
                        graph.add_edge(
                            to_node,
                            from_node,
                            VaultEdge {
                                field: field.clone(),
                            },
                        );
                    }
                }
            }
        }

        // Third pass: create edges from body [[WikiLinks]] (if enabled).
        if include_body {
            for (page_idx, page) in pages.iter().enumerate() {
                let body_links = page.body_wikilinks();
                let from_node = page_nodes[page_idx];

                for link in &body_links {
                    let to_node = if let Some(resolved_idx) = resolver.resolve(link) {
                        page_nodes[resolved_idx]
                    } else {
                        let lower = link.to_lowercase();
                        *title_index.entry(lower).or_insert_with(|| {
                            graph.add_node(VaultNode {
                                title: link.clone(),
                                path: None,
                                external: true,
                                node_type: None,
                            })
                        })
                    };

                    // Don't add duplicate edge if frontmatter already created one
                    let already_linked = graph.edges_connecting(from_node, to_node).next().is_some();
                    if !already_linked {
                        graph.add_edge(
                            from_node,
                            to_node,
                            VaultEdge {
                                field: "link".to_string(),
                            },
                        );
                    }
                }
            }
            fields.insert("link".to_string());
        }

        // Runtime layer: parse `cross_service:` typed edges into a SEPARATE store.
        // Nothing here touches `graph`, so structural queries stay byte-identical.
        let mut runtime_edges: Vec<RuntimeEdge> = Vec::new();
        for page in pages {
            let from_title = page.title(&config.resolve.title_field);
            for ce in page.cross_service_edges() {
                let resolved = resolver.resolve(&ce.target);
                let to_title = match resolved {
                    Some(i) => pages[i].title(&config.resolve.title_field),
                    None => ce.target.clone(),
                };
                runtime_edges.push(RuntimeEdge {
                    from: from_title.clone(),
                    to: to_title,
                    edge_type: ce.edge_type,
                    endpoint: ce.endpoint,
                    condition: ce.condition,
                    provenance: ce.provenance,
                    external_target: resolved.is_none(),
                });
            }
        }

        Self {
            graph,
            title_index,
            fields,
            total_pages: pages.len(),
            pages_with_frontmatter,
            runtime_edges,
        }
    }

    /// Runtime-layer edges touching `title` (as source or target), or ALL if `None`.
    pub fn runtime_edges_for(&self, title: Option<&str>) -> Vec<&RuntimeEdge> {
        match title {
            None => self.runtime_edges.iter().collect(),
            Some(t) => {
                let lower = t.to_lowercase();
                self.runtime_edges
                    .iter()
                    .filter(|e| e.from.to_lowercase() == lower || e.to.to_lowercase() == lower)
                    .collect()
            }
        }
    }

    /// Look up a node by title (case-insensitive).
    pub fn find_node(&self, title: &str) -> Option<NodeIndex> {
        self.title_index.get(&title.to_lowercase()).copied()
    }

    /// Look up a node by title with fuzzy/substring matching.
    /// Returns Ok(idx) for exact or unique substring match.
    /// Returns Err with candidates list for ambiguous matches, or empty vec for no match.
    pub fn fuzzy_find_node(&self, query: &str) -> Result<NodeIndex, Vec<String>> {
        // 1. Exact match first (case-insensitive)
        if let Some(idx) = self.find_node(query) {
            return Ok(idx);
        }

        // 2. Substring match on all indexed titles
        let lower = query.to_lowercase();
        let mut seen = std::collections::HashSet::new();
        let mut candidates: Vec<(NodeIndex, String)> = Vec::new();

        for (key, &idx) in &self.title_index {
            if key.contains(&lower) && seen.insert(idx) {
                candidates.push((idx, self.graph[idx].title.clone()));
            }
        }

        match candidates.len() {
            0 => Err(vec![]),
            1 => {
                let (idx, title) = &candidates[0];
                eprintln!("(matched '{}' \u{2192} \"{}\")", query, title);
                Ok(*idx)
            }
            _ => {
                candidates.sort_by_key(|a| a.1.to_lowercase());
                let titles: Vec<String> = candidates.into_iter().map(|(_, t)| t).collect();
                Err(titles)
            }
        }
    }

    /// Get node data by index.
    pub fn node(&self, idx: NodeIndex) -> &VaultNode {
        &self.graph[idx]
    }
}
