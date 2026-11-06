# DeepSeek Harness `packages/core` against Vivianne

Written 2026-11-06 from the sources at `deepseek-harness/packages/core` (about 13,000 lines of TypeScript across 10 packages) and the Vivianne worktree `ui-polish`. I read each package's README and the key source files (`session/repair.ts`, the compaction family, and the tool pipeline notes); I did not read every line. Where a claim is about Vivianne, it comes from a search of the code, and the result is stated.

## What the core is

A small spine that every agent composition shares. Everything is a plugin on a Cordis context, and every observable effect is a session event.

| Package | What it does | ctx key |
|---|---|---|
| `scope` | Layered registration: a child scope inherits its ancestors' contributions, the nearest definition wins, and disposing a scope removes everything it owned. Dependency-free. | none |
| `session` | The append-only event log (about 25 typed events) that all history derives from. Appends validate and snapshot every value once (lossless JSON only). A derived "surface" is the ordered view the model sees; compaction replaces spans of it without deleting them. `request/header` snapshots make every request a pure function of the log. `repair` closes interrupted turns. `fork` seeds a branch. | `ctx.sessions` |
| `system-prompt` | One ordered prompt from sections, dynamic runtime facts, variables and tool schemas. Per-agent contributions override global ones. Unresolved variables fail assembly instead of sending a broken prompt. | `ctx.systemPrompt` |
| `tools` | The registry plus a guarded pipeline: `pre-execute` (allow/deny/ask) then guards, then `execute` wrappers, then finalise. Typed `defineTool` with JSON-schema validation of arguments, cooperative timeouts, parallel-safety flags, ordered batch results and optional session logging. Tool-card presentation intents. | `ctx.tools` |
| `agent-tool-presentation` | Per-preset choice between showing every native tool or only `run_code`. | none |
| `agent` | The public `Agent` handle and registry: follow-up and steering input, a durable two-queue inbox (`nextTurn`, `nextStep`), durable model selection, typed cancel causes, step admission, and `foldConsumedWork`. | `ctx.agents` |
| `agent-default-model` | The deployment default model for fresh agents. | `ctx.agentDefaultModel` |
| `agent-loop` | The default driver: creation and resume in one rollback-covered path, then the turn and step machine, retry through `agent/request-error`, and recovery of unanswered tool calls (`TOOL_NOT_STARTED`, `TOOL_OUTCOME_UNKNOWN`). | `ctx.agentLoop` |
| `tool-executor-native` / `tool-executor-ptc` | Two ways to expose tools: each one as a schema, or only `run_code`, a program calling the tools through a generated SDK (TypeScript and Python types) with a per-program call limit. | none |

Four ideas carry it: event sourcing, a swappable driver behind a public `Agent` contract, scoped contributions per agent, and every failure becoming a visible event rather than an exception.

## Compaction family (read separately, see `docs/threads-worktrees-plan.md` for context)

`compaction` (shared contract), `compaction-basic` (automatic backend), `compaction-tool-result-pruner`, `compaction-image-offload`, `command-compact` (`/compact`). Its notable ideas: cuts that never split a tool call from its result, a cache-reusing summary call (replay the conversation, append the instruction last), one structured checkpoint merged forward, a summary that must shrink, per-model threshold and retention policy, a durable start/summary/end lifecycle, and manual compaction serialised against running turns.

## Side-by-side

"Same" means Vivianne has an equivalent that does the job; "partial" means part of it; "missing" means nothing found.

| dsh capability | Vivianne | Status |
|---|---|---|
| Layered scopes with inheritance | Per-thread `AppState` in `AgentRegistry`, per-profile tool sets (`tools/profile.rs`) | partial, different design (Rust state, not scopes) |
| Append-only typed session log | Session JSON files, plus the new SQLite event spine (`agent/orchestration.rs`) for thread events and a transcript copy (`agent/transcripts.rs`) | partial: threads yes, per-turn events no |
| Request headers (reproducible requests) | `runtime/trace.rs` records runs | partial |
| Closing an interrupted turn | v1 only dropped results without calls (`drop_orphaned_tool_results`); calls without results were not handled | missing, **built**: `runtime/repair.rs` |
| Fork seeds | `cmd_fork_session`, `cmd_fork_session_at`, `cmd_clone_session` | same |
| System prompt from ordered sections | `runtime/directive.rs`, `tools/guides.rs`, capability map | same (not section-for-section) |
| Tool registry with schemas | `tools/schemas.rs`, `tools/builtin.rs`, custom tools, MCP, tool search | same |
| Arguments checked against the schema before running | None found: tools received raw arguments | missing, **built**: `tools/arg_check.rs` |
| allow/deny/ask pipeline | `tools/exec.rs` verdicts, hooks (`runtime/hooks.rs`), bot policy, approvals | same |
| Parallel-safe batches with ordered results | `FAST_PATH_PARALLELISM` for read-only tools in `exec.rs` | same |
| Cooperative per-tool timeouts | `tools/timeouts.rs`: ceilings on the read-only tools, `toolTimeouts` setting | built |
| Tool presentation intents | The frontend tool cards (`ToolCallCard.tsx`) decide this themselves | partial |
| PTC (`run_code` over an SDK) | `tools/ptc.rs`, `run_script`, `run_native` | same |
| Durable inbox (nextTurn / nextStep) | Steering and follow-up queues exist in memory; they don't survive a restart | partial, not built (a separate rebuild is already planned) |
| Durable model selection per agent | `selected-model.json`, per-thread model in `AgentState`, restored from sessions | partial |
| Typed cancel causes | `abort_flag` only, plus `cancel_subagent(reason)` | missing, not built |
| Consumed-work fold | none | missing, not built |
| Default model entry point | `selected-model.json`, `cmd_agent_set_model` | same |
| Retry through a request-error hook | `runtime/recovery.rs`, `attempts.rs`: waits out rate limits, falls back, retries | same |
| Compaction family | `runtime/compaction.rs`, `context_shaping.rs` (tool-result pruning, image eviction), `cmd_compact_now` | partial; **upgraded** to `runtime/compaction_v2.rs` |

## What I built from this

1. **Compaction v2** (`runtime/compaction_v2.rs`): balanced cuts, an eight-section checkpoint prompt that merges an earlier checkpoint forward, a "must shrink" check, a shorter retry before the deterministic fallback. Wired into `maybe_compact_context`. 7 tests.
2. **Argument checking** (`tools/arg_check.rs`): every built-in tool call is checked against its own schema; an invalid call is not run and the model gets one message naming every problem. Reuses the existing denial path in `execute_tool_calls`, so the call still gets a result. Forgiving on numeric text and `"true"`/`"false"`; MCP and custom tools are not checked. 6 tests.
3. **Interrupted-call repair** (`runtime/repair.rs`): before each request, a tool call with no result gets an "outcome unknown" error result next to its siblings, so one cut-off turn can't make every later request fail. 3 tests.

## Not built, and why

- **Durable inbox:** the queue rebuild was already a separate pending task; building a second queue here would collide with it.
- **Per-tool timeouts:** a generic ceiling could kill legitimately long tools (builds, downloads, GPU jobs). It needs a per-tool decision, so it should be designed with you rather than defaulted.
- **Typed cancel causes and consumed-work fold:** small, but they only pay off once the inbox and the event log carry turn events (Phase 2 onward of the T3 plan).
- **Scopes, system-prompt sections, request headers, tool presentation intents:** they exist in Vivianne in a different shape. Copying dsh's shape would be churn without a gain.
- **Compaction extras not taken:** the cache-reusing summary call (Vivianne's one-shot background completion can't replay the conversation prefix), per-model policy tables and the `compaction/*` durable events (they need the turn-level event log).

## Unverified

Compaction v2, the argument check and the repair have unit tests only. None has been exercised in the running app with a real model.
