# JCode Memory System Analysis

> An in-depth analysis of the memory system from [jcode](https://github.com/1jehuang/jcode) — a high-performance Rust coding agent harness — with comparison to Vivianne's current memory implementation.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [The Four-Stage Per-Turn Pipeline](#2-the-four-stage-per-turn-pipeline)
3. [Performance Secrets](#3-performance-secrets)
   - [3a. Local ONNX Embedding (MiniLM-L6-v2)](#3a-local-onnx-embedding-minilm-l6-v2)
   - [3b. Multi-Mode Hybrid Retrieval](#3b-multi-mode-hybrid-retrieval)
   - [3c. Score-Relative Dynamic Gating](#3c-score-relative-dynamic-gating)
   - [3d. LLM Listwise Reranking](#3d-llm-listwise-reranking)
   - [3e. Memory Graph with BFS Cascade](#3e-memory-graph-with-bfs-cascade)
   - [3f. Confidence Decay with Category Half-Lives](#3f-confidence-decay-with-category-half-lives)
   - [3g. In-Memory Graph Cache](#3g-in-memory-graph-cache)
   - [3h. Embedding Model Tagging](#3h-embedding-model-tagging)
4. [Data Structures](#4-data-structures)
   - [MemoryEntry](#memoryentry)
   - [MemoryGraph](#memorygraph)
   - [MemoryStore](#memorystore)
   - [MemoryCategory](#memorycategory)
5. [Memory Types (Files in the Crate)](#5-memory-types-files-in-the-crate)
6. [Comparison: JCode vs Vivianne](#6-comparison-jcode-vs-vivianne)
7. [Key Takeaways](#7-key-takeaways)
8. [Potential Upgrades for Vivianne](#8-potential-upgrades-for-vivianne)

---

## 1. Architecture Overview

jcode's memory system is a purpose-built **multi-stage retrieval pipeline** designed for **maximum relevance per token** — not just retrieval speed. The core design philosophy is:

> **"Embrace the hybrid"** — combine multiple retrieval strategies (dense embeddings, BM25 lexical search, and graph BFS) rather than relying on a single approach, then use a cheap LLM sidecar to rerank the small candidate pool.

It lives under these crates:

| Crate | Purpose |
|---|---|
| `jcode-base/src/memory*.rs` | Core memory manager, pipeline, agent, types |
| `jcode-memory-types/` | Shared type definitions (MemoryEntry, MemoryGraph, Edge, Cluster, etc.) |
| `jcode-embedding/` | Standalone ONNX inference crate for MiniLM-L6-v2 embeddings |
| `jcode-app-core/src/tool/memory.rs` | Agent tool interface (remember/recall/search/forget/tag/link) |
| `jcode-app-core/src/tool/conversation_search.rs` | Full-text session search |
| `jcode-app-core/src/tool/session_search*.rs` | Session-level RAG search with index |

---

## 2. The Four-Stage Per-Turn Pipeline

Every turn, the memory agent runs this pipeline:

```
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│  Search   │ →  │  Verify   │ →  │  Inject   │ →  │ Maintain  │
│ (dense /  │    │ (Haiku   │    │ (pending │    │ (graph   │
│  BM25 /   │    │  relev-  │    │  memory  │    │  garden- │
│  graph)   │    │  ance)   │    │  queue)  │    │  ing)    │
└──────────┘    └──────────┘    └──────────┘    └──────────┘
     │                │               │               │
     │ 3-5ms          │ ~200ms        │ ~0ms          │ ~500ms+
     │ (ONNX local)   │ (LLM call)    │ (shared       │ (async,off
     │                │               │  state)       │  critical
     └────────────────┴───────────────┴───────────────┴── path)
```

### Stage 1: Search

- Runs **embedding similarity** (cosine on 384-dim MiniLM vectors) over all active memories
- Falls back to **BM25 lexical** search for memories with mismatched or missing embeddings
- Optionally runs **graph BFS cascade** for related-memory discovery
- Results combined via **RRF (Reciprocal Rank Fusion)** — no score normalization needed
- **LRU cache** (128 entries) avoids re-embedding repeated text

### Stage 2: Verify

- Haiku LLM sidecar judges whether each candidate is *actually* relevant to the current context
- Uses the **focused query** (latest user intent, noise-stripped) for accuracy
- Rejects false positives from the pure embedding search
- Has a **failure backoff circuit breaker**: 30s for transient errors, 5min for auth failures
- Tracks progress: `verify_progress: Option<(usize, usize)>`

### Stage 3: Inject

- Verified memories are placed into a **global pending queue** (`PENDING_MEMORY`)
- The main agent picks them up before its next LLM call
- **Deduplication**: `is_memory_injected_any()` prevents the same memory from being injected twice in the same session
- **Mark-known**: Agent-stored memories (via `memory` tool) are immediately marked as "already injected" to avoid re-surfacing

### Stage 4: Maintain (Async, Non-Blocking)

- Runs **after** the agent turn starts responding — never on the critical path
- Operations:
  - **Confidence adjustment**: boost memories that were verified, decay ones that were rejected
  - **Link inference**: discover `RelatesTo` edges between related memories
  - **Cluster refinement**: re-compute centroids, merge/detect clusters
  - **Tag inference**: suggest shared tags for cluster members
  - **Gap detection**: identify topics with insufficient coverage
  - **Staleness pruning**: mark superseded memories as inactive

---

## 3. Performance Secrets

### 3a. Local ONNX Embedding (MiniLM-L6-v2)

**384-dimensional vectors, ~3ms per inference, fully in-process.**

```rust
// jcode-embedding/src/lib.rs
pub struct Embedder { inner: backend::Embedder }

impl Embedder {
    pub fn load() -> Result<Self> { /* ONNX model from disk */ }
    pub fn embed(&self, text: &str) -> Result<EmbeddingVec> { /* ~3ms */ }
    pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<EmbeddingVec>> { /* ~5ms for 10 */ }
}
```

Key advantages:
- **No network call** — runs in the same process via ONNX Runtime
- **No Python subprocess** — pure Rust, no GIL, no IPC overhead
- **Batched inference** — processing 10 texts costs barely more than 1
- **LRU cache** (128 entries) — repeated text (e.g., code snippets) never recomputed
- **Process-wide singleton** — one model shared across all sessions
- **Separate workspace crate** `jcode-embedding` — unchanged code stays cached across self-dev rebuilds

### 3b. Multi-Mode Hybrid Retrieval

jcode uses **three fused retrieval strategies** — not just embeddings:

| Mode | Method | When Used |
|---|---|---|
| **Dense (semantic)** | Cosine similarity on 384-dim embeddings | Primary — every turn |
| **Lexical (BM25-like)** | Pre-computed `search_text` field + fuzzy word matching | Fallback for mismatched/missing embeddings |
| **Graph cascade** | BFS traversal across `RelatesTo`, `HasTag`, `InCluster` edges | "Related" queries, manual recall |

The lexical fallback is critical: memories from different embedding models (tagged by `embedding_model`) are still reachable via BM25 text search, even if their vectors are from a different vector space.

Results from each mode are fused via **RRF** — no score normalization needed across different rankers.

### 3c. Score-Relative Dynamic Gating

**The single most impactful optimization.** Instead of always injecting the top-5 memories:

```rust
const GATE_REL_FLOOR: f32 = 0.90;   // must be within 90% of top score
const GATE_DROP_RATIO: f32 = 0.95;  // must be within 95% of previous kept

fn dynamic_gate_select(candidates: Vec<(Entry, f32)>, max_k: usize) -> Vec<(Entry, f32)> {
    let top = candidates[0].1.max(f32::MIN_POSITIVE);
    let mut prev = top;
    let mut out = Vec::new();
    for (entry, sim) in candidates.into_iter().take(max_k) {
        if !out.is_empty() && (sim < top * GATE_REL_FLOOR || sim < prev * GATE_DROP_RATIO) {
            break;  // ← first meaningful gap cuts the tail
        }
        prev = sim;
        out.push((entry, sim));
    }
    out
}
```

**Results** (benchmarked on self-dev corpus, 150 query windows):
- **Avg injected memories**: 5.0 → ~2.25/turn (−55%)
- **Precision@5**: 0.23 → 0.36 (+56%)
- **Cost**: Zero (pure heuristic, no LLM call)
- **Can't reach 0**: The cosine-quality floor means ~1 memory always passes; true 0-injection requires the LLM precision rerank.

### 3d. LLM Listwise Reranking

The hybrid retrieval pulls ~99% of relevant memories into the pool but ranks them poorly (only ~53% in top 5). A **listwise LLM reranker** fixes this:

```rust
// memory_rerank.rs
pub const LLM_RERANK_SYSTEM: &str =
    "You re-rank stored MEMORIES by how useful each would be to surface
     to an AI coding agent for the CURRENT request...";

pub fn build_rerank_prompt(focused_query: &str, candidates: &[(String, String)]) -> String {
    // One-shot listwise prompt: all candidates in numbered list
    // Returns JSON array like [3, 1, 7]
}
```

**Iteration 1 → Iteration 2 improvement:**
- **Recall@5**: 0.50 → 0.75 (+50%)
- **Precision@5**: 0.28 → 0.35 (+25%)

Key design choices:
- **One listwise call** (not pairwise O(n²)) — feeds all candidates at once
- **Focused query** — strips system messages, tool noise; keeps latest user intent
- **Hard caps**: each candidate ≤ 600 chars, query ≤ 4000 chars
- **Failure backoff**: 30s for transient errors, 5min for auth failures

### 3e. Memory Graph with BFS Cascade

A **semantic graph** organizes memories with typed edges:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EdgeKind {
    HasTag,                    // traversal weight: 0.8
    InCluster,                 // traversal weight: 0.6
    RelatesTo { weight: f32 }, // traversal weight: variable
    Supersedes,                // traversal weight: 0.9
    Contradicts,               // traversal weight: 0.3
    DerivedFrom,               // traversal weight: 0.7
}
```

The graph enables **BFS cascade retrieval**: starting from a seed memory, follow edges with decaying traversal weights to find indirectly related memories. A `RelatesTo` edge with weight 0.8 followed by another at 0.7 gives a combined weight of 0.56 — highly relevant second-degree connections still surface.

### 3f. Confidence Decay with Category Half-Lives

Each memory category has its own **half-life** for automatic decay:

| Category | Half-Life | Rationale |
|---|---|---|
| `Correction` | 365 days | User corrections are permanently valuable |
| `Preference` | 90 days | Preferences evolve slowly |
| `Fact` | 30 days | Codebase facts can become stale quickly |
| `Entity` | 60 days | Entities change at a moderate pace |
| `Custom` | 45 days | Default for custom categories |

```rust
pub fn effective_confidence(&self) -> f32 {
    let age_days = (Utc::now() - self.created_at).num_days() as f32;
    let decay = (-age_days / half_life * 0.693).exp();
    let access_boost = 1.0 + 0.1 * (self.access_count as f32 + 1.0).ln();
    (self.confidence * decay * access_boost).min(1.0)
}
```

**Effect**: Old irrelevant memories naturally sink in rankings without explicit deletion. Frequently accessed memories get a boost. It's automated memory management at zero runtime cost.

### 3g. In-Memory Graph Cache

```rust
struct GraphCache { entries: HashMap<PathBuf, GraphCacheEntry> }

fn cached_graph(path: &PathBuf) -> Option<MemoryGraph> {
    if entry.modified == graph_mtime(path) { Some(entry.graph) }
    else { None }  // stale → caller reloads from disk
}
```

- Full memory graph is **cached in memory** across all sessions
- **mtime invalidation**: disk writes bump the file mtime; cache returns `None` only when stale
- Repeated reads (common in a long-running agent) are effectively **free**

### 3h. Embedding Model Tagging

Each `MemoryEntry` stores which embedding model produced its vector:

```rust
pub embedding_model: Option<String>
// None → legacy local MiniLM (all-MiniLM-L6-v2)
```

During search, cosine similarity is **only computed between vectors from the same model**:

```rust
pub fn embedding_matches_model(&self, model: &str) -> bool {
    self.embedding.is_some() && self.effective_embedding_model() == model
}
```

Legacy (untagged) memories are treated as `"minilm-l6-v2"`. Mismatched memories are still reachable via the **lexical fallback** (BM25-like text search). This means:

- **Seamless model upgrades**: switch to a new embedding model without re-embedding the entire store
- **No cold start**: old memories remain accessible via text search until re-embedded
- **Correct comparisons**: no apples-to-oranges cosine comparison across vector spaces

---

## 4. Data Structures

### MemoryEntry

```rust
pub struct MemoryEntry {
    pub id: String,
    pub category: MemoryCategory,
    pub content: String,
    pub tags: Vec<String>,
    pub search_text: String,              // Pre-normalized for BM25
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub access_count: u32,
    pub source: Option<String>,
    pub trust: TrustLevel,                // High / Medium / Low
    pub strength: u32,                    // Reinforcement count
    pub active: bool,                     // False if superseded
    pub superseded_by: Option<String>,
    pub reinforcements: Vec<Reinforcement>, // Breadcrumb trail
    pub embedding: Option<Vec<f32>>,      // 384-dim MiniLM vector
    pub embedding_model: Option<String>,  // Model id for this vector
    pub confidence: f32,                  // 0.0-1.0, decays over time
}
```

### MemoryGraph

```rust
pub struct MemoryGraph {
    pub entries: Vec<MemoryEntry>,
    pub edges_out: HashMap<String, Vec<Edge>>,  // source id → edges
    pub clusters: HashMap<String, ClusterEntry>,
    pub tags: HashMap<String, TagEntry>,
    pub metadata: GraphMetadata,
    pub version: u32,
}
```

### MemoryStore

```rust
pub struct MemoryStore {
    pub entries: Vec<MemoryEntry>,
    pub metadata: HashMap<String, String>,
}
```

Simple flat store — used for serialization. The `MemoryGraph` adds graph-like relationships on top.

### MemoryCategory

```rust
pub enum MemoryCategory {
    Fact,        // Things about the codebase
    Preference,  // User preferences
    Entity,      // People, projects, concepts
    Correction,  // User corrections
    Custom(String), // Extensible
}
```

---

## 5. Memory Types (Files in the Crate)

| File | Purpose |
|---|---|
| `jcode-memory-types/src/lib.rs` | Shared types: MemoryActivity, PipelineState, MemoryState, MemoryEvent, MemoryEntry, MemoryStore, ranking helpers |
| `jcode-memory-types/src/graph.rs` | Graph types: Edge, EdgeKind, TagEntry, ClusterEntry, MemoryGraph, BFS cascade logic |
| `jcode-base/src/memory.rs` | MemoryManager: CRUD, search, hybrid retrieval, scope management, sidecar integration |
| `jcode-base/src/memory_graph.rs` | Compatibility re-export for graph types |
| `jcode-base/src/memory_types.rs` | Re-exports from `jcode-memory-types` |
| `jcode-base/src/memory_agent.rs` | Per-turn pipeline orchestrator: search → verify → inject → maintain |
| `jcode-base/src/memory_rerank.rs` | Listwise LLM reranking (Recall-5, Mode-2) |
| `jcode-base/src/memory_prompt.rs` | Prompt templates for extraction and relevance checking |
| `jcode-base/src/memory_judge_metrics.rs` | Offline benchmark metrics |
| `jcode-base/src/memory_tests.rs` | Unit tests |
| `jcode-base/src/memory/cache.rs` | In-memory graph cache with mtime invalidation |
| `jcode-base/src/memory/activity.rs` | Activity tracking and event emission |
| `jcode-base/src/memory/pending.rs` | Pending memory queue for cross-session injection |
| `jcode-base/src/embedding.rs` | Embedding facade: process-wide singleton, LRU cache, stats |
| `jcode-base/src/embedding_backend.rs` | Backend abstraction (local ONNX vs remote API) |
| `jcode-base/src/embedding_stub.rs` | No-op stub for builds without ONNX support |
| `jcode-embedding/src/lib.rs` | Pure ONNX inference crate (separate for build caching) |
| `jcode-app-core/src/tool/memory.rs` | Agent tool interface (remember/recall/search/forget/tag/link/related) |
| `jcode-app-core/src/tool/session_search.rs` | Session-level RAG over past conversations |
| `jcode-app-core/src/tool/session_search_index.rs` | Inverted index for session search |
| `jcode-app-core/src/tool/session_search_tests.rs` | Tests for session search |
| `jcode-protocol/src/protocol_memory.rs` | Protocol types for TUI/UI rendering |

---

## 6. Comparison: JCode vs Vivianne

| Aspect | **JCode** | **Vivianne (Current)** |
|---|---|---|
| **Retrieval** | Multi-mode hybrid (dense + BM25 + graph BFS) | Simple `cmd_memory_recall` with query string |
| **Embedding** | Local ONNX MiniLM (384-dim, ~3ms, in-process) | Backend Rust whisper model (not for memory) |
| **Ranking** | 3-stage: hybrid → LLM listwise rerank → dynamic gate | Simple keyword match |
| **Graph** | Full semantic graph with typed edges, clusters, tags, BFS cascade | Flat memory store |
| **Categories** | Fact, Preference, Entity, Correction + Custom with decay half-lives | Not categorized |
| **Confidence** | Exponential time decay + access boost + category-specific half-lives | Not tracked |
| **LLM Judge** | Haiku sidecar per turn (relevance verification + extraction) | Not implemented |
| **Pipeline** | Async 4-stage (search→verify→inject→maintain) per turn | Single synchronous query |
| **Caching** | Graph in-memory cache (mtime-invalidated) + 128-entry embedding LRU | Not cached |
| **Deduplication** | Content-based dedup + `is_memory_injected_any()` check | Not implemented |
| **Memory per turn** | ~2.25 avg injected (variable-k), not fixed top-5 | N/A |
| **Maintenance** | Automatic: confidence decay, cluster refinement, link inference, tag inference | Manual purge only |
| **Synthetic entries** | Skills registered as synthetic memory entries | Separate skill list |
| **Legacy handling** | Embedding model tagging enables seamless upgrades | N/A |
| **Storage format** | JSON files per project (hashed by directory path) | JSON via backend |

---

## 7. Key Takeaways

### What Makes JCode's Memory "Insanely Fast" — The TL;DR

1. **Local ONNX inference** — No network call for embeddings. 384-dim MiniLM at ~3ms.
2. **Process-wide model singleton** — One model shared across all sessions, zero per-session overhead.
3. **In-memory graph cache** — Full graph cached with mtime invalidation; repeated reads are free.
4. **Variable-k injection** — Dynamic gate reduces injected memories from 5.0 → ~2.25 avg per turn.
5. **Async maintenance** — Background gardening (confidence decay, cluster refinement) off the critical path.
6. **Listwise LLM reranking** — One Haiku call (not pairwise O(n²)) to order the already-small pool.
7. **Legacy model tagging** — Avoids re-embedding the entire store on model upgrades.
8. **Separate workspace crate** — ONNX loading cached across self-dev rebuilds.

### The Architecture Motto

> **"Embrace the hybrid"** — Don't rely on a single retrieval method. Combine dense embeddings (for semantic understanding), BM25 (for lexical/gap coverage), and graph BFS (for relational discovery). Then use a cheap LLM (Haiku) to rerank the small pool rather than trying to make a single method perfect.

---

## 8. Potential Upgrades for Vivianne

Based on this analysis, here are the most impactful improvements Vivianne could adopt:

### Tier 1 (High Impact, Low Effort)

1. **Variable-k dynamic gating** — Replace fixed top-K recall with jcode's score-relative gate. Pure heuristic, no LLM cost.
2. **In-memory graph cache** — Add mtime-invalidated cache for the memory store. Small code change, big perf win.
3. **Confidence decay** — Add category half-lives to auto-sink stale memories without explicit deletion.

### Tier 2 (Medium Impact, Medium Effort)

4. **Multi-mode hybrid retrieval** — Combine embedding cosine similarity with BM25 text fallback. Implement RRF fusion for combined ranking.
5. **Memory categories** — Add `MemoryCategory` enum and per-category half-life decay to Vivianne's `memory_store.rs`.
6. **Embedding model tagging** — Tag each memory with its embedding model to enable seamless upgrades.

### Tier 3 (High Impact, High Effort)

7. **Dedicated ONNX embedding crate** — Port `jcode-embedding` approach to Vivianne's Rust backend. 384-dim MiniLM in-process.
8. **Full 4-stage async pipeline** — Implement the search→verify→inject→maintain pipeline in the coding agent's per-turn loop.
9. **Memory graph with BFS** — Add typed edges (`RelatesTo`, `Supersedes`, `Contradicts`) and BFS cascade retrieval.
10. **LLM sidecar reranker** — Use a cheap model (Haiku or equivalent) to verify relevance before injection.

---

*Analysis performed July 2026. JCode repository: [github.com/1jehuang/jcode](https://github.com/1jehuang/jcode)*
