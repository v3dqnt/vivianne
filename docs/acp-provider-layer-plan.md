# The ACP provider layer: from one generic runner to drivers, capabilities and sessions that last

Status: DRAFT from the T3 Code provider report the user supplied (2026-11-04). Decisions marked ❓ are asked before the phase starts.
Builds on: `src-tauri/src/agent/harness.rs` (today's single-file runner), `docs/threads-worktrees-plan.md` (threads, worktrees, thread messaging; its Phase B5/B5b is Phase P7 here).

## The design we are copying, in one paragraph

T3 splits a coding-agent integration into four layers. A **driver** says what an agent is (settings, how to start it). A **flavor** adapts the one shared ACP runtime to an agent's quirks (launch flags per approval mode, config-option ids, extra notifications). A **session runtime** owns one child process and one negotiated connection (`initialize`, `session/new` or `resume`, prompts, cancel, process-tree teardown). A **normalized adapter** turns that into the app's own events and **capabilities**, and the rest of the app reads capabilities, never the agent's name. Registry agents are the default path; a dedicated flavor exists only where an agent needs more than the protocol gives.

## Where `harness.rs` stands (verified in the code)

| T3 has | Vivianne today | Gap |
|---|---|---|
| `initialize` result kept: `agentCapabilities`, `authMethods`, `mcpCapabilities`, protocol v1/v2 | Result is thrown away after the call | No way to know what an agent can do |
| Auth methods (`env_var`, `agent`, `terminal`) and an `authenticate` call | A text guess on the error message (`explain_start_failure`) | A signed-out agent just looks stuck |
| `session/resume` / `session/load` with replay handling; native session id persisted on the thread | One in-memory session per (thread, agent), lost on app restart | Every restart loses the agent's context |
| Process-tree teardown (`taskkill /T /F`, process groups, cgroups) | `kill_on_drop` kills only the top process; `npx` on Windows is `npx.cmd` → `node` | Orphaned agent processes are likely |
| Cancel modes: interrupt, or wait for the prompt to answer `cancelled` | `session/cancel` once | Some agents never stop |
| `session/set_config_option`, `set_mode`, `set_model`; model and effort lists from `configOptions` | None; the model picker shows only the agent's name | Can't pick a model or effort for an agent |
| Approval mode mapped per agent (Grok flags, Kiro autopilot, fx modes); never persist `allow_always` | Permission dialog or "bypass" | Vivianne's Ask/Auto isn't mapped onto the agent's own modes |
| `mcpServers` carrying the app's tools (stdio self-invocation, per-session token) | Always `[]` | An agent can't use Vivianne's tools or message threads |
| `available_commands_update` → slash commands and skills | Ignored | |
| Registry settings: registry or local command, distribution `auto/binary/npx/uvx`, version-pinned install receipts, probe TTL | `npx`/`uvx` only; `-y <package>` unpinned; "ready" = it started once | Binary agents (Cursor, Amp, Antigravity) can't run; no pinned versions |
| A mock ACP peer and recorded transcripts to test the adapter | Only pure-function tests | The runner is untested without a real agent |

## Phases (risk first; every phase ends green on `cargo test --no-default-features --lib`, `npx tsc -p tsconfig.app.json --noEmit`, `npm test`)

### P0: Claude's first turn hangs: cause found, fix is a managed install - Size M
**Finding (2026-11-04, Test on Claude Agent):** the process starts (`npx.cmd -y @agentclientprotocol/claude-agent-acp@0.100.0`), prints only two npm warnings, and never answers `initialize` in 240 s. Run by hand with stdin closed it ran 60 s with almost no CPU and no output. The package exists (0.100.0 is `latest`). So it is waiting on the network: the first `npx` run downloads the agent and the large Claude Code binary it bundles, and nothing shows progress. A real first turn waits the same silent 240 s, which is the "stuck on hi" from before. Windows adds a second problem: `npx.cmd` starts `node` as a child, and we only kill the parent (see P2).
Verified when: Set up on Claude Agent shows a download with progress, ends in Ready, and the Test then reaches "Answer a prompt" in seconds; a thread on an agent that isn't set up says so at once instead of waiting.
- Install into a managed folder, `~/.viv/agents/<id>/<version>`, with `npm install --prefix <dir> <package>@<version>` (`--no-audit --no-fund`), reporting progress (bytes on disk, the last line npm printed) as an event the panel shows. Record an install receipt: `{ agentId, version, packageRoot, executablePath }`.
- Start the agent as `node <the package's bin script>` from that folder: no `npx` at run time, no network at start, no `npx.cmd` process pair.
- A turn on an agent that is not installed does not spawn it: it says "Set up <agent> first (a download of about N MB)" with the button.
- `initialize` times out at 30 s once installed (240 s only while installing).
- Tests: parse the package's `bin` entry (string and map forms); the receipt round trip; the not-installed refusal; progress-line parsing.
- Blocked by: none.

### P1: Keep what `initialize` says — Size M
Verified when: the Coding agents panel shows, per agent, its version, whether it can resume sessions, whether it needs sign-in and by which method, and what MCP transports it accepts.
- `AcpCapabilities` parsed from `initialize` (v1 and the v2 shape; Antigravity answers v2 with a v1-shaped body), stored on the connection and returned by a new `cmd_harness_probe` (initialize only: no session, no auth, nothing started beyond the process).
- Tests: parse fixtures for Claude, Codex, Gemini-shaped answers, a v2 answer, and a malformed one.
- Blocked by: none (can run beside P0).

### P2: Processes that actually stop — Size M
Verified when: after stopping a thread or closing the app there is no `node`/agent process left from the agent's tree (checked by listing child processes), and a cancelled prompt ends within its limit.
- Windows: `taskkill /PID <pid> /T /F`; POSIX: own process group and `kill(-pgid)`; a grace period before force.
- Cancel: `interrupt` (send `session/cancel`, expect `stopReason: cancelled`) and `wait-for-prompt`, with a timeout that then kills the tree.
- Stderr kept as a bounded ring (32 KB chunks, sanitised) for error messages.
- Blocked by: none.

### P3: Sign-in that you can see — Size M
Verified when: a signed-out agent shows "Sign in" with its method; completing it makes the next turn work without restarting the app.
- Methods: `agent` (call `authenticate`), `terminal` (open a terminal tab running the method's command), `env_var` (ask for the key, keep it in the vault, pass it as env). Retry the prompt after.
- Depends on P1. ❓ Which first: `agent` + `terminal` only, or `env_var` too? Recommend `agent` + `terminal`.

### P4: Sessions that survive a restart — Size M
Verified when: after restarting the app, the next message in an agent thread continues the agent's own conversation.
- Store `{ harness, acpSessionId, cwd }` on the thread's session file (`SessionInfo.harness`, serde default).
- Orchestration vocabulary (from the T3 orchestration report): a steer or a retry of an agent turn is a new *attempt* on the same run, so the restarted prompt is linked to the one it replaces; the agent's session id is part of the thread's persisted row, and moves into the thread event log when B7 lands.
- `session/resume` when `sessionCapabilities.resume`, else `session/load` with replay: ignore replayed `session/update`s until a 2 s idle gap; else start fresh and hand over a summary of the transcript.
- Depends on P1, B1 (thread ids). Tests with the mock peer (P9).

### P5: Flavors — Size M
Verified when: a flavor changes launch flags and config for an agent without touching the shared runtime.
- `trait Flavor { fn spawn(runtime_mode) -> SpawnInput; fn session_config(policy) -> Vec<ConfigWrite>; fn permission(option_kinds, policy) -> Disposition; fn model_id(selection) -> String }` with a default impl for registry agents.
- First flavors: Claude (subagent updates from `Agent`/`Task` tools), Codex, Gemini. Later: Grok, Kiro, fx, Antigravity, Copilot (listed in the report with their exact flags and config ids).
- Modes to map: T3 has four runtime modes (approval-required, auto-accept-edits, auto, full-access), and a side chat that shares a worktree starts in approval-required. Vivianne has Ask and Auto plus bypass; decide whether to add "auto-accept edits" before mapping.
- A coding agent's own subagents (Claude's `Agent`/`Task` tools) are shown read-only: they can't be messaged, unlike threads made with `spawn_thread`.
- Permission decisions: map the agent's `allow_once` / `allow_always` / `reject_*` options to Vivianne's choices (accept, accept for this session, decline, cancel); "accept for session" is remembered for the thread only and never sent to the agent as `allow_always`. A pending request puts the thread in `waiting`, which the sidebar shows. An agent calling a thread tool can never approve another thread's permission request.
- Rules from the report: offer only approval modes the agent enforces itself; never persist `allow_always` on policy approvals; auto mode asks only for what the agent refused itself.
- ❓ Which agents first. Recommend Claude, Codex, Gemini, and everything else through the generic path.

### P6: Models and effort from the agent — Size M
Verified when: the model picker lists an agent's own models and effort levels and a choice takes effect in the next turn.
- Read `configOptions` from `session/new` (models can arrive late in a `config_option_update`); write with `session/set_config_option` (v1: `session/set_model`/`set_mode`).
- Depends on P1, P5.

### P7: Vivianne's tools inside the agent — Size M (this is B5b in the threads plan)
Verified when: a real Claude Code thread calls `list_threads` and `message_thread`.
Design from the T3 tools report: ONE tool server, two transports.
- **Server:** an MCP server inside the running app, on loopback only (`127.0.0.1:<random>/mcp`, MCP 2025-06-18), one registration of every tool, each request authorised by a per-session bearer token that names the thread, the agent and what it may do. Tools declare whether they read, write threads, start threads or write the environment; a read-only session is refused the writers, and a tool above the thread's approval mode is refused.
- **Transports:** HTTP direct to agents whose `initialize` reports `mcpCapabilities.http` (Claude, Cursor, Codex take it in their config); for the rest a **stdio bridge**, our own binary started as `--mcp-bridge` with the endpoint and token in its environment, relaying JSON-RPC verbatim. Last resort for an agent that takes no MCP at all: a command it can run in its terminal (`viv-tool <name> '<json>'`).
- **Per-thread session record:** `{ thread id, agent session id, endpoint, authorization, capabilities }`, created when the agent starts and revoked when it stops; it also decides which instructions are added.
- **Instructions:** when tools are available the first prompt is wrapped as `<viv_instructions>…</viv_instructions><user_request>…</user_request>`, except a prompt that starts with `/` (a slash command passes through untouched). A budget per agent (Codex's is about 4,000 bytes); the block is repeated after a compaction. It says: ordinary coding tasks need no Vivianne tools; call `list_threads` once before the first `spawn_thread`; never ask for a secret in chat.
- **Tool names and behaviour** (threads plan B4): `list_threads`, `read_thread`, `message_thread`, `ask_thread`, `wait_thread` (long timeout; excluded from the normal tool list for agents with a short tool timeout), `interrupt_thread`, `spawn_thread`, `thread_configure`, `pending_requests` (list and read only: **it cannot approve a permission request**), `thread_transfer` (fork, merge back). Worktree and pull-request tools (`worktree_status`, `worktree_handoff`, `link_pull_request`, `watch_pull_request`) come later with B3.3.
- Depends on P1 (transports), P5, threads-plan B4.1.

### P8: The registry done properly — Size L, split in two
- P8a: settings (`source: registry|local`, `distribution`, command and args override), version-pinned `npx`/`uvx` packages, install receipts (what was installed, where), probe results cached 15 min.
- P8b: binary distribution (archive URL, sha256, platform target, unpack, mark executable) for Cursor, Amp, Antigravity.
- ❓ P8b now or later. Recommend later.

### P9: A mock agent to test against — Size M (build early; P2 and P4 need it)
- A small test peer that speaks ACP over stdio (initialize, session/new, prompt with scripted updates, permission requests, cancel, resume), plus recorded transcripts from real agents replayed through the runner.
- Verification: tests for update mapping, permission flow, cancel, resume, stderr capture, process-tree teardown.

### P10: Richer events — Size S per item, after P1
- **Slash commands and skills:** `available_commands_update` fills the thread's `/` menu; skills are read from the agent's own folders without opening a session (front matter `name`, `description`, `disable-model-invocation`, `user-invocable`), with scan limits of 10,000 entries, 1 MB per file, 8 MB in all and depth 10. `/usage` is handled by Vivianne and not sent to the agent.
- **Per-turn usage:** the prompt response's `usage` (`totalTokens`, `inputTokens`, `outputTokens`, `thoughtTokens`, `cachedReadTokens`, `cachedWriteTokens`) becomes the turn's token usage: cache reads and writes are parts of input, thought tokens a part of output, each clamped to its parent; status `complete`, `partial` or `unavailable`; the context ring uses `usage_update` and the model's window when the agent reports them. Gemini counts cache and thoughts differently, so it gets a flag. Feeds the existing cost ledger (`usage_by_source`).
- **Subagent updates → task rows** (Claude's `Agent`/`Task` tools, read-only); plan and diff mapping already exist.
- **Failure class `usage_limit`:** recognised from each agent's own signal (Grok's rate-limit code `-32003`, Vibe's `_session/retrying` `-31001`, Cursor's `resource_exhausted`/`*_usage_limit`) with its reset time; feeds B9's resume.
- **Run metrics:** counters for runs finished by outcome, queue wait and duration, shown in Diagnostics; never stored.

## Decision log (proposed)

| Decision | Choice | Why |
|---|---|---|
| Shape | one shared runtime + `Flavor` trait + capabilities struct | the report's own conclusion: orchestration reads capabilities, never the agent's name |
| Where | split `harness.rs` into `agent/harness/{mod,runtime,flavor,registry,probe,mock}.rs` as phases land | `harness.rs` is ~1000 lines already |
| Protocol versions | v1 first, parse the v2 `initialize` shape from the start | agents already answer v2 with a v1 body |
| Session continuity | persist the agent's session id on the thread | restart must not lose context |
| MCP into agents | stdio self-invocation with a per-session token | what T3 does; every agent must accept stdio |

## Not yet specified

- P3: how `terminal` auth methods run on Windows (an interactive shell tab is the working assumption).
- P7: transports each agent really accepts (read from `mcpCapabilities` in P1, then decided per flavor).
- P8b: archive formats and per-platform targets in the registry.

## Always / Never

- Always: the Test panel (`cmd_harness_debug`, trace, reset) keeps working through every phase; a thread whose agent fails shows the reason, never a silent hang.
- Never: persist an agent's blanket "always allow" from a policy approval; leave an agent process running after its thread ends; send an API key anywhere but the agent's own environment.
- Out of scope: writing a Vivianne-native adapter for Cursor's SDK, OpenCode's HTTP server or Codex's `app-server` protocol (those are separate, non-ACP integrations, listed in the report for later).

## Progress

| Phase | Status |
|---|---|
| P0 Why the first turn hangs | running the Test now |
| P1 Keep `initialize` | not started |
| P2 Processes that stop | not started |
| P3 Sign-in | not started |
| P4 Resume across restarts | not started |
| P5 Flavors | not started |
| P6 Models and effort | not started |
| P7 Vivianne's tools in the agent | not started |
| P8 Registry | not started |
| P9 Mock agent | not started |
| P10 Richer events | not started |
