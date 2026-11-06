# Agent intelligence & inference pipeline survey

Date: 2026-10-29
Scope: everything that decides what the model knows, what it can do, and how fast it answers.
Method: full read of `packages/gui/src-tauri/src/agent/**`, `packages/gui/src-tauri/src/ai/**`,
`packages/gui/src-tauri/src/models/**` and `packages/gui-next/src/modules/ai/**`.
All findings are code-reading only — nothing was modified.

Severity: **H** = actively costs intelligence or seconds on most turns. **M** = costs it in
specific but common situations. **L** = worth fixing when touching the area.

---

## 1. How a turn actually runs

```
Composer.submit
  → agentStore.sendMessage            (gui-next …/ai/store/agentStore.ts:543)
  → cmd_agent_prompt                  (agent/runtime/commands.rs:152)
     · multiplayer::before_run        (git subprocesses, awaited)
     · viv-auto router classify       (network: Laya ≤1500 ms, Jev ≤700 ms, + keychain)
     · run_ledger.begin_run           (blocking std::fs)
     · rebuild the whole system prompt (commands.rs:233-301 → directive.rs)
  → Agent::prompt                     (runtime/agent.rs:79) — reseeds from persisted state
  → run_agent_loop_continue           (runtime/loop_core.rs:492)  × up to MAX_STEPS = 100
       maybe_compact_context          (preflight; may add a full LLM round trip)
       shape_history                  (masks old tool results, evicts images, in place)
       convert_agent_messages_to_ai   (fresh each step)
       byte guard (4 MB)              (drains oldest half of context — not synced to state)
       call_provider_with_fallback → … → provider_stream_request
          → webview dispatcher.ts → AI SDK streamText → ai-stream-token per delta
          → Rust batches to UI at 50 ms / 500 chars
       execute_tool_calls             (tools/exec.rs:740 — read-only parallel, rest serial)
       nudges (doom loop, todo, resume, verification)
```

Context sizing is `chars ÷ 4` (`compaction.rs:26-29`); images count as **0 tokens**
(`compaction.rs:67`). Budget: `reserve = window/4` clamped `[2048, 32768]`,
`keep_recent = (window − reserve)/2` (`loop_core.rs:1403-1414`).

System prompt is rebuilt from disk every turn: `BASE_DIRECTIVE` alone is ~12.9 KB
(`directive.rs:21-124`), plus capability catalogue, AGENTS.md/CLAUDE.md (≤20 KB), master
charter (≤12 KB), core memory (≤2.4 KB), workspace/git state — realistically 20–40 KB, plus
~57 KB of tool schemas for cloud models (`tools/profile.rs:5`).

---

## 2. Highest-value issues (read this section first)

| # | Area | Issue | Where |
|---|------|-------|-------|
| 1 | tools | `ToolResult.is_error` is thrown away; failure is re-guessed from whether the text starts with `"Error"` | `exec.rs:376-388, 715-717, 946, 1052` |
| 2 | loop | Thinking signatures are destroyed at every boundary, so reasoning is always replayed as plain assistant **text** | `runtime/types.rs:46-52`, `loop_core.rs:2856-2901, 2944-2948` |
| 3 | loop | Compaction counts only messages — the 20–40 KB prompt and 57 KB of tool schemas are invisible to the budget | `compaction.rs:87-116`, `loop_core.rs:1434-1436` |
| 4 | loop | The 4 MB byte guard drains history *after* the orphan filter, so it can orphan a tool result and then send it (OpenAI rejects the request) | `loop_core.rs:733` vs `789-806` |
| 5 | pipeline | `timeout_ms` is defined in every options struct and consumed by nothing; no `reqwest::Client` has any timeout or pooling | `ai/types.rs:69`, `simple_options.rs:44`, 9× `Client::new()` |
| 6 | pipeline | Azure OpenAI and Bedrock **buffer the whole response** before the first token | `providers/azureOpenai.ts:170`, `providers/bedrock.ts:127-134` |
| 7 | memory | The rerank cadence gate is dead code on its only call site → every `recall` costs an extra LLM round trip | `exec.rs:2926-2934`, `memory/mod.rs:781-784` |
| 8 | loop | Subagent and team reports are returned **unbounded**, and `task_output` is exempt from masking | `exec.rs:1994-2000`, `runtime/subagents.rs:207-209`, `context_shaping.rs:31-34` |
| 9 | tools | `write`/`edit`/`grep`/`ls` — the four most-used tools — have the weakest descriptions and hide their caps | `schemas.rs:144,156,181,194` |
| 10 | loop | Retry (4) × pool rotation × fallback models multiply with no turn deadline | `loop_core.rs:1576-1602, 1776, 1855` |

---

## 3. The agent loop — what hurts intelligence

### H1 — Tool failures are reported to the model as successes

`tool_result_to_agent_result` (`exec.rs:376-388`) keeps only `content` and `details`; the
batch's `is_error` is recomputed by

```rust
fn looks_like_error(r: &AgentToolResult) -> bool {
    r.content.iter().any(|c| matches!(c, MessageContent::Text { text }
        if text.starts_with("Error") || text.starts_with("FileNotFound")))
}
```

(`exec.rs:715-717`) — verified in the source. Consequences:

- a `bash`/`powershell` run with a non-zero exit starts with `[stdout]`, so it is recorded as
  a **success** (`streaming.rs:231-235` sets the real flag; Anthropic/Google/Bedrock would
  have used it — `anthropic.rs:321`, `google_shared.rs:249-251`, `bedrock.rs:332`);
- `edit`'s "old_string not found" / "not unique" (`builtin.rs:411,419`) is a success;
- custom-tool timeouts "Timed out after Ns…" (`custom_tools.rs:195,222,226`) are a success;
- `Tool not found: x` (`exec.rs:1034-1047`) is a success, so the attempt-record guard that
  exists to stop repeated unknown-tool guesses never fires (`attempts.rs:96-114`).

This is the classic "tool says fine, build is red" loop. **Fix:** propagate `is_error` from
`ToolResult` into `AgentToolResult` and delete the heuristic.

### H2 — Reasoning is never replayed as reasoning

Agent-side `MessageContent::Thinking` has no signature field (`runtime/types.rs:49`), the
provider signature is dropped on ingest (`loop_core.rs:2944-2948`), and re-conversion
hardcodes `thinking_signature: None` (`loop_core.rs:2895-2901`). Converted messages also carry
`provider/api/model_id = None` (`loop_core.rs:2856-2858`), so
`transformMessages.ts:73-81` always takes the "convert thinking to text" branch — for *every*
replay, even back to the same model. Redacted thinking is dropped entirely
(`transformMessages.ts:77`).

Three costs: Anthropic's signed-thinking replay never works; every historical CoT becomes
permanent visible assistant text (and is counted by compaction, `compaction.rs:66`); and after
a model fallback the new model reads the old model's raw reasoning as if it were the answer.
Gemini's tool-call signature *is* preserved (`types.rs:27-31`) — the pattern exists, it was
just never applied to thinking.

### H3 — Compaction is blind to the fixed part of the prompt

`calculate_context_tokens*` walks **messages only** (`compaction.rs:87-116`) and is compared
against `window − reserve` (`loop_core.rs:1434-1436`). The 20–40 KB system prompt and ~57 KB
of tool schemas are excluded, so the loop believes it has more room than it does. Recovery is
a single reactive pass per run (`overflow_compacted`, `loop_core.rs:860-869`); a second
overflow ends the turn with a raw provider error (`loop_core.rs:948-969`). On an 8k-window
model the fixed overhead can *be* the whole window.

Two related holes: compaction is impossible below `min_messages = 10` even on overflow
(`compaction.rs:121-124`), and images are counted as zero tokens (`compaction.rs:67`) so
screenshot-heavy desktop/browser sessions only find out from a hard provider error.

### H4 — The byte guard can orphan tool results, then sends them anyway

`drop_orphaned_tool_results` runs at `loop_core.rs:733`; the 4 MB guard drains the oldest
half at `loop_core.rs:789-806` and re-converts **without** re-running the filter. If the drain
boundary splits an assistant-tool-call / tool-result pair, the request carries a result with
no matching call — the exact payload OpenAI rejects outright (the function's own doc comment
at `loop_core.rs:2793-2810` says so). The guard also never syncs state, so the pruned half
returns on the next turn (`agent.rs:90-94`) and pays the cost again.

### H5 — Subagent and team output is unbounded

- `task` / `task_output` return the subagent's final text verbatim with **no byte cap**
  (`exec.rs:1994-2000`, `subagents.rs:207-209`), while every other tool obeys a budget.
- `task_output` is in `NEVER_MASK` (`context_shaping.rs:31-34`), so a polled subagent report
  survives until full compaction and repeated polls duplicate it.
- `team_run` is quadratic: each member's brief embeds **all** upstream reports verbatim
  (`run.rs:208-212, 233-244`) and the coordinator's report concatenates every member's full
  output, up to 2 fix rounds, as one tool result (`run.rs:552-618`).
- Bot fan-out dispatches a full agent run per teammate per hop, up to 4 hops, each attaching
  up to a 45 KB room digest (`bots.rs:686-687, 773-786, 1056-1066`).

### M — loop

| Issue | Where |
|-------|-------|
| Compaction syncs ephemeral `<system-reminder>` messages into the persisted transcript, so stale nudges ("you are on the final step…") get re-seeded on every future turn | `loop_core.rs:56-69` (contract) vs `1471` (violation) |
| Masked tool outputs are destroyed permanently once compaction syncs state — the model is told "run it again if you need it" for data nobody can recover | `context_shaping.rs:9-12, 134-149` vs `loop_core.rs:1449-1471` |
| "Done means checked" clears on **any** successful shell call, so `git status` satisfies three pending file-change checks | `verification.rs:55`, `46-51` |
| Anthropic thinking budget (8192/16384) can exceed `max_tokens` (placeholder model uses 8192) → every thinking request 400s | `loop_core.rs:2011-2030`, `agent.rs:44-53` |
| In-place masking mutates history but the token cache only invalidates on shrink → compaction fires early | `compaction.rs:106-116`, `types.rs:366-377` |

### L — loop

`looks_like_error` misses "Tool not found" (`exec.rs:715-717`); 120 s stream idle timeout
kills long silent reasoning and retries from scratch (`loop_core.rs:2425, 2468-2473`);
doom-loop stop at 5 identical consecutive signatures (`loop_core.rs:518-519, 1137-1155`);
announced-action heuristic can force an extra turn on a forward-looking final sentence
(`loop_core.rs:118-144, 1228-1233`); the `<vivianne_browser>` block sits **mid**-prompt and
changes when a browser connects, invalidating the prompt-cache prefix for everything after it
(`directive.rs:183-190` vs the deliberate append-last order at `directive.rs:281-292`).

---

## 4. The tool layer

### Inventory (56 static schemas in `schemas.rs:128-935`, plus dynamic)

Files: `read write edit ls apply_patch notebook_edit todo_write` ·
Search: `grep glob code_search` ·
Shell: `bash powershell terminal sandbox_execute run_script` ·
Documents: `create_document inspect_document edit_document` ·
Git/code: `repo_clone repo_overview github github_act github_api diagnostics` ·
Web: `web_fetch web_search sleep` · Compute: `math generate_image get_current_time` ·
Delegation: `task task_output kill_task hire_agent list_subagent_profiles team_run team_post
team_read message_agent submit_plan assign_plan_steps` · Desktop: `dip`/`computer`, `set_panel` ·
Browser: `user_browser_driver` + 12 legacy `browser_*` aliases ·
Memory/meta: `remember recall read_skill guide check_diagram list_tools load_tools question
present_snippet` + goal/automation/MCP tools · Extensibility: `create_tool delete_tool
list_custom_tools`, `mcp__*`, mod tools.

Good: `bash` documents its 32 KB head/tail cap and spill file honestly (`schemas.rs:169` ↔
`streaming.rs:217-229`); `math` and `dip` document theirs too. Truncation is signalled for
`read` (2000 lines / 50 KB), `grep` (100 matches), `glob` (100), `ls` (1500), `web_fetch`
(100 KB), `math` (30 k chars), `github*` (16 k).

### H1 — Descriptions of the most-used tools are the weakest

`write` says "Write content to a file, creating it if it doesn't exist" (`schemas.rs:144`) —
never that it **overwrites wholesale**, that a checkpoint is taken (`exec.rs:869-895`), or that
`edit` is the surgical tool. `edit` (`schemas.rs:156`) omits the exact-match and uniqueness
rules it actually enforces (`builtin.rs:411,419`) — the model learns the rule from a failed
turn. `grep` (`schemas.rs:194`) is silent on the 100-match / 4000-file caps and that a capped
scan still reports success (`builtin.rs:565-583`). `ls` hides its 1500-entry cap
(`schemas.rs:181` vs `builtin.rs:2049`).

### H2 — `details` is invisible to the model but counted by compaction

`details` is dropped when the transcript becomes a provider request (`loop_core.rs:2910-2916`)
yet still counted by the size heuristic (`compaction.rs:69,77`). Anything communicated *only*
through it is reported to nobody: grep's `truncated`/`scan_capped`, read's `truncated`,
`inspect_document`'s full part texts (model gets 120-char previews, `documents.rs:2253-2264`),
`sandbox_execute`'s `exitCode` (`exec.rs:2685-2693`), `web_fetch`'s HTTP status.

### H3 — `sandbox_execute` has no output cap and hides its exit code

Full stdout+stderr into context (`exec.rs:2670-2693`); the exit code lives only in `details`.
A `pip install` log floods the window and the model cannot tell pass from fail. `task`,
`task_output` and `run_script` are likewise uncapped (`ptc.rs:291-321`).

### H4 — Tools that are dispatched but have no schema

`set_panel` (`exec.rs:482`) and the 12 legacy `browser_*` names (`exec.rs:457-468`) are
dispatchable with no JSON schema anywhere. The model can never discover them, and a model that
*happens* to guess one gets a working tool — rewarding hallucinated names. The guardrail test
at `exec.rs:3191` only covers `create_coding_tools()`, not `AGENT_TOOLS`.

### M — tools

Non-read-only calls in a batch run **sequentially** (`exec.rs:968-1054`) — five independent
`web_fetch`/`github`/MCP calls are serialized — and a pending approval blocks the batch inline
for up to 900 s (`exec.rs:701-712`). Read-only calls run unbounded in parallel with no
semaphore (`exec.rs:924-943`). Keyless `web_search` holds a mutex **across** a 1.5 s sleep, so
concurrent searches serialize at ≥1.5 s gaps with a 300 s cooldown (`builtin.rs:1067-1084`).
Grep's capped scan reports success (`builtin.rs:565-583`); `inspect_document` gives 120
chars/part (`documents.rs:2256`) and PDF pages 400 chars (`documents.rs:2161`), forcing an
extra round trip per real edit.

---

## 5. Inference pipeline — why it can be slow

### Two paths

- **GUI (default):** Rust never does provider HTTP. It emits `provider_stream_request`, the
  webview runs the Vercel AI SDK, and tokens come back one IPC event per delta
  (`sdkShared.ts:156,164`).
- **CLI/headless (`app run`) and `local` provider:** Rust does the HTTP itself with a bare
  `reqwest::Client`.

### H1 — Azure and Bedrock buffer the entire response

`azureOpenai.ts:170` (`await resp.text()`) and `bedrock.ts:127-134`
(`await resp.arrayBuffer()`) read the whole body before emitting anything; the Rust twin does
the same (`azure_openai.rs:370`). First-token latency equals full generation time. Everything
else streams per token. **Fix:** `resp.body.getReader()` with incremental line/frame parsing —
`openai_completions.rs:544-593` already has the correct pattern.

### H2 — `timeout_ms` is dead plumbing; no client is pooled or timed out

Present in `ai/types.rs:69` and every options struct (`anthropic.rs:68`, `google.rs:49`,
`openai_completions.rs:43`, `openai_responses.rs:43`…) and consumed nowhere — the only
reference is the pass-through at `simple_options.rs:44` (verified). Every provider client is
`Client::new()` (`completion.rs:74`, `anthropic.rs:465`, `openai_completions.rs:446`,
`openai_responses.rs:344`, `google.rs:300`, `azure_openai.rs:342`, `bedrock.rs:572`,
`embedding.rs:285,354`, `jev.rs:99`, `chatgpt.rs:429`, …): no connect timeout, no pooling, a
fresh TCP+TLS handshake on the CLI path every attempt. The only bounds are the TS watchdogs
(60 s connect / 90 s idle, `shared.ts:15-17`) and the 120 s Rust guard (`loop_core.rs:2425`).
The pattern for a pooled client already exists at `ocr.rs:198` and `semantic_index.rs:259`.

### H3 — Retries multiply with no deadline

4 attempts × ≤16 s exponential backoff (`loop_core.rs:1576-1602`) × **every** pool candidate
(`1776`) × **every** fallback model (`1855`), each attempt re-reading the keychain
(`keychain_store.rs:82-85`, `auth.rs:1031-1034`), then the recovery layer on top: 30 s →
15 min backoff, interactive waits up to 5 min ×4, long tasks up to 6 h
(`recovery.rs:107-119`). On top of that the retry classifier matches `timeout`, `500`,
`terminated`, `server error`, `internal error` **anywhere in the message string** (`utils.rs:411-450`),
excluding only quota/billing shapes — so model text echoed into an error message can trigger
futile 1–16 s retries.

### H4 — Work that happens before the first token

Ranked:

1. **Router classify** when the model is `viv-auto`: Laya ≤1500 ms (`difficulty.rs:202`) plus
   Jev ≤700 ms (`jev.rs:39`) plus per-tier keychain `provider_has_key`
   (`routing.rs:178-181`) — up to ~2.2 s before anything else (`commands.rs:342-358`).
2. **Preflight compaction** adds a full extra LLM round trip when over budget
   (`loop_core.rs:1454-1460`).
3. The **webview hop**, a dynamic `import()` of the provider module on the first request of a
   session (`dispatcher.ts:51-53`), and a full-history `transformMessages` rebuild per request
   (`universalProvider.ts:103-236`).
4. **Keychain round-trips** — `resolve_api_key` *and* `CredentialPool::candidates` per attempt
   (`loop_core.rs:1542-1572, 1748-1755`).
5. **A disk write on the response path:** `pool.mark_result` awaited before returning
   (`loop_core.rs:1769`) re-serializes the whole runtime file with `to_string_pretty`
   (`auth.rs:1189-1196, 1212-1220`).
6. **Blocking `std::fs`** in `run_ledger.begin_run` before the turn (`run_ledger.rs:164`),
   plus `chatgpt.rs:66-83`, `model_discovery.rs:407,415`.
7. Every token payload is **JSON-parsed twice** (`loop_core.rs:1946` and `1964`), and `listen_any`
   makes N concurrent streams each parse every event.
8. First visible token waits for the 50 ms batch tick (`loop_core.rs:2418-2419`) even after
   `MessageStart`.

### H5 — Prompt caching is computed, then dropped on most paths

- **Anthropic GUI:** correct — system `cache_control`, rolling last-message breakpoint, last
  tool breakpoint (`universalProvider.ts:117-134`, `sdkShared.ts:87`).
- **Anthropic native/CLI:** system + last tool only, **no** rolling breakpoint
  (`anthropic.rs:431-437, 359`) → full re-prefill every turn.
- **OpenAI Responses:** `promptCacheKey` is applied in the webview (`universalProvider.ts:218`).
- **OpenAI Chat Completions / Mistral / every OpenAI-compatible host:** Rust computes
  `viv-<sha256>` (`loop_core.rs:1994-1998`) but TS sends `providerOptions: {}`
  (`universalProvider.ts:193`), and the native path builds options without `session_id`
  (`loop_core.rs:2074-2079, 2125-2130`) — so the key never reaches the API on either leg.
- Invalidation comes from `shape_history` in-place masking, image eviction,
  `drop_orphaned_tool_results` and compaction rewriting history from a point onward
  (`context_shaping.rs:20-28,131`, `loop_core.rs:709, 733, 1447-1474`).

### M — pipeline

O(n²) SSE buffering — `buffer = buffer[x..].to_string()` per line in `anthropic.rs:534-535`,
`google.rs:361-362`, `openai_responses.rs:400`. Silent token loss — `tx.try_send` drops on a
full channel (`loop_core.rs:1965`) and `final_tx_holder.try_lock()` drops the terminal event
(`1955`) → "No completion message received from provider". No mid-stream fallback: a failure
after tokens start restarts the whole attempt (`loop_core.rs:857-858`). Azure and Bedrock have
no `native()` arm, so in headless CLI mode their request has no listener and stalls the full
120 s (`loop_core.rs:2217-2351`). `read_settings_json` deep-clones settings on every call
(`exec.rs:2981`). L1 memory extraction uses the **chat** model rather than the cheap tier the
routing module reserves for outside-turn work (`mod.rs:944`, `evolve.rs:176` vs `routing.rs:136-141`).

---

## 6. Memory & teams

Memory is in good shape on the hot path: the per-turn cost is one snapshot plus two SQLite
queries (`evolve.rs:360-371`); turn-close capture and L1 extraction are fire-and-forget
(`loop_core.rs:1354-1355`). Retrieval is FTS + sqlite-vec KNN, RRF-fused, confidence-gated,
max 8 hits (`memory/mod.rs:709-812`), injected only as `<core_memory>` ≤2400 B plus the
`recall` tool.

Issues:

- **The rerank cadence gate is unreachable.** The only caller passes `turn_count: 0,
  query_embedding: None` (`exec.rs:2926-2934`), which forces `should_rerank = true`
  (`mod.rs:781-784`); the topic-stability gate in `rerank.rs:34-87` never runs. Every `recall`
  with a sidecar model configured pays an embedding **plus** a full LLM call, inside the turn.
- **`core_block` drops the newest instructions first** — sorted oldest-first then truncated at
  the end (`evolve.rs:366, 388-395`). Once it overflows, the most recent standing directives
  are the ones silently cut from every prompt.
- Recall write amplification — up to 8 `touch_entry` writes per call plus a full edge scan
  (`mod.rs:826-874`), which grows with store size. `remember` blocks the turn on a synchronous
  dedup embed (`mod.rs:547-555`).
- No embedding key and no local model → BM25-only (`mod.rs:623-628`), i.e. paraphrase misses.
  The gate's ~15% false-negative rate is acknowledged in-source (`mod.rs:51-55`).
- Dead legacy 8 KB `<project_memory>` machinery (`mod.rs:23, 486-533`) still gets string-split
  out of the stored prompt every turn (`commands.rs:243`).
- **Automations bypass routing entirely** — `workflowRunner.ts:49-99` creates a thread and
  sends each prompt as an ordinary chat turn on whatever model is currently selected, at full
  prompt cost.

---

## 7. Frontend

Solid: rAF-coalesced streaming flush (`agentStore.ts:198-221`), 50 ms/500-char backend batch,
`virtua` virtualization, memoized bubbles, autoscroll pause-on-manual-scroll, and
`transformMessages` as a careful port of the Rust logic.

Issues:

- **Attachments on a queued/steering prompt are silently dropped** — the busy branch never
  collects images or files (`Composer.tsx:340-344`, `agentStore.ts:527-533`); `QueuePrompt.images`
  exists (`lib/types.ts:215-221`) and is never populated.
- **`sendMessage` force-clears `isBusy`** when the prompt IPC resolves (`agentStore.ts:556-559`),
  racing `agent_start` — a second prompt in that window takes the non-busy path concurrently
  with a running turn.
- **Revert races the live stream** — `cmd_agent_revert` is fired without awaiting
  (`MessageList.tsx:249`) while in-flight `message_update`/`message_end` events re-append a
  stale streaming message (`agentStore.ts:325-337`).
- **The memory-recall cap is per store instance, not per turn** — the counter is an event-local
  `let` (`agentStore.ts:183-184`) while `agent_end` resets a *different* field (`agentStore.ts:130,494-495`),
  so recall can be permanently suppressed for a thread.
- Tool-result **images** are replaced by a text placeholder on cross-model resume
  (`sdkShared.ts:54-55`) — screenshots from `dip`/browser are invisible to any other model.
- First-launch default is a hardcoded `gpt-4` placeholder with no key
  (`agent.rs:44-53`, `modelStore.ts:51`); `cmd_get_default_model` exists and is never called.
- Streaming `flushTokens` rebuilds the message as exactly `[thinking, text]`, dropping any
  other block type (`agentStore.ts:204-221`).
- Two competing throttle mechanisms — the rAF flush plus a 100 ms `useThrottledValue` in
  `Markdown.tsx:48` and `ReasoningBlock.tsx:17`.

---

## 8. Did recent changes interfere with the pipeline?

Nothing in the recent commits rewrote the request path; the pipeline was already shaped this
way. What the recent work *did* change, and what to watch:

- `030dd23` "Agent loop: an attempt record, kept errors, a run trace" — added `attempts.rs` and
  the run trace. Cost is bounded (240 chars × 3 per key, `attempts.rs:18-22`), but it is where
  the `looks_like_error` heuristic is consumed, so it is currently recording *successes* for
  failures (§3 H1).
- `faf0f49` "All browsing goes through the extension-connected browser, not Camoufox" — moved
  browsing to `user_browser_driver`; the 12 legacy `browser_*` names were left dispatchable
  without schemas (§4 H4).
- `3d0b45c` "retire Git review, Background tasks and Memory panels" — the panels are gone, but
  the memory machinery still runs on the turn path (§6).
- The panel rebuild and its removal touched only `gui-next` UI; no Rust inference file was
  modified by it.

**The slowness is structural, not a regression:** no HTTP timeouts or pooling (§5 H2), the
router classify before every turn (§5 H4.1), keychain reads per attempt (§5 H4.4), and — if you
are on Azure or Bedrock — fully buffered responses (§5 H1).

---

## 9. Suggested order of work

**Quick wins, low risk**
1. Propagate `is_error` from `ToolResult`; delete `looks_like_error` (`exec.rs:376-388,715-717`).
2. Add a pooled `reqwest::Client` with `connect_timeout(10s)` + the already-plumbed `timeout_ms`.
3. Send `prompt_cache_key` on the OpenAI-compatible chat path (`universalProvider.ts:193`,
   `loop_core.rs:2074-2079`) and add the rolling Anthropic breakpoint to the native path.
4. Fix `core_block` ordering: newest-first, truncate the tail (`evolve.rs:388-395`).
5. Fix the rerank gate: pass the real `turn_count`/`query_embedding` from `exec.rs:2926-2934`.
6. Improve the `write`/`edit`/`grep`/`ls` descriptions and state their caps.

**Bigger, still contained**
7. Stream Azure and Bedrock instead of buffering.
8. One overall deadline for the provider phase; rotate keys on the first retryable failure
   instead of after 4.
9. Include system prompt + tool schemas in the token estimate; drop the `min_messages = 10`
   floor on forced compaction; give images a non-zero estimate.
10. Re-run the orphan filter after the byte guard, and sync the drain to state.
11. Cap `task`/`task_output`/`run_script`/`sandbox_execute` output and put `task_output` back
    under masking.

**Structural**
12. Preserve thinking signatures end to end (`runtime/types.rs:49` → `loop_core.rs:2895-2948`).
13. Stop rewriting history in place; prefer append-only elision notes so prefix caches survive.
14. Populate `QueuePrompt.images`, fix the `isBusy` race and the revert race in `agentStore`.
