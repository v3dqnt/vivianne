# Plan: bringing Hindsight's memory ideas into Vivianne's memory module

Source: `C:\Users\v3dqn\Work\Project Versa\hindsight` (the user's fork of vectorize-io/hindsight, MIT, "Agent Memory That Learns"). Target: `packages/gui/src-tauri/src/agent/memory/` (about 4,700 lines of Rust).

Read for this plan: Hindsight's README and its developer docs (retain, retrieval, observations, mental models, pages, reflect, decision models, memory defense), the engine's file layout, and Vivianne's memory module headers and public API. The 24,000-line `memory_engine.py` was not read; every Hindsight mechanism below is from its docs, so each phase starts by reading the matching engine file.

## What Hindsight is, in one paragraph

A Python (FastAPI) server over PostgreSQL (embedded `pg0`, or external), plus an LLM for extraction, embeddings, a cross-encoder reranker and optional small "decision models". Three operations: **retain** (an LLM extracts rich facts, entities, times and causal links from content), **recall** (four strategies in parallel — semantic, keyword, graph, temporal — fused by RRF, cross-encoder reranked, boosted for recency, temporal fit and evidence count, packed to a token budget), **reflect** (an agent loop that reasons over memory in the order mental models, then observations, then raw facts, under the bank's disposition and directives). Background **consolidation** turns facts into evidence-grounded **observations**; **mental models / pages** are standing answers that refresh themselves incrementally. Clients exist for Python, TypeScript and Go; the Rust `hindsight-cli` is only an HTTP client. Hindsight also ships an MCP server and `hindsight-embed`, a local daemon.

## Where Vivianne already is

Vivianne's memory is not basic. It has: SQLite with FTS5 (BM25) and sqlite-vec; hybrid recall fused by RRF; a dynamic relevance gate; an LLM listwise reranker with a topic-stability gate; a typed memory graph (RelatesTo, Supersedes, Contradicts, HasTag, InCluster) with BFS cascade retrieval; reconcile-on-write (ADD / UPDATE / DELETE / NOOP, with a novelty gate and provenance rules); sleep-time passes (re-embed, episode summaries, dedupe, forget by decayed confidence); "as of" queries; a persona and an always-in-context core block; a project store and a user-wide store; secret scrubbing before writes; an in-repo benchmark (`bench.rs`).

## Where Hindsight adds something Vivianne lacks

| Hindsight idea | Vivianne today | Gap |
|---|---|---|
| **Entities**: people, projects, libraries and concepts recognised, resolved (fuzzy name plus co-occurrence) and linking every fact that mentions them | Edges between memories; no entity nodes | Large. "Everything about X" is the most natural question for a coding agent |
| **Two times**: when it happened versus when it was learned; time expressions in the query parsed to a window; a temporal retrieval strategy and boost | `created_at`, validity and archive for "as of" | Medium |
| **Observations**: consolidated beliefs, each citing its supporting facts, updated when evidence changes (a changed preference becomes "used to prefer X, now prefers Y"), with a staleness flag and a proof count | Reconcile-on-write rewrites or closes facts; episodes summarise sessions | Medium: Vivianne's facts overwrite, Hindsight's beliefs keep the evolution and the evidence |
| **Mental models / pages**: named questions whose answers are kept current in the background, edited incrementally rather than regenerated | Persona bullets and the core block | Medium. Project pages ("architecture", "conventions", "how we release") would be useful |
| **Reflect**: an agent that searches pages, then observations, then facts, and answers with citations | The model calls `recall` itself | Small to medium: a `reflect` tool, mostly prompt and loop |
| **Token-budgeted recall** (`max_tokens`, depth levels) and source-chunk expansion | Fixed hit cap and byte cap | Small |
| **Causal links** | None | Small |
| **Memory defense**: 45 secret and PII patterns, redact or block at retain, with audit | A smaller heuristic `redact_secrets` | Small, cheap, and a clear win |
| **Fact perspective**: the agent's own experience versus facts about the world | `kind` field | Small |
| Cross-encoder reranker and trained decision models | LLM listwise reranker | A local cross-encoder is optional; the trained models' weights and licence were not checked |

## Three ways to integrate, and a recommendation

- **A. Hindsight as a backend.** Vivianne's `remember` and `recall` call a running Hindsight server. Fastest to the full feature set. But it needs Python, a database, an LLM and models running next to a local-first desktop app, and it is hard on Windows. Reasonable only as an option.
- **B. Port the mechanisms into the Rust memory module.** Local-first, one SQLite file, no new runtime, and it reuses what Vivianne already has (RRF, graph, sleep passes, embeddings, the cheap model for extraction). Costs real work. Hindsight is MIT, so porting with attribution is fine.
- **C. B as the default, plus an optional connector to an external Hindsight server** for people who already run one, merged into recall by the RRF code that exists.

**Recommendation: B, then C.** Also, before writing anything: Hindsight's MCP server (`docs/developer/mcp-server.md`) can be added today in Vivianne's MCP settings to try its behaviour on real work, with no code. That is untested here.

## Phases

Each phase: tests first, a clean build, and a before/after number from the benchmark.

**Phase 0: baseline.** Extend `bench.rs` with a long-term-memory question set. Hindsight's `hindsight-system-evals` and its LongMemEval harness show the format; read them first. Record recall quality and cost on the current module so every later phase has a number to beat. Decision: which dataset, and whether the LLM judge may be a cheap model.

**Phase 1: memory defense.** Port Hindsight's 45 patterns (and the redact-or-block policy) into `redact_secrets` in `memory/mod.rs`. Add a per-store policy, a `[REDACTED:type]` marker and a count in memory status. Small and safe; ship first. Test: a table of each pattern's positive and negative cases.

**Phase 2: entities and two times.** Schema: `entities`, `entity_mentions(entry_id, entity_id)`, plus `occurred_at` and a perspective on entries. Fill them from the existing L1 extraction call by extending its output, so there is no new LLM call per turn. Entity resolution: normalised name similarity plus shared co-mentions, with a conservative merge threshold and a way to undo a merge. Add entity-linked expansion as a retrieval strategy beside the BFS graph. Backfill existing memories in a sleep pass. Read first: `engine/graph_maintenance.py`, the entity code under `engine/`.

**Phase 3: temporal recall.** Parse time expressions in the query ("last spring", "before the migration", "in 2024") to a window using `chrono` and simple rules, with an LLM fallback only when the rules find a time word they cannot read. Add a temporal strategy and the recency and temporal-fit boosts as bounded multipliers on the fused score (Hindsight's alpha form: `1 + a(signal - 0.5)`). Read first: `engine/search/temporal_extraction.py`, `engine/query_analyzer.py`.

**Phase 4: observations.** A new kind with evidence ids, a history of its previous texts, a staleness flag, and a proof count. A consolidation step in the sleep pass compares new facts with existing observations by similarity first (clear restatements attach as evidence; chatter is skipped), and only the uncertain remainder pays for an LLM call that creates or refines an observation. Keep Vivianne's provenance rule: an inferred observation never rewrites something the user asked to be remembered. Read first: `engine/consolidation/` (`consolidator.py`, `triage.py`, `prompts.py`).

**Phase 5: project pages.** Named questions per project, each an auto-maintained document built from observations, edited incrementally rather than regenerated, kept with the facts they came from, and listed in the core block by title only (bodies are searched on demand, not injected). Page definitions live in the project's memory settings. Read first: `engine/mental_model_refresh.py`.

**Phase 6: `reflect` tool.** A bounded loop (at most about 6 steps) over: search pages, search observations, recall facts, expand a source. Returns an answer with the ids it actually retrieved as citations. Honour Vivianne's existing rules instead of copying Hindsight's disposition traits. Gate it by plan mode and the memory on/off switch.

**Phase 7: budgeted recall.** Replace the fixed hit cap with a token budget and a depth setting that controls candidate count, graph hops and whether the reranker runs; add source-text expansion for a hit.

**Phase 8: optional connector.** A small HTTP client for a running Hindsight server (retain, recall, reflect), exposed as a second store whose hits join recall through the existing RRF. Off by default; the base URL and key live in settings and the keychain.

**Phase 9: migration, settings and docs.** SQLite migrations with a backup before the first one, a Settings → Memory section for the new switches (defense policy, entities, pages), and a docs page.

## Risks and open decisions

- **Cost.** Entity, time and causal extraction ride on the existing batched L1 call; observation consolidation and page refresh are the new LLM spend, and both are gated by similarity and by idle time.
- **Entity merge errors** are worse than missed merges: start conservative and keep merges reversible.
- **Not ported:** multi-tenant banks, Postgres, webhooks, audit log, Hindsight's own trained models, the control-plane UI, and tags as a security boundary (Vivianne's project and user stores already isolate).
- **Licence:** MIT; keep a copyright and attribution note in each ported file. The Hindsight model weights on Hugging Face were not checked and are not part of this plan.
- **Decide before Phase 2:** whether entity-centred recall should also surface in the always-on core block, or stay on demand.
