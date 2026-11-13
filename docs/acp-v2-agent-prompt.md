# Prompt for a coding agent: move Vivianne's Claude/ACP layer to ACP v2

Paste everything below the line into the agent. It is self-contained.

---

## Task

Upgrade Vivianne's ACP harness layer (how Vivianne runs Claude Code, Codex, Gemini CLI and other coding agents as models) from ACP **v1** to ACP **v2**, by porting the behaviour of the reference implementation in t3code. Work in phases. Finish and verify one phase before starting the next. Do not commit or push unless asked.

## Where things are

- Vivianne worktree (work here): `C:\Users\v3dqn\Work\Project Versa\viv-main-gui-exp-ui-polish` (branch `updated-threads`). Windows. Use Git Bash or PowerShell.
- ACP layer to change: `packages/gui/src-tauri/src/agent/harness.rs` (Rust, tokio). Related: `agent/harness_install.rs`, the caller at `agent/runtime/commands.rs` (search `harness::run_turn`), permission prompts in `agent/tools/exec.rs` (`ask_permission`), events in `agent/runtime/events.rs` (`AgentEvent`), MCP config in `mcp/mod.rs`, background-process helper `proc.rs` (`crate::proc::quiet`).
- Reference implementation (read-only, TypeScript + Effect): `C:\Users\v3dqn\Work\Project Versa\t3code`. Key files: `packages/provider-acp/src/server/AcpSessionRuntime.ts`, `packages/provider-acp/src/server/adapter.ts` (9,234 lines), `packages/provider-acp/src/server/runtimeModel.ts`, `packages/provider-acp/src/server/clientPolicy.ts`, `packages/provider-acp-registry/src/server/adapter.ts`, `packages/provider-acp-registry/src/server/claudeAcp.ts`, `packages/effect-acp/src/client.ts`, `packages/effect-acp/src/compat.ts`.
- Notes already written, read these first: `docs/acp-claude-port-plan.md` (phases, differences, decisions) and `docs/acp-t3code-report.md` (an earlier explorer's report on t3code).

## Read this before trusting the notes

`docs/acp-t3code-report.md` was written by a quick explorer that read only about 15-20% of t3code's `provider-acp/src/server/adapter.ts` and grepped the rest. Treat it as a map, not as truth. Before each phase, read the exact t3code line ranges listed for that phase in the plan yourself, and quote what you rely on. Where the report and the source disagree, the source wins. If something the plan needs is in a part nobody read (the plan lists them under "Known gaps"), read it first.

Also confirm v2 against the official Agent Client Protocol spec (https://agentclientprotocol.com, and the schema in the `agentclientprotocol` GitHub repo), not only against t3code. t3code calls v2 a "preview" protocol; field names can differ from what it uses. If the spec and t3code differ, follow the spec and say so.

## What "v2" means here (from t3code, to be confirmed against the spec)

- `initialize` sends `protocolVersion: 2` and richer `clientCapabilities` (fs and terminal false; `elicitation: { form: {} }`; `session: { notices: {} }`).
- For Claude, launch the agent with `CLAUDE_AGENT_ACP_EXPERIMENTAL_V2=1` (the `claude-agent-acp` adapter reads it).
- A turn does **not** end when the `session/prompt` response returns. It ends when the agent sends a `state_update` with state `idle` (its `stopReason` says completed, cancelled or error). While an approval or question is pending (`requires_action`) the turn stays open.
- New updates: `state_update`, `plan_update`, `plan_removed`, `notice`, `session_info_update`, `compaction_update`, `compaction_summary_chunk`, richer `usage_update`, `available_commands_update`, `config_option_update`, `current_mode_update`, and subagent / child-session updates that arrive wrapped as an unknown update type (t3code wraps them as `_t3_unknown`).
- New methods: `session/resume`, `session/close`, `elicitation/create`, plus capability-gated list/fork/delete.
- MCP can be offered to the agent three ways, picked by what it advertises: over ACP (`mcpCapabilities.acp`), HTTP (`mcpCapabilities.http`), or stdio.

## Current state of Vivianne's layer (v1), so you know what exists

- Registry fetched from `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`, cached 24 h; agents run through `npx -y` / `uvx` or an install under `~/.viv/agents`.
- `Conn` holds the child process, pending requests, an `initialize` answer (`caps`), a session id, `seen` (how many thread messages the agent's session has been told about), and the last reported cost. One connection per (thread, harness) in `CONNS`.
- `spawn()` sends `protocolVersion: 1` and rejects an agent that answers a different version. **This check must change with the move to v2.**
- `run_turn` builds a prompt (a digest of messages the session missed, then the user's text and images if the agent accepts images), sends `session/prompt`, and loops on updates until the prompt response returns. It handles message and thought chunks, `tool_call` / `tool_call_update`, `plan`, and `usage_update` (cost only).
- Sessions resume after a restart through `session/load` using `~/.viv/acp-sessions.json`.
- `session/new` passes the user's own MCP servers (stdio and, when the agent allows it, HTTP; not keychain-signed or built-in ones).
- `task` can delegate a job to a harness (`harness::delegate`), with the turn's stop flag and a 20-minute limit.
- Permission requests are answered through `ask_permission` (900 s timeout) **inside the update loop**, so Stop and other updates are dead while a prompt is open. Approval only ever selects `allow_once`.
- Known weaknesses to fix as part of this work: no kill fallback when an agent ignores `session/cancel`; no authentication flow (only text telling the user to sign in); only `session/load` for resume; most v2 updates ignored.

## Rules that must not break

- **Never select `allow_always`** (or any standing grant) on the user's behalf. Approval is per call. This is a deliberate difference from t3code.
- **Keep the process-tree kill** (`TreeKillOnDrop`) and `own_group`. t3code does not tree-kill on its Claude path; Vivianne does.
- **No console windows on Windows.** Start background programs through `crate::proc::quiet` / `quiet_std`, or set `CREATE_NO_WINDOW`.
- Do not change Vivianne's own agent loop, the provider/model code, or anything outside the ACP layer, except the narrow edits a phase names.
- Stay compatible with v1 agents: negotiate by the agent's `initialize` answer, as t3code does. Do not break Codex, Gemini CLI, Copilot, Qwen Code or the others that may still speak v1.
- No new dependencies unless a phase cannot be done without one; say why if you add one.
- Keep the user-facing error text plain. Do not put secrets in logs or traces.

## Phases (do in this order)

**Phase 0 — mock agent and replay tests.** Build a scripted ACP agent for tests (a pair of in-memory pipes or a small test binary) and a helper that drives `run_turn` against it and captures emitted `AgentEvent`s. `run_turn` has no such test today. t3code reference: `apps/server/src/orchestration-v2/Adapters/AcpRegistryAdapterV2.test.ts` (lines 120-200, 1018-1050, 1390-1420, 1548-1628), `apps/server/scripts/acp-mock-agent.ts`. Done when a test drives `initialize`, `session/new`, a streamed reply, a tool call and a permission request and asserts the events.

**Phase 1 — handshake and sessions.** `protocolVersion: 2` with the v1 fallback by the agent's answer; v2 `clientCapabilities`; Claude launch env; replace the strict version check with negotiation; `authenticate` with the advertised method (type `agent` only) and one retry of session setup on an auth-required error; resume by `session/load` (quarantine replayed updates: 90 s total, 2 s idle gap) else `session/resume`, else a clear error. t3code: `AcpSessionRuntime.ts` 2085-2284 and 1949-2028; `provider-acp-registry/src/server/adapter.ts` 195-210.

**Phase 2 — permissions and elicitation.** Policy by tool kind (read, search, think allowed without asking; edit, delete, move, execute follow Vivianne's permission mode); handle permission requests off the update loop so Stop works while a prompt is open; `elicitation/create` form requests shown as a question and answered. Keep `allow_once`-only. t3code: `adapter.ts` 6371-6560 and 1272-1293; `clientPolicy.ts` (whole file).

**Phase 3 — stop and failure.** `session/cancel` non-blocking; session-wide Stop also sends `session/close` and waits up to 10 s; add a kill fallback (end the process tree if the turn has not ended a few seconds after cancel); attach the agent's last stderr lines to failures (wait ~250 ms for the drain). t3code: `AcpSessionRuntime.ts` 2366-2412 and 1298-1319; `adapter.ts` 8640-8660 and 2286-2294.

**Phase 4 — v2 turn lifecycle.** End the turn on `state_update` idle (stop reason decides completed, cancelled, error); keep it open while approvals or questions are pending; handle `requires_action`; do not finalise on the prompt response when the agent follows work state. Wrap unknown update types so none are lost. Read first: `adapter.ts` 4790-4830, 5256-5275, 8198-8224; `claudeAcp.ts`; `effect-acp/src/client.ts` near lines 291-304 and 500-520; `AcpSessionRuntime.ts` 1716-1773. This is the riskiest phase; the plan lists it as the thinnest in the earlier report, so read widely and test hard.

**Phase 5 — update mapping.** `usage_update` as context meter (used / size / cost), `session_info_update` (thread title), `plan_update` / `plan_removed`, `notice`, `available_commands_update`, `current_mode_update` / `config_option_update`, compaction, unknown update as a quiet row. Emit events from the backend; keep any UI change minimal and separate. t3code: `runtimeModel.ts` 1363-1620; `adapter.ts` 4864-5447. Subagent / child-session routing is optional; do it last and only if the earlier phases are solid.

**Phase 6 — Vivianne's tools inside the agent.** Expose Vivianne's own tools (memory, browser, vault, project tools) to the agent as an MCP server, chosen by what the agent advertises (ACP, HTTP, stdio), authenticated per thread, gated by Vivianne's permission rules. Write a short design note first and stop for review before coding. t3code: `adapter.ts` 812-880; `AcpSessionRuntime.ts` 2135-2149; `mcpOverAcpBridge.ts`.

**Phase 7 — prompt parity (optional).** Runtime-instructions block re-sent only when its state changed or after compaction; none for a native slash command. t3code: `adapter.ts` 7697-7829, 8160-8181.

## How to verify

- Rust: `cd packages/gui/src-tauri` then `cargo test --lib harness`. Add tests with each phase; no phase is done without them. A full `cargo test --lib` should stay green.
- Frontend (if touched): `cd packages/gui-next` then `npx tsc -p tsconfig.app.json --noEmit` and `node --test "src/**/*.test.ts"`.
- Before calling a phase done, run it against a real agent if one is installed (Claude Code via `claude-acp`): one plain prompt, one that edits a file and asks permission, one Stop mid-turn. Say plainly what you did and did not run.
- Report per phase: what changed, tests added and their result, what you read in t3code (file and lines), what you could not verify.

## What to hand back

A short summary per phase, a list of every t3code range you read, every place you deviated from t3code and why, and anything you left undone. Do not commit; leave the working tree for review.
