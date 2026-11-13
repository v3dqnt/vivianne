# Port plan: t3code's Claude-over-ACP behaviour into Vivianne

Source studied: `C:\Users\v3dqn\Work\Project Versa\t3code` (TypeScript, Effect). Target: `packages/gui/src-tauri/src/agent/harness.rs` (Rust, tokio).
Full source report: `C:\Users\v3dqn\AppData\Local\Temp\claude\14dcedbf658dfa5c\828c379f-f468-4ffc-8f78-22f373a87ab5\scratchpad\t3code-claude-acp.md`

## Ground rules

- **Not a literal copy.** Different language and runtime. The port matches t3code's protocol calls, ordering, option handling and event mapping step for step; the code is Rust.
- **The report is partial.** The explorer read about 15-20% of t3code's `provider-acp/src/server/adapter.ts` (9,234 lines). Before each phase, read the exact t3code ranges listed under it yourself, not the summary.
- **One phase at a time.** Each phase ends with tests that pass and a build that is clean. No phase starts before the one before it is merged.
- **Test harness first.** t3code tests Claude with replayed ACP frames and a mock agent (`apps/server/scripts/acp-mock-agent.ts`, `packages/effect-acp/test/fixtures/acp-mock-peer.ts`). Vivianne's `run_turn` has no such test. Phase 0 builds a scripted mock agent so every later phase is tested against frames, not by hand.

## Decision needed: protocol version

t3code hard-codes ACP **v2** (`protocolVersion: 2`) and launches Claude with `CLAUDE_AGENT_ACP_EXPERIMENTAL_V2=1`. In v2 a Claude turn ends on the agent's `state_update` with state `idle`, not on the `session/prompt` response. Vivianne speaks **v1** and ends a turn on the response.

- **Option A (recommended): v1 first.** Phases 1-4, 6-8 work on v1. Phase 5 (v2 turn lifecycle) comes last and is only done if wanted. Lowest risk.
- **Option B: v2 now.** Closest to t3code, but v2 is a preview, and a mistake in turn completion leaves turns hanging or ending early.

Note: `spawn()` in `harness.rs` now rejects an agent whose `initialize` answer has a different `protocolVersion`. Moving to v2 means changing that check at the same time.

## Where Vivianne differs from t3code today

| Area | t3code | Vivianne now |
|---|---|---|
| Handshake | v2; `clientCapabilities` has fs/terminal false, `elicitation.form`, `session.notices` | v1; fs/terminal false only |
| Auth | picks an `authMethod`, calls `authenticate`, retries session setup once; rejects non-agent auth types | shows sign-in instructions as text |
| Resume | `session/load` (replay quarantined) else `session/resume`, else error | `session/load` only, ids kept in `acp-sessions.json` |
| Permissions | policy by tool kind (reads never ask); maps accept / accept-for-session / decline / cancel to option kinds | every request asks; picks `allow_once` only |
| Permission wait | no timeout; does not block other updates | 900 s timeout, awaited inside the update loop so Stop is dead meanwhile |
| Stop | `session/cancel` (non-blocking), then `session/close` with a 10 s confirm for a session-wide Stop | `session/cancel` sent from a 200 ms tick; no close; no kill fallback |
| Updates handled | plan, plan_update, plan_removed, tool_call(_update), usage_update, session_info_update, notice, available_commands_update, mode/config updates, compaction, thought/message chunks, state_update, subagent updates, unknown | message/thought chunks, tool_call(_update), plan, usage_update (cost only) |
| MCP | injects the app itself as an MCP server (stdio bridge, ACP or HTTP when the agent advertises it) | passes the user's own MCP servers only |
| Prompt | text, then images (gated), then a runtime-instructions block re-sent only when state changes | digest of missed history, text, images (gated) |
| Install | versioned dir, lock file, receipts checked against `package.json`, binary distributions with sha256 | managed install and receipt; no binary distributions |
| Process end | child kill only; no tree kill on the registry Claude path | whole process tree killed (better than t3code) |

## Phases

### Phase 0 — mock agent and replay tests
Scripted JSON-RPC agent as a test binary or in-process pair of pipes, plus a `run_turn` test helper that feeds it frames and captures emitted events.
- t3code to read: `AcpRegistryAdapterV2.test.ts` lines 120-200, 1018-1050, 1390-1420, 1548-1628; `acp-mock-agent.ts`.
- Done when: a test drives `initialize`, `session/new`, a prompt with streamed chunks, a tool call and a permission request, and asserts the events.

### Phase 1 — handshake and session parity
- Add `elicitation: { form: {} }` and `session: { notices: {} }` to `clientCapabilities`.
- Keep the whole `initialize` answer (already stored in `Conn.caps`); read `agentCapabilities.sessionCapabilities.resume`.
- Authentication: choose the advertised method (configured id, else first of type `agent`), call `authenticate`, retry session setup once on an auth-required error. Reject non-`agent` types with a clear message.
- Resume: `session/load` with replay quarantined (90 s total, 2 s idle gap), else `session/resume`, else a clear error. No silent fall back to a new session when the user expects history; a new session is still correct for a first turn.
- Claude launch env: set `CLAUDE_AGENT_ACP_EXPERIMENTAL_V2` only together with the Option B decision.
- t3code to read: `AcpSessionRuntime.ts` 2085-2284 and 1949-2028; `packages/provider-acp-registry/src/server/adapter.ts` 195-210.

### Phase 2 — permissions
- Policy by tool kind: read, search, think are allowed without asking; edit, delete, move, execute follow Vivianne's permission mode (ask, auto-accept edits, bypass).
- Move permission handling off the update loop so Stop and other updates keep flowing while a prompt is open.
- Map decisions to option kinds as t3code does (`allow_once`; accept-for-session to `allow_always` else `allow_once`; decline to `reject_once`; cancel). **Deliberate difference kept:** Vivianne never selects `allow_always`; accept-for-session is mapped to `allow_once` unless the user decides otherwise, because a standing grant outlives what the user saw.
- Elicitation (`elicitation/create`, form mode): show as a question card and answer it.
- t3code to read: `adapter.ts` 6371-6560, 1272-1293; `clientPolicy.ts` (whole file, 250 lines).

### Phase 3 — stop and failure handling
- `session/cancel` stays non-blocking. For a session-wide Stop also send `session/close` and wait up to 10 s.
- **Beyond t3code:** t3code has no kill fallback for Claude; Vivianne adds one (end the process tree if the turn has not ended a few seconds after cancel).
- On process exit, attach the last stderr lines (wait about 250 ms for the drain) to the error.
- t3code to read: `AcpSessionRuntime.ts` 2366-2412, 1298-1319; `adapter.ts` 8640-8660, 2286-2294.

### Phase 4 — update mapping
Add, in order of user value: `usage_update` as a context meter (used / size / cost); `session_info_update` (thread title); `plan_update` / `plan_removed`; `notice`; `available_commands_update` (slash commands); `current_mode_update` and `config_option_update` (mode and model options); compaction; unknown update shown as a quiet row.
- t3code to read: `runtimeModel.ts` 1363-1620; `adapter.ts` 4864-5447.
- UI work for the meter, commands and notices is separate and small; the backend emits first.

### Phase 5 — turn lifecycle on v2 (only with Option B)
`state_update` handling, turn ends on `idle`, `requires_action`, pending-request gate, `_t3_unknown` wrapping of non-standard updates, subagent / child-session routing.
- t3code to read: `adapter.ts` 4790-4830, 5256-5275, 8198-8224; `claudeAcp.ts` (whole file); `effect-acp/src/client.ts` around 291-304 and 500-520; `AcpSessionRuntime.ts` 1716-1773.
- Needs a second explorer pass first: that part of the report is the thinnest.

### Phase 6 — Vivianne's own tools inside the harness
Expose Vivianne tools (memory, browser, vault, project tools) to the agent as an MCP server, chosen by what the agent advertises: ACP-tunnelled MCP if `mcpCapabilities.acp`, else HTTP if `mcpCapabilities.http`, else a stdio bridge. Authenticated per thread, tool gating follows Vivianne's permission rules.
- t3code to read: `adapter.ts` 812-880; `AcpSessionRuntime.ts` 2135-2149; `mcpOverAcpBridge.ts` (whole file).
- Largest phase and the real answer to "the harness has none of Vivianne's tools". Needs its own design pass before code.

### Phase 7 — prompt parity
Runtime-instructions block re-sent only when its state changed or after compaction; none for a native slash command. Keep the digest of missed history (t3code relies on the agent's own session and has a separate context-transfer step).
- t3code to read: `adapter.ts` 7697-7829, 8160-8181.

### Phase 8 — install parity (optional)
Versioned install directory, lock file, receipts validated against the installed `package.json`, and binary distributions with sha256. Only matters for agents Vivianne cannot launch through npx or uvx.
- t3code to read: `AcpRegistrySupport.ts` 1058-1354, 1396-1595.

## Kept as they are (Vivianne already better)
Process-tree kill on drop; never granting a standing permission; digest-based history handoff for model switches; usage taken from the prompt response when the agent reports it.

## Known gaps in the t3code report to close before the phases that need them
`AcpSessionRuntime.ts` 230-1284 and 2410-2540; `adapter.ts` 1658-1900, 7840-8120, 5496-5520; `effect-acp/src/client.ts`; `probe.ts`; `AcpRegistryRuntimeCoordinator.ts`.
