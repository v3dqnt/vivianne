# Plan: LSP uplift and a debugger for Vivianne, ported from oh-my-pi

Read first: `c4-context.md`, `c4-container.md`, `c4-component-lsp.md`, `c4-component-dap.md` in this folder; the evidence is in `docs/omp-lsp-map.md` and `docs/omp-dap-map.md`.

## Is it a copy?

No. omp's LSP and DAP are TypeScript on Bun. The only Rust involved is glob, IPC (used by the mux, which we are skipping), process spawning, and a profiler. So the work is a port from TypeScript to Rust, guided by the maps. What does carry over directly is Vivianne's own Rust: the strict JSON-RPC framing in `lsp/client.rs`, the in-process mock-peer test pattern in `agent/harness_mock.rs`, `own_group` / `TreeKillOnDrop` / `kill_tree`, `crate::proc::quiet`, `sandbox::filtered_env()`, the permission gate and `ask_permission`. omp's design and its caps and timeouts are what we reuse.

## Ground rules

- One phase at a time, tests first, a clean `cargo test --lib` before the next phase.
- Every new process goes through `crate::proc::quiet` (no console flashes) and is killed as a tree.
- A mock server and a mock adapter over in-memory pipes come first, so every phase is tested without installing anything.
- Nothing in a project's own files may name a command Vivianne runs until the user has trusted that project.
- Do not reproduce omp's defects listed in `c4-component-dap.md` and `c4-component-lsp.md`.

## Part A: LSP uplift (grow what exists)

**A0. Mock language server and characterization tests.** A scripted LSP peer over pipes (initialize, didOpen, publishDiagnostics, definition, rename, window requests). Pin the behaviour of the current client before changing it. Size: S.

**A1. Client robustness** (`lsp/client.rs`). Write queue with a per-write bound; `$/cancelRequest` on timeout or Stop; answers to `workspace/configuration` (dotted sections handled), `client/registerCapability`, `window/workDoneProgress/create`, `workspace/applyEdit`; a 3 minute negative cache for a failed start; a registry key of command, cwd, args and init options. Size: M.

**A2. Version-aware diagnostics** (`lsp/manager.rs`, `lsp/mod.rs`). Accept a report only for the document version that was sent; a 250 ms settle window for push-only servers; `textDocument/diagnostic` pull where the server supports it; keep "unreported" distinct from "clean". Size: M. This is the change that makes post-edit errors trustworthy.

**A3. Ledger and deferred results.** Dedupe by message identity (path, line and column prefix removed); wait 500 ms inline after a write, then hand the rest to a deferred task (12 s) that delivers a late result as a system notice in the thread, dropped if the file changed again (a per-file version counter). Cap at 50 messages. Size: M. Needs a design note on how a notice reaches a thread that is between turns.

**A4. Edits.** Apply workspace edits (text edits, document changes, file create, rename, delete) with a path check against the workspace and an atomic write through the file-safety layer; editorconfig then content sniffing for formatting options; optional format-on-write setting (off by default). Size: M.

**A5. Tool surface.** Add `type_definition`, `symbols`, `diagnostics` (folded into `lsp`), `status`, `capabilities` to the read tool; add `lsp_edit` with `rename`, `rename_file`, `code_actions`, `reload` and raw `request`, gated and refused in plan mode. Output caps as in `c4-component-lsp.md`. Size: M.

**A6. Servers and config.** Extend `server_candidates` to omp's list with root markers; a project-level `.vivianne/lsp.json` that is **ignored until the project is trusted**; extra server definitions shown in the approval card as the full command. Size: S to M.

**A7 (optional). Project check and linters.** A `check` action that runs the project's own checker by root marker; biome and swiftlint as CLI linters. Size: S each. **Not planned:** the mux.

## Part B: debugger (new module `src/dap/`)

**B0. Mock debug adapter.** A scripted DAP peer over pipes (initialize, launch, initialized event, setBreakpoints, configurationDone, stopped, stackTrace, scopes, variables, continue, terminated). Extract the `Content-Length` framer shared with LSP into one module. Size: S.

**B1. Config and adapter selection** (`dap/config.rs`). A table for debugpy, lldb-dap, gdb, delve, js-debug, netcoredbg and codelldb (corrected: check against the real adapters before trusting any argv); selection by file type, root marker and rank; PATH only for binaries; user overrides from a project file need trust. Size: S.

**B2. Client** (`dap/client.rs`). Stdio transport first. Requests with ids and timeouts, events, reverse requests. Dispose kills the tree. Windows: TCP dial-back instead of a unix socket. Size: M.

**B3. Session manager** (`dap/session.rs`). Keyed by Vivianne thread id. State: status, threads, stop (thread, frame, reason), breakpoints, bounded output, capabilities. Handshake in the **correct order**: `initialize`, `launch` or `attach`, wait for `initialized`, set the initial breakpoints, `configurationDone`. Handle `output` (by category), `stopped`, `continued`, `exited`, `terminated`, `breakpoint`, `thread`, `invalidated`. Waiting for a stop returns "still running" on timeout instead of failing. Size: L.

**B4. Tools** (`agent/tools/debug.rs`). Split by risk: `debug` (read: `stack_trace`, `threads`, `scopes`, `variables`, `output`, `sessions`, `modules`, `loaded_sources`, allowed in plan mode) and `debug_run` (exec: `launch`, `attach`, breakpoints, run control, `evaluate`, `write_memory`, `custom_request`, `terminate`, gated). The `launch` approval card shows the **full adapter command and arguments** as well as the program. Cap every result (variables paged, stack frames limited, dumps limited); add exception breakpoints, `setVariable`, `restart`, log points and hit conditions. Size: L.

**B5. Safety.**
- `runInTerminal` and `startDebugging` go through `ask_permission`; the process handle is kept and killed with the session.
- Environment: start from `filtered_env()`, honour `null` as unset, never pass the app's own secrets.
- No idle kill while a debuggee is running; a hook on app exit ends every adapter.
- `/tmp` paths and unix sockets replaced by the temp directory and TCP.
Size: M. Part of B3 and B4, listed separately so it is not skipped.

**B6. Child sessions and TCP.** js-debug style adapters (a second connection per child session), the stdout "listening" gate, and delve's dial-back. Size: L.

**B7. UI (later, separate).** A debug panel (threads, stack, variables, breakpoints) fed by events. The agent-only tool comes first and is useful alone. Size: L.

**B8 (optional). Offer both tools to harness agents** (Claude Code and others) through the MCP bridge built in ACP Phase 6, read tier only to start. Size: S.

## Order

A0, A1, A2, A3 (the diagnostics story), then B0 to B5 in parallel with A4 to A6, then B6, then B7. Start with A2: it improves every edit the agent makes today.

## Risks and decisions

- **Adapter reality.** Most adapter argv in omp's `defaults.json` are from reading, not from running. Each adapter needs one real run before it ships.
- **Trust model.** Decide once how a project becomes trusted (a prompt on first use, stored per folder) and reuse it for LSP config, DAP config and any other repo-defined command.
- **Deferred notices** need a delivery route into a thread; decide it before A3.
- **Debugging is exec-tier.** Launching, `evaluate` and memory writes run code. They are never allowed in plan mode, never auto-approved by bypass for the first launch of a new adapter command, and always show the exact command.
- **Out of scope:** the mux and lspmux, omp's self-diagnostics (`debug/`, the TUI debug app), and a visual debugger UI beyond B7.
