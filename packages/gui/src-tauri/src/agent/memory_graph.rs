//! Memory graph with typed edges and BFS cascade retrieval — ported from
//! jcode's `memory_types::graph` module (§3e in the deep-dive). Adds
//! relational discovery on top of the flat memory store: entries can be
//! linked via `RelatesTo`, `Supersedes`, `Contradicts`, `HasTag`, and
//! `InCluster` edges, and a BFS traversal starting from seed entries can
//! surface indirectly related memories that pure embedding/BM25 wouldn't
//! catch.
//!
//! The graph is stored in a `memory_edges` table alongside the existing
//! `memories` table, and edges are created explicitly via the `link` tool
//! (or inferred during background maintenance).

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// Typed relationships between memory entries. Each has a traversal weight
/// that controls how far BFS cascade propagates (multiplied as depth
/// increases — two hops at 0.7 each gives a combined 0.49 weight).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum EdgeKind {
    /// Shared tag — traversal weight 0.8.
    HasTag { tag: String },
    /// Part of the same cluster — traversal weight 0.6.
    InCluster { cluster_id: String },
    /// Related in some way (the `weight` field captures how strongly) — default 0.7.
    RelatesTo { weight: f32 },
    /// Newer entry fully replaces an older one — traversal weight 0.9.
    Supersedes,
    /// Entries contradict each other — traversal weight 0.3 (low; contradictions
    /// should be surfaced cautiously).
    Contradicts,
    /// Entry was derived/transformed from another — traversal weight 0.7.
    DerivedFrom,
}

impl EdgeKind {
    /// The traversal weight used during BFS cascade. Higher = more likely to
    /// surface the connected entry even at depth.
    pub fn weight(&self) -> f32 {
        match self {
            EdgeKind::HasTag { .. } => 0.8,
            EdgeKind::InCluster { .. } => 0.6,
            EdgeKind::RelatesTo { weight } => *weight,
            EdgeKind::Supersedes => 0.9,
            EdgeKind::Contradicts => 0.3,
            EdgeKind::DerivedFrom => 0.7,
        }
    }
}

/// A directed edge from `source` to `target` entry id.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub kind: EdgeKind,
    pub created_at: u64,
}

/// A named cluster (topic group) that multiple memories belong to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterEntry {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: u64,
    pub member_count: usize,
}

/// A tag associated with multiple memory entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagEntry {
    pub name: String,
    pub description: Option<String>,
    pub entry_count: usize,
}

/// BFS cascade result: a memory entry paired with the cumulative traversal
/// weight from the seed set. Higher weight = more strongly related.
#[derive(Debug, Clone)]
pub struct CascadeResult {
    pub entry_id: String,
    pub cumulative_weight: f32,
    pub depth: usize,
}

/// BFS cascade retrieval: starting from `seed_ids`, follow typed edges with
/// decaying weights to find indirectly related memories. `max_depth` caps
/// how many hops to traverse (3 is enough for most use cases). `weight_floor`
/// stops propagation once the cumulative weight drops below this threshold.
///
/// The full edge set is passed in as `all_edges` — the caller (memory_store)
/// loads all edges from SQLite once rather than making per-hop queries.
pub fn bfs_cascade(
    seed_ids: &[String],
    all_edges: &[Edge],
    max_depth: usize,
    weight_floor: f32,
) -> Vec<CascadeResult> {
    if seed_ids.is_empty() || max_depth == 0 {
        return Vec::new();
    }

    // Build adjacency list: source -> [(target, kind)]
    let mut adjacency: HashMap<&str, Vec<(&str, &EdgeKind)>> = HashMap::new();
    for edge in all_edges {
        adjacency
            .entry(edge.source.as_str())
            .or_default()
            .push((edge.target.as_str(), &edge.kind));
    }

    let seed_set: HashSet<&str> = seed_ids.iter().map(|s| s.as_str()).collect();
    let mut visited: HashSet<String> = seed_set.iter().map(|s| (*s).to_string()).collect();
    let mut queue: VecDeque<(&str, f32, usize)> = VecDeque::new();
    let mut results: Vec<CascadeResult> = Vec::new();

    // Seed the queue: edges from any seed entry
    for seed in &seed_set {
        if let Some(neighbors) = adjacency.get(seed) {
            for (target, kind) in neighbors {
                let w = kind.weight();
                let target_str = (*target).to_string();
                if !visited.contains(&target_str) {
                    visited.insert(target_str.clone());
                    queue.push_back((*target, w, 1));
                }
            }
        }
    }

    // BFS with decaying weights
    while let Some((entry_id, cum_weight, depth)) = queue.pop_front() {
        if cum_weight < weight_floor {
            continue;
        }
        results.push(CascadeResult {
            entry_id: entry_id.to_string(),
            cumulative_weight: cum_weight,
            depth,
        });

        if depth >= max_depth {
            continue;
        }

        if let Some(neighbors) = adjacency.get(entry_id) {
            for (target, kind) in neighbors {
                let w = cum_weight * kind.weight();
                let target_str = (*target).to_string();
                if !visited.contains(&target_str) {
                    visited.insert(target_str.clone());
                    queue.push_back((*target, w, depth + 1));
                }
            }
        }
    }

    // Sort by cumulative weight descending (strongest connections first)
    results.sort_by(|a, b| b.cumulative_weight.total_cmp(&a.cumulative_weight));
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(source: &str, target: &str, kind: EdgeKind) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            kind,
            created_at: 0,
        }
    }

    #[test]
    fn bfs_from_seed_finds_direct_neighbors() {
        let edges = vec![
            edge("a", "b", EdgeKind::RelatesTo { weight: 0.8 }),
            edge("a", "c", EdgeKind::HasTag { tag: "rust".into() }),
        ];
        let result = bfs_cascade(&["a".into()], &edges, 3, 0.1);
        assert_eq!(result.len(), 2);
        // b has weight 0.8, c has weight 0.8 (HasTag also 0.8)
        for r in &result {
            assert!(r.depth == 1);
        }
    }

    #[test]
    fn bfs_two_hops_decays_weight() {
        let edges = vec![
            edge("a", "b", EdgeKind::RelatesTo { weight: 0.8 }),
            edge("b", "c", EdgeKind::RelatesTo { weight: 0.7 }),
        ];
        let result = bfs_cascade(&["a".into()], &edges, 3, 0.1);
        assert_eq!(result.len(), 2);
        let c = result.iter().find(|r| r.entry_id == "c").unwrap();
        assert!((c.cumulative_weight - 0.56).abs() < 0.01); // 0.8 * 0.7
        assert_eq!(c.depth, 2);
    }

    #[test]
    fn bfs_avoids_cycles() {
        let edges = vec![
            edge("a", "b", EdgeKind::RelatesTo { weight: 0.8 }),
            edge("b", "a", EdgeKind::RelatesTo { weight: 0.8 }),
            edge("b", "c", EdgeKind::RelatesTo { weight: 0.7 }),
        ];
        let result = bfs_cascade(&["a".into()], &edges, 3, 0.1);
        assert_eq!(result.len(), 2); // b and c, but not a (already visited)
    }

    #[test]
    fn bfs_respects_weight_floor() {
        let edges = vec![
            edge("a", "b", EdgeKind::Contradicts), // weight 0.3
            edge("b", "c", EdgeKind::RelatesTo { weight: 0.8 }),
        ];
        // weight_floor of 0.5 should stop at b (0.3 < 0.5)
        let result = bfs_cascade(&["a".into()], &edges, 3, 0.5);
        assert_eq!(result.len(), 0);

        // Same query with lower floor finds b and then c (0.3 * 0.8 = 0.24)
        let result = bfs_cascade(&["a".into()], &edges, 3, 0.1);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn bfs_empty_seeds_returns_empty() {
        assert!(bfs_cascade(&[], &[], 3, 0.1).is_empty());
    }

    #[test]
    fn edge_kind_weights_are_as_expected() {
        assert!((EdgeKind::Supersedes.weight() - 0.9).abs() < 0.01);
        assert!((EdgeKind::HasTag { tag: "x".into() }.weight() - 0.8).abs() < 0.01);
        assert!((EdgeKind::InCluster { cluster_id: "x".into() }.weight() - 0.6).abs() < 0.01);
        assert!((EdgeKind::RelatesTo { weight: 0.75 }.weight() - 0.75).abs() < 0.01);
        assert!((EdgeKind::Contradicts.weight() - 0.3).abs() < 0.01);
        assert!((EdgeKind::DerivedFrom.weight() - 0.7).abs() < 0.01);
    }
}
