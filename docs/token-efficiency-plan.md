# Vivianne token-efficiency and task-completion plan

Status: research and plan only, no code changed. Written 2026-10-04.

## 1. Current token budget (measured from source; bytes/4 ≈ tokens)

| Piece | Where | Size | Notes |
|---|---|---|---|
| Tool schemas, cloud (~75 tools) | `agent/tool_schemas.rs` | ~57 KB ≈ **14–15k** | Figure from `tool_profile.rs` docs. Sent with every cloud request, plus any MCP or playbook tools. |
| Tool schemas, local lean profile | `tool_profile::CORE_TOOLS` | ~12 KB ≈ 3k | Local models only (`is_lean_provider == "local"`). |
| `BASE_DIRECTIVE` | `agent/directive.rs` | 9.6 KB ≈ **2.4k** | About 0.5k of it is chart/model3d/mermaid guidance. |
| Capability map, full | `capabilities::capability_section` | ~10–13 KB ≈ **2.5–3.3k** | Largely repeats the tool descriptions. The lean map is about a third of that. |
| Project instructions | `project_instructions.rs` | ≤20 KB ≈ ≤5k | Size depends on the repo. |
| Master charter | `master.rs` | ≤12 KB ≈ ≤3k | Primary agent only. |
| Core memory | `memory_evolve::core_block` | ≤2.4 KB ≈ ≤0.6k | Recall is on demand, which is good. |
| **Fixed prefix per request (cloud)** | | **≈20–28k** | Paid on every step of every run, and again by every subagent. |
| `read` result | `tools_enh.rs` | ≤2000 lines / 50 KB ≈ ≤12.5k | |
| `bash`/`powershell` result | `streaming.rs MAX_STREAM_BYTES` | ≤**256 KB ≈ 64k** | Claude Code caps this at about 30k chars. |
| Terminal read / github / custom / math | various | 256 KB / 40 KB / 60 KB / 30 KB | |
| Ageing | `context_shaping::prune_aging_tool_results` | Results older than the last **20 messages** that exceed 4 KB are cut to 800+800 bytes | Never masked fully. |
| Compaction | `loop_core::maybe_compact_context` | Fires at about window − reserve (200k model: about 168k of messages) | On 1M-window models it almost never fires. |

### How requests are cached today (`gui-next/.../universalProvider.ts`, `sdkShared.ts`)

- **Anthropic.** There are cache breakpoints on the last tool and on the system message. **The conversation itself has no breakpoint**, so every step pays full price for the whole history. Anthropic only looks back from an explicit breakpoint ([prompt caching docs](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)). This is the largest waste in the app.
- **OpenAI Responses, Google, OpenAI-compatible.** These rely on automatic prefix caching. No `promptCacheKey` is set; only the Rust Azure path sets `prompt_cache_key`.
- **Two things break the prefix every step:**
  1. `prune_aging_tool_results` rewrites the message that crosses the 20-message boundary on every step, and `evict_old_images` does the same for images. Any prefix cache, including Anthropic's once breakpoints are added, therefore misses from roughly 20 messages back on every step.
  2. In `assemble_capability_blocks`, the goal block and `@mentioned` skills sit in the middle of the system prompt. Any change to them invalidates everything after them.
- **Thinking.** Anthropic uses `thinking: enabled, budgetTokens` mapped from the level (1k/2k/8k/16k), not `effort`. OpenAI `reasoningEffort` is fixed at `'medium'`, whatever the user's level.

### Completion reliability today

- `BASE_DIRECTIVE` has strong "verify before claiming" rules.
- Goal and ledger runs get `CONTINUE_PROMPT` nudges.
- Ordinary (non-goal) runs end whenever the model stops calling tools. Nothing checks for unverified side effects or open todos.
- The Windows `dip type` result already asks the model to check with `read_text`, but nothing enforces it. That gap is how the typing task got reported done without being checked.

## 2. Prioritized plan

Order is (savings × confidence) / effort. Item 1 is only fully effective once item 2 is done, so ship them together.

### Quick wins (about a day each)

**1. A rolling cache breakpoint on the conversation (Anthropic), and a cache key for OpenAI**
- **Change:** in `universalProvider.ts` `anthropicMessages`, add `providerOptions.anthropic.cacheControl: {type:'ephemeral'}` to the last message's last content part. Keep the tool and system breakpoints, for 3 of the 4 allowed. Anthropic checks back 20 blocks from a breakpoint, and runs of `tool_use`/`tool_result` count as one block, so the previous step's entry is still found. For `openAiResponses`, pass `promptCacheKey: sessionId`, which helps routing on older models. Check whether the Bedrock path (`bedrock.ts`) also needs a message `cachePoint`.
- **Savings estimate:** take a 30-step task where history grows about 3k tokens per step to about 90k. Uncached, that is Σ ≈ 1.4M input tokens at 1×. Cached, it is about 90k written at 1.25× plus about 1.3M read at 0.1× (0.05× on Opus 5.5), roughly 0.25M-equivalent. That is **about 75–85% off history input cost** on Anthropic, plus lower time to first token.
- **Risk:** none to intelligence. Cache writes cost 1.25×, which pays back on the second read.
- **Measure:** per-step `cache_read / (cache_read + cache_write + input)`, already in `usage`. Target ≥0.8 after step 3.
- **Sources:** [Anthropic prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching), [OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching).

**2. Make history edits cache-friendly (hysteresis)**
- **Change:** in `context_shaping.rs`, stop trimming one message per step. Trim only when the untrimmed mass outside the keep window is at least X tokens (for example 20k), and then trim everything eligible at once. This is the same idea as Anthropic's `clear_at_least`. Do the same in `evict_old_images`: evict in batches, not one per step. Leave `prune_aging_tool_results` idempotent.
- **Savings:** today every step re-writes about the last 20 messages (often 20–60k tokens) at 1.25×, or re-prefills them on local or OpenAI. After the change that happens about once every N steps. Expect a cache hit ratio of 0.9 or better, versus perhaps 0.5–0.7 with item 1 alone. This is an estimate; measure it.
- **Risk:** large results stay in context a few steps longer, so the bound is slightly looser.
- **Measure:** the cache-stability unit test described in section 3.
- **Sources:** [context editing docs](https://platform.claude.com/docs/en/build-with-claude/context-editing) (`clear_at_least` exists for exactly this trade-off); [Token Reduction Is Not Cost Reduction, arXiv 2607.12161](https://arxiv.org/abs/2607.12161). That paper found that compressing tool output by 38% *raised* billed cost by 6.8% because cache creation and reads dominate.

**3. Put volatile prompt blocks last**
- **Change:** in `directive.rs` `assemble_capability_blocks`, reorder to: directive → mode → capabilities → workspace → project instructions → master charter → core memory → goal → mentioned skills. Better still, send the goal and mentioned skills as a `<system-reminder>` next to the user message, the way `intent_hint` already works.
- Add a test that renders the prompt twice, changing only the goal or the mentions, and asserts that the common prefix covers all the stable blocks.
- **Savings:** avoids about 5–10k system tokens being re-written on each goal or mention change. Small, but free.
- **Risk:** none.
- **Sources:** [OpenAI caching: stable content first](https://developers.openai.com/api/docs/guides/prompt-caching); [Gemini implicit caching](https://ai.google.dev/gemini-api/docs/caching).

**4. Cap tool outputs at the source**
- **Change:**
  - `streaming.rs`: cut the final `MAX_STREAM_BYTES` from 256 KB to about 32 KB, split about 8 KB head and 24 KB tail, and keep the existing spill-to-disk path so the model can grep the spill file.
  - `tool_exec.rs`: cut `TERMINAL_READ_BUDGET_BYTES` from 256 KB to 32 KB.
  - `github.rs`: cut the default `max_chars` from 40k to 16k.
  - Every truncation notice must state the spill path and say how to see more.
- **Savings:** a noisy build or test log drops from up to 64k tokens to 8k. Until ageing kicks in, it rides along for 20 messages, so a single log can cost hundreds of thousands of input tokens across a run.
- **Risk:** the error that matters can sit in the cut middle. Guard: tail weighting, the spill file, and grep of the spill.
- **Measure:** the re-read rate (calls that grep or read a spill file) and task success on the build and test tasks.
- **Sources:** [Effective context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) (just-in-time retrieval).

**5. A deterministic "done means verified" gate**
- **Change, in `loop_core.rs` (end-of-run branch where `next.is_empty()`) and `tool_exec.rs`:**
  - (a) Track mutating calls this run: `write`, `edit`, `apply_patch`, `dip` `type`/`key`/`click`, `browser_type`/`browser_click`, `remember`, and so on. Also track whether an observing call came after each one: `read`, `read_text`, a screenshot, `diagnostics`, a test or build command, or `browser_snapshot`. If the run ends with unverified mutations, inject **one** `system_reminder` that names them, for example: *"You typed into Notepad but never checked it landed. Verify it, or say plainly that you didn't."* Then continue. Cap this at one per run, as `MAX_TRUNCATED_RESUMES` already caps resumes.
  - (b) Apply the same rule to open `todo_write` items in non-goal runs. `latest_todos` already exists.
  - (c) Best of all, verify inside the tool. After `dip` `type` (`coding_agent/computer.rs`), read the focused control back through UI Automation and return `Typed 42 chars; field now contains "…" (matches)` or `(MISMATCH)`. That costs zero extra model steps.
- **Cost:** about 100–200 tokens when the gate fires, nothing otherwise.
- **Risk:** extra steps on read-only Q&A. Guard: the gate fires only when a mutation happened.
- **Measure:** the false-success rate defined in section 3.
- **Sources:**
  - [False success in LLM agents, arXiv 2606.09863](https://arxiv.org/abs/2606.09863): 45–76% of failures were confident false "done" claims. LLM judges reached only AUROC ≤0.65, while cheap lexical detectors reached 0.83–0.95. Use deterministic checks, not an LLM judge.
  - [Progress mirage, arXiv 2607.25152](https://arxiv.org/abs/2607.25152): the agent claimed improvement in all 54 cycles, but 56% showed no real progress. External, world-state verification fixes this.

**6. Use effort instead of fixed thinking budgets**
- **Change, in `universalProvider.ts` and the `ThinkingLevel` mapping in `loop_core.rs`:**
  - For Anthropic models that support it, send `effort` (the SDK 4.0.39 types expose it) with adaptive thinking, not `budgetTokens`. Map Minimal/Low/Medium/High/XHigh to low/medium/high/xhigh.
  - For OpenAI, map the level to `reasoningEffort` instead of hard-coding `'medium'`.
  - Default `task` subagents to `low`/`medium`.
  - Keep effort constant within a conversation, because changing it top-level breaks the cache. Use per-message effort only where the model supports it.
- **Savings:** output and thinking tokens. Lower effort also means "fewer and terser tool calls". The size is uncertain, so it needs a sweep.
- **Risk:** shallower reasoning on hard tasks. Guard: keep the main-loop default, and lower only subagents and summaries until the evals hold.
- **Sources:**
  - [Anthropic effort docs](https://platform.claude.com/docs/en/build-with-claude/effort).
  - [Overthinking in agentic tasks, arXiv 2502.08235](https://arxiv.org/abs/2502.08235): picking low-overthinking trajectories gave about 30% better results at 43% lower cost.
  - [Adaptive reasoning in agentic AI, arXiv 2608.26442](https://arxiv.org/abs/2608.26442) (read only via the search summary).

### Larger changes (several days each)

**7. Observation masking instead of head/tail ageing**
- **Change:** in `context_shaping.rs`, keep the last N tool results in full (N about 8–10 tool uses, not 20 messages). Replace older results with a one-line placeholder such as `[read src/x.rs: 412 lines, elided — re-run to view]`. Keep the tool inputs, apply the change in batches (item 2), and never mask `todo_write`, goal, plan or `recall` results.
- On Anthropic, an optional alternative is server-side `clear_tool_uses_20250919` with `clear_at_least`. It keeps client history intact, and the SDK exposes `contextManagement`.
- **Savings:** "The Complexity Trap" found masking **halved cost** with the same solve rate as LLM summarization on SWE-bench Verified. AgentDiet cut input tokens 40–60% at −1 to +2% performance. ACON cut peak tokens 26–54%.
- **Risk:** the agent re-reads files it needs. Guard: placeholders say how to get the content back. Measure the re-read rate and success.
- **Sources:** [arXiv 2508.21433](https://arxiv.org/abs/2508.21433), [AgentDiet arXiv 2509.23586](https://arxiv.org/abs/2509.23586), [ACON arXiv 2510.00615](https://arxiv.org/abs/2510.00615), [context editing](https://platform.claude.com/docs/en/build-with-claude/context-editing).

**8. Deferred tool schemas for cloud models**
- **Change:**
  - Anthropic: send every schema, mark the non-`CORE_TOOLS` groups `defer_loading: true`, and add `tool_search_tool_bm25`, or a custom search that returns `tool_reference` blocks and reuses `GROUPS`. Deferred tools stay out of the cached prefix.
  - Other cloud providers: offer the existing lean profile plus `load_tools` behind a setting. The tool set only grows, so cache churn stays low.
  - Use the lean capability map in both cases.
- **Savings:** about 10–12k tokens of context per request and per subagent. With caching working, the *dollar* saving is modest (those tokens already read at 0.1×). The real wins are first-turn cost and latency, context headroom, and accuracy: Anthropic reports that selection degrades beyond 30–50 tools, and that tool search raised Opus 4 from 49% to 74% on MCP evals.
- **Risk:** the model fails to find a tool. Guard: the capability map still names every group, and `intent_hint` can pre-load the matched group.
- **Sources:** [tool search docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool), [Less is More arXiv 2411.15399](https://arxiv.org/abs/2411.15399), [DTDR arXiv 2512.17052](https://arxiv.org/abs/2512.17052).

**9. Send side calls to a cheap model**
- **Change:** `generate_llm_summary` (compaction), memory extraction and rerank (`memory.rs`, `memory_rerank.rs`), and read-only "explore" subagent profiles (`subagent_profiles.rs`) should default to a small or fast model at low effort. The main loop stays on the user's model.
- Later, add failure-triggered escalation (cheap first, escalate on a failed test).
- **Savings:** depends on the share of these calls. Measure it first through the per-call `usage`.
- **Risk:** weaker summaries lose facts. Guard: the six-section summary prompt plus a test that the summary keeps file paths.
- **Sources:**
  - [CodeRescue arXiv 2607.19338](https://arxiv.org/abs/2607.19338v1): beat always-escalate at 35% of its cost.
  - [Anthropic effort docs](https://platform.claude.com/docs/en/build-with-claude/effort) ("low … such as subagents").

**10. Cap the effective working context on very large windows**
- **Change:** in `context_budgets`, cap the compaction trigger at about 150–200k even on 1M models. Context rot means a 600k history makes the model slower, costlier and worse.
- **Uncertainty:** this is the least certain item. Compaction breaks the cache and can drop facts. Ship it only behind the eval, after item 7 has reduced how often it is needed.
- **Sources:** [Effective context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents).

**11. Trim the fixed prefix**
- **Change:**
  - In full mode, drop the `How:` text from the capability map where it repeats tool descriptions.
  - Move the chart/model3d format specs out of `BASE_DIRECTIVE` and into the description of the relevant tool or skill.
- **Savings:** about 2–3k tokens per request. Low priority while caching works, but it matters for subagents and cold starts.

## 3. Measurement: a small token and completion benchmark

### A. Static checks (cargo tests, no network, run in CI)

1. **`prompt_budget` snapshot.** Render `assemble_capability_blocks` and `select_tool_defs` for a fixture `AgentState` in both cloud and lean modes. Record the bytes of each block and fail on a regression of more than 10%. This becomes the "current budget" table above, measured exactly instead of estimated.
2. **Cache stability.** Simulate 30 loop steps with fixture tool results of mixed sizes. Run `prune_aging_tool_results`, `evict_old_images` and `convert_agent_messages_to_ai` each step, and compute the longest common prefix between consecutive requests. Assert that the prefix is at least the previous request's length on at least 90% of steps. This catches regressions of items 2, 3 and 7.

### B. Live suite (in-app, about 25 tasks, 3 seeds per configuration)

Each task has a **deterministic environment checker**, never an LLM judge:

- Coding in a fixture repo (about 8 tasks): the test passes and the diff touches the expected files.
- Read-only Q&A on the repo (about 5): the answer contains the key facts.
- Web lookup (about 4).
- Desktop typing in Notepad (about 4): the UI Automation text equals the expected text.
- Long multi-step todo or goal tasks (about 4).

**Logged per run (JSONL):**
- Σ input, cache_read, cache_write, output and thinking tokens, from the message `usage` fields.
- Steps, cost in dollars (the run ledger already has `spent_usd`), and wall time.
- The checker result.
- **Claimed success:** a regex or classifier over the final message.

**Metrics:**
- **Cost per successful task.** The primary metric, per [arXiv 2607.12161](https://arxiv.org/abs/2607.12161).
- Success rate, with non-inferiority set at −2 points.
- **False-success rate** = claimed ∧ ¬checker.
- Cache hit ratio.
- Re-read rate.

**Runner:** Anthropic calls go through the webview, so drive the suite through the existing Tauri command path with the app running, for example a hidden "eval" command that loads tasks and writes the JSONL. There is no headless entry point today.

## 4. Honest uncertainty

- Token sizes come from source bytes: the schema size is the figure in `tool_profile.rs` docs, and the capability map is estimated from its string literals. The static test in section 3A should replace these estimates with exact numbers.
- I did not confirm that `@ai-sdk/anthropic` 4.0.39 forwards message-level `cacheControl` exactly as expected. Its types expose `cacheControl`, `effort`, `contextManagement`, `deferLoading` and `toolSearch`, but a raw request dump should confirm it.
- I did not analyze the Bedrock or Azure paths in depth. The local llama.cpp path has its own prompt cache, which items 2 and 3 also help.
- The savings percentages for items 1, 2 and 7 come from my arithmetic or from papers on other harnesses. Vivianne's real numbers depend on its task mix, so measure before and after with the benchmark in section 3.
- Some figures come only from search-result summaries I couldn't fully verify: the 30–50-tool degradation and the Opus 49→74% result (taken from Anthropic's pages), and arXiv 2608.26442.
