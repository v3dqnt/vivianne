# Vivianne Backend Rewrite Plan

**Status:** Draft v1 — 2026-08-17
**Owner:** Vivianne engineering
**Reference clone:** `scratch/pi-reference/` (git clone of `https://github.com/earendil-works/pi`, commit `d3ab2af` — "fix: track kimi cached tokens (#8119)")
**Goal:** Replace the current Rust/Tauri backend (≈33,300 lines, corrupted by many uncoordinated changes) with a new, modular, faster backend that `gui-next` talks to, porting the architecture of the `pi` coding-agent project.

---

## 0. Executive summary

- The old backend (`packages/gui/src-tauri/src`, ≈33,313 lines) grew organically across many sessions and is now unreliable. `gui-next` needs a clean, fast, maintainable backend.
- **Critical discovery: `pi` is a TypeScript/Node monorepo with zero Rust.** A literal code copy is impossible. The port is an **architectural port**: pi's proven design (agent state machine, model/provider abstraction, CBOR protocol, server/client split) becomes the blueprint for a new Rust backend.
- Two viable strategies, decided in §5:
  - **A (recommended):** Port pi's *design* to idiomatic Rust — a self-hosted Rust agent server + thin Tauri shell, GUI speaking a versioned protocol. Highest performance, matches the "rewrite the Rust backend" ask, largest effort.
  - **B (low-risk alternative):** Keep pi's proven TypeScript `pi-server`/`pi-agent-core` as a managed sidecar process and make the Tauri shell + gui-next its clients. Fastest to "new and improved backend," but the core is not Rust.
- The rewrite must preserve the **existing command surface** `gui-next` already calls (§3) so the frontend keeps working during cutover, then evolve it toward the pi-derived protocol.

---

## 1. Reference clone (done)

- Location: **`scratch/pi-reference/`** — full working-tree clone, browseable in the IDE.
- Remote: `https://github.com/earendil-works/pi` (shallow, depth 1).
- Refresh anytime with:
  ```bash
  git -C scratch/pi-reference pull --depth 1
  ```
- Read these first to learn pi's architecture:
  - `README.md` — what pi is (interactive coding agent, monorepo).
  - `tui-plan.md` — layout/state philosophy (relevant to future GUI state work).
  - `packages/agent/src/` — **the port blueprint** (agent loop + harness).
  - `packages/ai/src/` — model/provider abstraction (≈16,300 lines).
  - `packages/protocol/src/` — CBOR codec, framing, schemas (≈790 lines).
  - `packages/server/src/` — connection, sessions, snapshots, transports.
  - `packages/client/src/` — how a client talks to the server.
- Do **not** edit the clone; it is read-only reference material.

### What pi is made of (port targets)

| pi package | Purpose | Lines | Port target in Rust |
|---|---|---|---|
| `agent` (`pi-agent-core`) | Agent loop, harness, reducer, messages, events, compaction, session/state, JSONL storage, tools (bash, edit-diff, read, write, image) | ~7,757 | `agent/` core: state machine, session store, tool registry |
| `ai` (`pi-ai`) | Unified model API, model catalog, providers (Anthropic, OpenAI, Google, Bedrock, Mistral), image API | ~16,329 | `ai/` provider layer, model registry, credential pool (exists) |
| `protocol` (`pi-protocol`) | CBOR codec, framing, request/response schemas | ~790 | `protocol/` module or `ipc_protocol.rs` v2 |
| `server` (`pi-server`) | Listener, connections, sessions, snapshots, unix-domain transports | ~— | `orchestrator/` → real server process |
| `client` (`pi-client`) | Client connection, session handles, state | ~— | `gui-next` IPC layer or Tauri `ipc/` |

---

## 2. Current backend inventory (`packages/gui/src-tauri/src`)

Total ≈33,313 lines. The top of the tree (oldest/most-mutated) is the agent core.

| Module | Lines | Notes |
|---|---|---|
| `agent/` | 11,521 | `loop_core.rs` (2,082), `commands.rs` (1,921), `tool_exec.rs` (1,683), `memory.rs` (930), `memory_store.rs` (876), projects, skills, playbooks, llama manager, notifications |
| `coding_agent/` | 1,020+ (tool_exec only; total unmeasured) | Grok-parity subagents, plan mode, ask-user, streaming bash/powershell, computer/capture_screen, patch apply, sandbox |
| `orchestrator/` | 748 | `supervisor.rs` (313), `commands.rs` (131), `ipc_protocol.rs` (124), `storage.rs`, `types.rs` — proto-server: spawn/list/status/stop/rpc/rpc_stream |
| `ai/` | unmeasured | auth (keychain + file), credential pool, local registry, llama manager |
| `mcp/` | 498 | transport (271), oauth (227) |
| `lsp/` + `terminal.rs` + `automation.rs` + `voice/` | 1,871 (grouped) | integrations measured as a group; `gesture/`, `camoufox.rs`, `docker_installer.rs` exist but were not line-counted |

### Signs of corruption / why a rewrite
- 2 `TODO`/unimplemented markers already in `loop_core.rs`; many subsystems carry vestigial code from abandoned experiments (per `VIVIANNE-GUI-DEEP-SCAN.md`, "identified problems and roadmap" section).
- The agent loop, commands, and tool execution are entangled in giant files; behavior drifted from what `gui-next` expects (the old frontend defensive-read both `snake_case` and `camelCase` for the same fields).
- One global `AppState` plus an `AgentRegistry` for docked panes — concurrency and ownership are fragile.

---

## 3. The contract that must survive (what `gui-next` calls today)

The typed boundary lives in `packages/gui-next/src/ipc/index.ts` (`call`/`on`). A grep of `call('...')` across `packages/gui-next/src` gives the **verified** command surface — this is what the rewrite must keep working (same names, same shapes) until cutover.

### 3a. Verified `gui-next` contract (≈50 commands — must survive)

**Agent:** `cmd_agent_prompt`, `cmd_agent_abort`, `cmd_agent_reset`, `cmd_agent_steer`, `cmd_agent_steering_queue`, `cmd_agent_follow_up_queue`, `cmd_agent_clear_queues`, `cmd_agent_remove_queued`, `cmd_agent_set_model`, `cmd_agent_set_thinking_level`, `cmd_agent_set_workspace`, `cmd_agent_revert_to_index`, `cmd_agent_answer_question`, `cmd_agent_respond_permission`, `cmd_agent_respond_confirmation`
**Sessions/projects:** `cmd_load_session`, `cmd_delete_session`, `cmd_rename_session`, `cmd_create_project`, `cmd_rename_project`, `cmd_delete_project`, `cmd_assign_session_project`
**Settings/endpoints:** `cmd_save_settings`, `cmd_register_local_endpoint`, `cmd_unregister_local_endpoint`, `cmd_llama_register/start/stop/unregister`
**Auth/pool:** `auth_store_api_key`, `auth_delete_credential`, `cmd_pool_add/remove/set_strategy/reset_cooldowns`
**Memory:** `cmd_memory_purge`
**MCP:** `cmd_mcp_add_server`, `cmd_mcp_remove_server`
**Terminal:** `cmd_terminal_spawn/write/resize/close`
**Automation:** `cmd_automation_create/update/set_enabled/delete/run_now`
**Coding-agent:** `cmd_coding_kill_subagent`
**Playbooks:** `cmd_add_playbook_file`, `cmd_remove_playbook`
**Gesture:** `cmd_gesture_start/stop/set_debug`

### 3b. Backend surface NOT called by gui-next (port only if needed, else delete)

These exist in `lib.rs` but a grep found no `call()` for them in gui-next: `cmd_agent_continue`, `cmd_agent_messages`, `cmd_agent_set_persona_mode`, `cmd_compact_now`, `cmd_get_sessions`, `cmd_get_settings`, `cmd_get_models_config`, `cmd_save_models_config`, `cmd_llama_status/list`, `cmd_mcp_list_servers/list_tools/authorize`, `cmd_terminal_list_sessions/list_shells`, `cmd_automation_list`, all memory commands except `purge`, `cmd_coding_*` beyond `kill_subagent`, `cmd_web_preview_*`, `cmd_git_*`, `cmd_lsp_diagnostics`, `cmd_whisper_*`, `cmd_check_docker_status`/`cmd_install_docker`, orchestrator `cmd_orch_*`, file ops `cmd_list_dir_tree`/`cmd_read_file_content`/`cmd_write_text_file`.

> **Decision rule:** the single biggest win of this rewrite is deleting ~40 commands nothing calls. Default = drop 3b unless a feature (web preview, git panel, voice, LSP, docker) is actually used by a shipped surface. Verify with the user per feature before keeping.

### 3c. Events — CONTRACT UNVERIFIED

The events `coding_ask_user_question`, `automation_fired`, `voice-wake`, streaming channels are backend `emit()` calls in `lib.rs`, but a grep found **no `on('...')` listeners anywhere in gui-next** (`listen` appears only in the IPC wrapper and the AI-provider dispatcher). The real event contract must be established in Phase 0 by auditing gui-next components before freezing it — do not assume the events listed above are consumed.

---

## 4. Target architecture (Strategy A — Rust port of pi's design)

```
┌──────────────────────────────────────────────────────────┐
│ gui-next (Vite/React)                                     │
│   └─ ipc/ (typed call/on wrapper) ──► Tauri invoke ─┐     │
└──────────────────────────────────────────────────────┼─────┘
                                                       ▼
┌──────────────────────────────────────────────────────────┐
│ Tauri shell (thin) — NEW `src-tauri`                     │
│   - window, global shortcuts, notifications              │
│   - proxies commands to the agent server over IPC        │
└──────────────────────────────────────────────────────────┘
                       │ local protocol (CBOR or JSON)
                       ▼
┌──────────────────────────────────────────────────────────┐
│ vivianne-agent-server (Rust binary, spawned by shell)    │
│   - listener + connection handling  (port from pi-server)│
│   - agent core: state machine, reducer, events           │
│       (port from pi-agent harness)                       │
│   - tools: bash, edit-diff, read, write, image, subagents│
│   - model/provider layer  (port from pi-ai)              │
│   - sessions + JSONL storage  (port from pi session)     │
│   - subsystems: terminal PTY, MCP, LSP, voice, automations│
└──────────────────────────────────────────────────────────┘
```

**Why a separate server process (the key change):**
- The old backend ran the agent loop *inside* the Tauri app process with one giant `AppState`. pi's architecture instead runs a **headless server** that any client (TUI, GUI, CI) can attach to — sessions survive, the loop is decoupled from window lifecycle, and the loop can be tuned/restarted without touching the shell.
- The existing `orchestrator/` (spawn/list/status/rpc/rpc_stream) is already a proto-server — it validates this direction; the rewrite makes it real and fast.

**Module map (new `packages/gui/src-tauri/src`):**

| New module | Ported from | Key contents |
|---|---|---|
| `protocol/` | `pi-protocol` | versioned request/response enums, framing, (CBOR optional; JSON v1 for speed-of-delivery) |
| `server/` | `pi-server` | listener, connections, session registry, snapshots |
| `core/` (renamed `agent/`) | `pi-agent-core` harness | **state machine + reducer**, events, prompt templates, system prompt, compaction |
| `models/` (renamed `ai/`) | `pi-ai` | unified `Models` API, catalog, provider backends, credential pool, local endpoints |
| `tools/` | `pi harness/tools` | bash, edit-diff, read, write, image + existing Vivianne tools |
| `session/` | `pi session/jsonl` | state, context window mgmt, JSONL storage, repo/memory backends |
| `integrations/` | existing | terminal PTY, MCP, LSP, voice, automation, gesture, web preview, docker |
| `shell.rs` | existing `lib.rs` | thin Tauri entrypoint only |

---

## 5. Strategy decision gate

| | A: Rust port of pi design | B: pi TS server as sidecar |
|---|---|---|
| Core language | Rust (matches ask) | TypeScript (pi's own) |
| Effort | Very high (≈30–40k lines port) | Medium (vendor/adapt pi packages) |
| Speed | Fastest (native, no JS runtime) | Good (Node startup ~100–300ms) |
| Backend rewrite? | Yes — what was asked | No — pi is already the backend |
| Risk of corruption | Control via design | Inherits pi's maturity |
| GUI contract | Must reimplement | pi protocol already exists + `pi-client` |

**Recommendation:** A, but staged so B remains a fallback (see phases). If speed-to-working-backend outranks Rust purity, B is legitimately better. Ask the user to confirm before Phase 2.

---

## 6. Phased plan

### Phase 0 — Freeze the contract (1–2 days)
- [ ] Split §3a (verified commands) from §3b (unreferenced) and confirm per feature what survives.
- [ ] Audit gui-next components for real event consumption (listen/`on`) and write down the verified event contract — §3c is currently unverified.
- [ ] Export the frozen surface into a checked-in contract file (`packages/gui-next/src/ipc/contract.ts` with literal types).
- [ ] Write a **contract test**: a stub Rust backend (or mocked `invoke`) that asserts every `call()` name exists in both directions. This is the canary that catches drift during the rewrite.
- [ ] `git tag` a checkpoint of the current backend + frontend so we can diff behavior and roll back. (Note: the worktree has many uncommitted changes — checkpoint the tag on current state, don't commit other sessions' work.)
- **Gate:** `cargo check` on the backend, `npm run check` (frontend typecheck) green, contract test green.
- Deliverable: a testable spec the rewrite must satisfy.

### Phase 1 — Greenfield scaffold (2–3 days)
- [ ] Create new crate layout under `packages/gui/src-tauri` (or new `packages/agent-server/`) implementing only `protocol/` + `server/` skeleton + thin shell.
- [ ] Wire the Tauri shell to spawn the agent server on app start and proxy a handful of commands (auth, settings) end-to-end.
- [ ] Keep the old backend compiling in parallel (`--features legacy`) until cutover.
- **Gate:** `cargo build` + `cargo clippy -- -D warnings` + `cargo test` green; contract test green.
- Deliverable: shell boots, server spawns, `cmd_save_settings` round-trips.

### Phase 2 — Port the agent core (1–2 weeks, biggest risk)
- [ ] Port `pi-agent` harness **state machine + reducer + events** to Rust (the single most valuable piece: pi's loop is cleanly separated from I/O).
- [ ] Port prompt templates / system prompt / compaction (pi `compaction.ts` 848 lines → Rust).
- [ ] Port session state + context window + JSONL storage (pi `session/`).
- [ ] Port tools in order: read, write, edit-diff, bash (pi `harness/tools/`), then Vivianne-only tools (memory, skills, playbooks, MCP, subagents).
- [ ] Keep `gui-next` on the OLD backend during this phase; run both backends side-by-side in dev (different ports) and compare transcript outputs on identical prompts (regression harness).
- **Gate:** `cargo build` + `cargo clippy` + `cargo test` green each step; agent loop produces byte-comparable transcripts on a 10-prompt suite vs the old backend.

### Phase 3 — Port the model/provider layer (3–5 days)
- [ ] Port pi-ai's unified `Models` API + catalog + provider backends (Anthropic/OpenAI/Google/Bedrock/Mistral) to Rust, reusing existing `reqwest`/credential-pool code where sane.
- [ ] Keep local endpoints (llama) and the credential pool intact; unify their API shape with pi's.
- **Gate:** `cargo build` + `cargo test` green; same model/params produce equivalent completion deltas in a side-by-side test.

### Phase 4 — Integrations (3–5 days)
- [ ] Terminal PTY, MCP, LSP, voice (whisper), automation scheduler, gesture, web preview, docker — port only the modules that survived Phase 0 (§3b decision rule), module-by-module, deleting old code as each lands.
- [ ] Replace the orchestrator's `rpc_stream` with the new server protocol streaming.
- **Gate:** `cargo build` + `cargo test` green; all §3a commands functional on the new backend; contract test green.

### Phase 5 — Cutover & delete legacy (2–3 days)
- [ ] Point `gui-next` at the new backend (contract test green).
- [ ] Run the app end-to-end: chat, sessions, terminal, MCP, voice, automations.
- [ ] Delete `--features legacy` and the old agent/orchestrator/coding_agent modules.
- [ ] Bench startup + first-token latency vs old backend; record in `CHANGELOG.md`.
- **Gate:** `cargo build` + `cargo clippy` + `cargo test` + `npm run check` + contract test all green; manual end-to-end pass.

### Phase 6 — Optimize (ongoing)
- [ ] Profile hot paths (tokenization, tool exec, JSONL I/O), add streaming where pi does, optional CBOR upgrade, snapshot/resume, subagent fan-out.

---

## 7. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Porting 30k+ lines of agent logic is huge | Phase 2 in isolation; port pi's *clean* harness instead of untangling the old loop; regression suite gates each step |
| pi has no Rust code (discovery) | Strategy B fallback is explicitly viable; confirm early (§5) |
| Command-surface drift breaks gui-next | Phase 0 contract test is the canary; parallel backends until cutover |
| Compaction/session semantics differ | Port pi's compaction logic faithfully; side-by-side transcript diffs |
| Native deps (whisper, ONNX, PTY) complicate a separate server process | Server is same-machine child process; keep native crates in the server, not the shell |
| Scope creep from old features (camoufox, docker) | §3b decision rule: keep only features a shipped surface uses; confirm each with the user |

---

## 8. Immediate next steps (do this session)

1. Confirm the reference clone is visible: `scratch/pi-reference/`.
2. Decide **A vs B** (§5) — this gates Phase 1/2.
3. Kick off **Phase 0**: write the contract test + checkpoint tag.
4. Then Phase 1 scaffold.
