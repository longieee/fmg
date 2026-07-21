use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction as PetDirection;

use crate::graph::VaultGraph;

/// Traversal direction for queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalDirection {
    /// Follow outgoing edges (A depends_on B → go from A to B).
    Up,
    /// Follow incoming edges (who depends on this node?).
    Down,
    /// Follow both directions.
    Both,
}

/// A single edge in a query result.
#[derive(Debug, Clone)]
pub struct ResultEdge {
    pub from: NodeIndex,
    pub to: NodeIndex,
    pub field: String,
    pub hop: u32,
}

/// Result of a multi-hop query.
#[derive(Debug)]
pub struct QueryResult {
    pub center: NodeIndex,
    pub depth: u32,
    pub direction: TraversalDirection,
    /// Nodes discovered, with their hop distance.
    pub nodes: Vec<(NodeIndex, u32)>,
    /// Edges discovered, with hop number.
    pub edges: Vec<ResultEdge>,
}

/// BFS traversal from a center node.
pub fn query(
    vg: &VaultGraph,
    center: NodeIndex,
    depth: u32,
    direction: TraversalDirection,
    fields: Option<&[String]>,
) -> QueryResult {
    let mut visited: HashMap<NodeIndex, u32> = HashMap::new();
    let mut queue: VecDeque<(NodeIndex, u32)> = VecDeque::new();
    let mut edges: Vec<ResultEdge> = Vec::new();

    visited.insert(center, 0);
    queue.push_back((center, 0));

    while let Some((node, hop)) = queue.pop_front() {
        if hop >= depth {
            continue;
        }
        let next_hop = hop + 1;

        let directions = match direction {
            TraversalDirection::Up => vec![PetDirection::Outgoing],
            TraversalDirection::Down => vec![PetDirection::Incoming],
            TraversalDirection::Both => vec![PetDirection::Outgoing, PetDirection::Incoming],
        };

        for dir in directions {
            for edge in vg.graph.edges_directed(node, dir) {
                let edge_data = edge.weight();

                // Filter by fields if specified
                if let Some(allowed) = fields
                    && !allowed.contains(&edge_data.field) {
                        continue;
                    }

                let neighbor = if dir == PetDirection::Outgoing {
                    edge.target()
                } else {
                    edge.source()
                };

                // Determine the logical from/to for the result edge
                let (from, to) = if dir == PetDirection::Outgoing {
                    (node, neighbor)
                } else {
                    (neighbor, node)
                };

                edges.push(ResultEdge {
                    from,
                    to,
                    field: edge_data.field.clone(),
                    hop: next_hop,
                });

                if let std::collections::hash_map::Entry::Vacant(e) = visited.entry(neighbor) {
                    e.insert(next_hop);
                    queue.push_back((neighbor, next_hop));
                }
            }
        }
    }

    // Sort nodes by (hop, title) for deterministic output (ties were HashMap-order before).
    let mut nodes: Vec<(NodeIndex, u32)> = visited.into_iter().collect();
    nodes.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| vg.node(a.0).title.cmp(&vg.node(b.0).title))
    });

    // Deduplicate edges (same from/to/field)
    let mut seen_edges: HashSet<(NodeIndex, NodeIndex, String)> = HashSet::new();
    edges.retain(|e| seen_edges.insert((e.from, e.to, e.field.clone())));

    // Sort edges by (hop, from-title, to-title, field) for deterministic output.
    edges.sort_by(|a, b| {
        a.hop
            .cmp(&b.hop)
            .then_with(|| vg.node(a.from).title.cmp(&vg.node(b.from).title))
            .then_with(|| vg.node(a.to).title.cmp(&vg.node(b.to).title))
            .then_with(|| a.field.cmp(&b.field))
    });

    QueryResult {
        center,
        depth,
        direction,
        nodes,
        edges,
    }
}

/// Find the shortest path between two nodes using BFS.
pub fn bridge(
    vg: &VaultGraph,
    from: NodeIndex,
    to: NodeIndex,
) -> Option<Vec<(NodeIndex, NodeIndex, String)>> {
    if from == to {
        return Some(vec![]);
    }

    let mut visited: HashSet<NodeIndex> = HashSet::new();
    // parent map: node → (parent_node, edge_field)
    let mut parent: HashMap<NodeIndex, (NodeIndex, String)> = HashMap::new();
    let mut queue: VecDeque<NodeIndex> = VecDeque::new();

    visited.insert(from);
    queue.push_back(from);

    while let Some(node) = queue.pop_front() {
        // Search both directions for shortest path
        for dir in [PetDirection::Outgoing, PetDirection::Incoming] {
            for edge in vg.graph.edges_directed(node, dir) {
                let neighbor = if dir == PetDirection::Outgoing {
                    edge.target()
                } else {
                    edge.source()
                };

                if visited.contains(&neighbor) {
                    continue;
                }

                visited.insert(neighbor);
                parent.insert(neighbor, (node, edge.weight().field.clone()));
                queue.push_back(neighbor);

                if neighbor == to {
                    // Reconstruct path
                    let mut path = Vec::new();
                    let mut current = to;
                    while let Some((prev, field)) = parent.get(&current) {
                        path.push((*prev, current, field.clone()));
                        current = *prev;
                    }
                    path.reverse();
                    return Some(path);
                }
            }
        }
    }

    None
}

/// Compute degree centrality for all nodes.
/// Returns (NodeIndex, in_degree + out_degree) sorted by degree descending.
pub fn centrality(vg: &VaultGraph, limit: usize) -> Vec<(NodeIndex, usize)> {
    let mut degrees: Vec<(NodeIndex, usize)> = vg
        .graph
        .node_indices()
        .map(|idx| {
            let in_deg = vg
                .graph
                .edges_directed(idx, PetDirection::Incoming)
                .count();
            let out_deg = vg
                .graph
                .edges_directed(idx, PetDirection::Outgoing)
                .count();
            (idx, in_deg + out_deg)
        })
        .collect();

    // Sort by degree desc, breaking ties by title for deterministic output.
    degrees.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| vg.graph[a.0].title.cmp(&vg.graph[b.0].title))
    });
    degrees.truncate(limit);
    degrees
}

/// Find orphan nodes: nodes with no inbound or outbound edges.
pub fn orphans(vg: &VaultGraph) -> Vec<NodeIndex> {
    vg.graph
        .node_indices()
        .filter(|&idx| {
            let node = &vg.graph[idx];
            // Only report orphans that have backing files (not external nodes)
            if node.external {
                return false;
            }
            let in_deg = vg
                .graph
                .edges_directed(idx, PetDirection::Incoming)
                .count();
            let out_deg = vg
                .graph
                .edges_directed(idx, PetDirection::Outgoing)
                .count();
            in_deg == 0 && out_deg == 0
        })
        .collect()
}

/// Find broken links: external nodes (unresolved wikilinks).
pub fn broken_links(vg: &VaultGraph) -> Vec<(NodeIndex, Vec<(NodeIndex, String)>)> {
    let mut result: HashMap<NodeIndex, Vec<(NodeIndex, String)>> = HashMap::new();

    for idx in vg.graph.node_indices() {
        let node = &vg.graph[idx];
        if !node.external {
            continue;
        }
        // Find all edges pointing to this external node
        for edge in vg.graph.edges_directed(idx, PetDirection::Incoming) {
            let source = edge.source();
            result
                .entry(idx)
                .or_default()
                .push((source, edge.weight().field.clone()));
        }
    }

    let mut sorted: Vec<_> = result.into_iter().collect();
    sorted.sort_by(|a, b| {
        vg.graph[a.0]
            .title
            .to_lowercase()
            .cmp(&vg.graph[b.0].title.to_lowercase())
    });
    sorted
}
