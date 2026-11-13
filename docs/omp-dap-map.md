# omp debugger subsystem (DAP): architecture map for a Rust/Tauri port

Source: `C:\Users\v3dqn\Work\Project Versa\oh-my-pi-breakdown` (read-only; nothing was run, built, or edited).
Confidence labels: **[read]** = read in code; **[by reading]** = behaviour inferred from code paths, not executed; **[unverified]** = external fact not shown by this repo.
Line numbers are from the clone as read.

---

## 0. Scope and coverage

**Read in full:**
- `packages/coding-agent/src/dap/`: `index.ts` (4 lines), `client.ts` (1060), `config.ts` (418), `session.ts` (1877), `types.ts` (615), `defaults.json` (212).
- `packages/coding-agent/src/tools/debug.ts` (986), the model-facing tool.
- `packages/coding-agent/src/prompts/tools/debug.md` (3 lines).
- `packages/coding-agent/src/debug/`: `index.ts` (603), `profiler.ts` (185), `remote-debugger.ts` (151), `report-bundle.ts` (445), `system-info.ts` (111).
- Tests: `test/debug/dap-config.test.ts` (340), `test/debug/dap-launch-failures.test.ts` (1086), `test/debug/dap-multi-session.test.ts` (423), `test/debug/profiler.test.ts`, `test/debug/raw-sse-report-bundle.test.ts`, `test/debug/report-bundle-logs.test.ts`, `test/debug/report-bundle-sessions.test.ts`, and `test/dap-config-precedence.test.ts` (this last one sits outside `test/debug/`).
- TUI debug app: `packages/tui/src/apps/debug/` in full: `log-viewer.ts` (1219), `raw-sse.ts` (499), `raw-sse-buffer.ts` (457), `protocol-probe.ts` (258), `viewer-frame.ts` (174), `terminal-info.ts` (115), `log-formatting.ts` (53).
- `packages/tui/src/tools/debug.ts` (239): the renderer for the debug tool card. It is imported by the tool and was not in the original list.
- `packages/coding-agent/src/tools/tool-result.ts` (115): the result builder the tool uses.
- `packages/coding-agent/src/jsonrpc/message-framing.ts` (199): the Content-Length framer used by the DAP client.
- `docs/tools/debug.md` (392): the only dedicated DAP doc.

**Read in part or grep only (not read in full):**
- `packages/utils/src/ptree.ts` (841): read only around the kill and detach branches (about lines 340-400, 742-800) plus greps. Process spawn, group handling, and the `exited` promise were not traced end to end.
- `packages/coding-agent/src/lsp/config.ts`: only the signatures of `parseConfigContent`, `hasRootMarkers`, and `resolveCommand` were confirmed.
- `packages/coding-agent/src/utils/config-file-sources.ts`: not read. The search order (`.omp/`, `.claude/`, `.codex/`, `.gemini/`, user dirs, plugin roots) comes from `docs/tools/debug.md` and is **unverified** against code.
- `packages/coding-agent/src/exec/non-interactive-env.ts`: first 40 lines only.
- `packages/coding-agent/src/tools/tool-timeouts.ts`: grep only (`debug: { default: 30, min: 5, max: 300 }`, line 18).
- `packages/coding-agent/src/tools/settings.ts`: only the `cfgDebugEnabled` block (lines 576-585).
- `packages/coding-agent/src/tools/index.ts`: only lines 56, 90, 606, 792.
- `packages/coding-agent/src/tools/output-meta.ts`: not read. The debug tool never calls its truncation setters.
- `crates/pi-natives/src/prof.rs`: only the signature at line 182 of `get_work_profile`.
- `packages/coding-agent/test/fixtures/malformed-jsonrpc-peer.ts`: exists, not read.
- `packages/utils/src/` helpers used by the DAP code, such as `untilAborted`, `materializeString`, `withTimeout`, `$which`, `logger`: not read.
- `docs/environment-variables.md`: grep hits only (`JS_DEBUG_DAP_SERVER`, `XDG_DATA_HOME`, and the OTEL isolation note about LSP/DAP/MCP children).
- `docs/bash-tool-runtime.md`, `docs/keybindings.md`, `docs/marketplace.md`, `docs/skills/authoring-marketplaces.md`: grep hits on "debugger" or "debug" only; these were not read.
- `packages/coding-agent/src/debug/index.ts` consumers: `selector-controller.ts` line 2600 (`showDebugSelector`) and the `/debug` slash command in `slash-commands/builtin-lifecycle.ts` lines 578-587, read only at those lines.

**Not run:** no tests, no adapters, no Bun. Every behavioural claim below is by reading unless labelled otherwise.

**Crates:** a grep over `crates/` (excluding `vendor/` and `target/`) found no DAP or debugger code. The only other matches were tokenizer data files. The one Rust touchpoint is `get_work_profile` (section 9).

**Naming:** the "28 dap ops" in the brief are the 28 values of the `debug` tool's `action` field. They map onto about 25 distinct DAP requests plus `initialize`, `launch`/`attach`, and `configurationDone`.

---

## 1. Context (C4 level 1)

**Who uses the debugger module:**
- **The model.** It calls the `debug` tool, a class `DebugTool` implementing `AgentTool` (`tools/debug.ts` line 543, `name = "debug"`). It is registered by `tools/index.ts` line 606 (`debug: DebugTool.createIf`) and gated by setting `debug.enabled` (default `true`, `tools/settings.ts` 576-585; also filtered at `tools/index.ts` line 792). It is `loadMode: "discoverable"` and `concurrency: "exclusive"` (`debug.ts` lines 579-580).
- **The human, through the TUI.** The `/debug` slash command (`slash-commands/builtin-lifecycle.ts` 578-587) calls `showDebugSelector` (`selector-controller.ts` 2600), which mounts `DebugSelectorComponent` (`debug/index.ts`). That menu has 13 routes (`DEBUG_MENU_ITEMS`, lines 41-67). None of them is model-callable. Section 7 covers them.
- **The agent harness.** It reads the approval tier that `DebugTool.approval` returns (`read` or `exec`, lines 545-549) and enforces the exclusive scheduling. How the harness applies those tiers was **not traced** (see section 8).

**External systems (debug adapters).** `dap/defaults.json` ships 14 adapters:

| id | command and args | languages | connectMode | notes |
|---|---|---|---|---|
| `gdb` | `gdb -i dap` | c, cpp, rust | stdio | extensionless fallback, preferred first |
| `lldb-dap` | `lldb-dap` | c, cpp, objc, swift, rust, zig | stdio | extensionless fallback, second |
| `codelldb` | `codelldb --port 0` | c, cpp, rust, zig | stdio (default) | see section 8: args look inconsistent with stdio |
| `debugpy` | `python -m debugpy.adapter` | python | stdio | "justMyCode": false; attach prefers it when `port` is given |
| `dlv` | `dlv dap` | go | socket | `acceptsDirectoryProgram: true`; mode `debug` or `exec` chosen per program |
| `js-debug-adapter` | resolved to `node dapDebugServer.js <port> 127.0.0.1` | javascript, typescript | tcp | special resolver (section 5) |
| `netcoredbg` | `netcoredbg --interpreter=vscode` | csharp, fsharp | stdio | uses `stopAtEntry` |
| `kotlin-debug-adapter` | `kotlin-debug-adapter` | kotlin | stdio | no stopOnEntry default |
| `rdbg` | `rdbg --open --command --` | ruby | stdio (default) | args look inconsistent with stdio, see section 8 |
| `php-debug-adapter` | `php-debug-adapter` | php | stdio | |
| `bash-debug-adapter` | `bash-debug-adapter` | bash, shell | stdio | `type: bashdb` |
| `dart-debug-adapter` | `dart debug_adapter` | dart | stdio | |
| `flutter-debug-adapter` | `dart debug_adapter --flutter-sdk-path ""` | dart | stdio | empty arg is dropped (section 8) |
| `elixir-ls-debugger` | `elixir-ls-debugger` | elixir | stdio | `type: mix_task` |

The adapter binaries are external and **[unverified]** against this repo: whether each default argv works is recall, not shown here.

Other external touchpoints: the local filesystem (`dap.json` / `.omp/dap.yaml` configs read from the working directory; reports; `/tmp`), the OS file manager or browser (`openPath`, UI only), the clipboard (UI only), and Bun's JavaScriptCore remote inspector (UI only, section 7).

---

## 2. Containers (C4 level 2)

| Container | Process | Transport | Notes |
|---|---|---|---|
| **omp host** | the Bun process running omp | in-process | Holds the module singleton `dapSessionManager` (`session.ts` line 1877). Only one root session at a time. |
| **Adapter, stdio** (default) | child spawned with `ptree.spawn(..., { stdin: "pipe", detached: true })` (`client.ts` 117-126) | DAP over the child's stdin/stdout, framed with `Content-Length` | Adapter env is `{...Bun.env, ...NON_INTERACTIVE_ENV}` (`client.ts` 117-120). |
| **Adapter, socket, Linux** | same spawn, with `--listen=unix:/tmp/dap-<name>-<ts>-<rand>.sock` appended (`client.ts` 250-251) | Unix domain socket via `Bun.connect({ unix })` | Polls `stat` until `isSocket()` (`waitForCondition`, lines 787-801) with a 10 s default. |
| **Adapter, socket, non-Linux** | same spawn, with `--client-addr=127.0.0.1:<port>` (`client.ts` 306) | omp listens on `127.0.0.1:0`; the adapter dials back | Used by `dlv` on macOS and Windows. |
| **Adapter, tcp** | spawn with `${port}` substituted into args (`client.ts` 182) | TCP to `127.0.0.1:<port>` | omp reserves a free port, releases it, and waits for the adapter's stdout to print that port (banner gate, `waitForTcpServerListening`, 895-923), then connects with 50 ms retries. This is how js-debug works. |
| **Child DAP session** (tcp only) | a second TCP connection to the same adapter server (`DapClient.connect`, `client.ts` 136-165) | TCP | Created when the adapter sends reverse `startDebugging`. The child is a `DapClient` with a stub `proc`. |
| **Debuggee via `runInTerminal`** | spawned by omp itself with `ptree.spawn(args.args, { detached: true })` (`session.ts` 1378-1387) | child stdout drained into the session output buffer (`drainTerminalStdout`, 234-244) | Adapter-requested. omp keeps no handle to the process (see section 8). |
| **Report files** | none (file I/O) | tar.gz via `writeArchive` (`report-bundle.ts` 182) | Under the reports dir. |
| **JSC inspector** | in-process Bun socket | raw loopback TCP | UI-only, process-wide, no stop operation (`remote-debugger.ts`). |

**Launch vs attach:**
- *Launch:* spawn adapter, `initialize`, then `launch` with `program`, `cwd`, `args`, plus `launchDefaults` and `extraLaunchArguments` (`session.ts` 323-329).
- *Attach:* spawn adapter, `initialize`, then `attach` with `pid`/`processId`, `port`, `host`, `cwd`, and `attachDefaults` (`session.ts` 400-406). With `attachDefaults.skipAttachRequest === true` the attach request is skipped (396-398). An explicit adapter may attach with no pid or port.
- omp never attaches to a pre-existing adapter daemon. Every root session starts its own adapter process.

---

## 3. Components (C4 level 3)

### 3a. `dap/` (TypeScript core)

- **`dap/index.ts`**: barrel. Re-exports `client`, `config`, `session`, `types`. Nothing imports `dap/` except `tools/debug.ts` and the tests.
- **`dap/types.ts`**: all DAP shapes. Key exports: `DapSessionStatus` (`"launching" | "configuring" | "stopped" | "running" | "terminated"`, line 4), `DapCapabilities` (line 96), `DapAdapterConfig` (482), `DapResolvedAdapter` (500), `DapSessionSummary` (563), `DapContinueOutcome` (591), `DapLaunchSessionOptions` (597), `DapAttachSessionOptions` (609), and about 60 request/response interfaces. Depends only on the `ptree` type from `@oh-my-pi/pi-utils`.
- **`dap/defaults.json`**: the 14 built-in adapters above.
- **`dap/config.ts`**: adapter discovery and selection. Exports `getAdapterConfigs(cwd)`, `resolveAdapter(name, cwd)`, `getAvailableAdapters(cwd)`, `selectLaunchAdapter(program, cwd, adapterName?, programKind)`, `selectAttachAdapter(cwd, adapterName?, port?)`, `resolveLaunchOverrides(adapter, program, programKind)`, and types `LaunchAdapterSelection`, `LaunchProgramKind`. Depends on `lsp/config.ts` (`hasRootMarkers`, `parseConfigContent`, `resolveCommand`), `utils/config-file-sources.ts` (`configFileSources`), and `@oh-my-pi/pi-utils` (`isRecord`, `logger`, `WhichCachePolicy`).
- **`dap/client.ts`**: one connection to one adapter. Static factories `DapClient.spawn` (line 106) and `DapClient.connect` (136). Methods `initialize`, `sendRequest` (432), `sendResponse` (502), `waitForEvent` (387), `onEvent`, `onAnyEvent`, `onReverseRequest`, `dispose` (580), `isAlive`. Exports the test hooks `connectSocket` (954) and `waitForTcpServerListening` (895). Depends on `jsonrpc/message-framing.ts`, `exec/non-interactive-env.ts`, `tools/tool-errors.ts` (`ToolAbortError`), and `@oh-my-pi/pi-utils` (`ptree`, `logger`, `isEnoent`).
- **`dap/session.ts`**: `DapSessionManager` (class at line 284, singleton at 1877). Owns the one-root-session state machine, breakpoint mirroring, stop tracking, the bounded output buffer, the child-session tree, the idle cleanup loop, and the reverse-request handlers. Exports `DapSessionManager`, `dapSessionManager`, `DapOutputSnapshot`. Depends on `client.ts`, `types.ts`, `exec/non-interactive-env.ts`, `@oh-my-pi/pi-utils` (`ptree`, `untilAborted`, `logger`), `node:timers/promises`, and `node:path`.
- **`dap/config.ts`** and **`dap/session.ts`** are the two files that encode most of the policy. The rest is transport.

### 3b. Tool and prompt

- **`tools/debug.ts`**: `DebugTool`. Schema (`debugSchema`, lines 99-138) built with `@oh-my-pi/omptype`'s `type`. The 28-value enum is `debugActionSchema` (69-98). `DEBUG_READONLY_ACTIONS` (57-68). `execute` (596-985) is one switch with 28 cases. Also holds the text formatters (173-456), the adapter-unavailable messages (464-490), launch-program classification and validation (492-512), and `examples` (564-578). Depends on `dap/*`, `tools/approval` (`truncateForPrompt`), `tools/path-utils`, `tools/tool-timeouts`, `tools/settings`, `tools/tool-result`, `@oh-my-pi/pi-tui/tools/debug` (`formatSessionSnapshot`, `formatLocation`), and `@oh-my-pi/pi-tui/render/*`.
- **`packages/tui/src/tools/debug.ts`**: the renderer for the tool card. `renderCall`, `renderResult` (preview truncated to `PREVIEW_LIMITS`), `describeCall`, `describeResult`. Also exports `formatSessionSnapshot` (lines 71-91) and `formatLocation` (63-68), which the tool uses for its text output. The model receives the full text; only the human preview is truncated (lines 199-210).
- **`prompts/tools/debug.md`**: three lines. Says to prefer `debug` over bash for program state, that only one active session is supported, and that directories need a directory-capable adapter such as `dlv`. Injected by `prompt.render` in the constructor (line 583).

### 3c. `debug/` (omp self-diagnostics; see section 7)

- **`debug/index.ts`**: `DebugSelectorComponent` (line 77) and `showDebugSelector` (600). Menu dispatch in `#handleSelection` (107-149). Depends on everything else in `debug/` and on `tui/src/apps/debug/*`.
- **`debug/profiler.ts`**: `startCpuProfile` (117-150) using `node:inspector/promises` with a 100 µs sampling interval (line 135); `collectMemoryStats` (168-185) using `bun:jsc` `heapStats` after `Bun.gc(true)`. Numeric counters only.
- **`debug/remote-debugger.ts`**: `startRemoteDebuggerServer` (105-113) wrapping `bun:jsc`'s `startRemoteDebugger` on `127.0.0.1`, idempotent, reserves a free port via `node:net` (50-66), and verifies with a loopback probe (1 s deadline, 50 ms interval). No stop.
- **`debug/report-bundle.ts`**: `createReportBundle` (91-185) writes `omp-report-<ts>.tar.gz`. Also `collectSameDayLogs` (246-311), `createDebugLogSource` (315-374), `getArtifactCacheStats` (377-415), and `clearArtifactCache` (418-445).
- **`debug/system-info.ts`**: `collectSystemInfo` (42-77), `formatSystemInfo` (80-98), `sanitizeEnv` (101-111). `sanitizeEnv` redacts values whose *name* matches `/key|secret|token|pass|auth|credential|api|private/i`.

### 3d. `tui/src/apps/debug/` (TUI viewers)

- **`log-viewer.ts`** (1219 lines): `DebugLogViewerModel` (line 165; filter, pid filter, load-older paging, range selection, expand/collapse, copy payload) and `DebugLogViewerComponent` (643; a `picker` sheet for the native UI plus a row-based fallback). Caps: `MAX_LOG_ENTRIES = 50_000` (line 36), `INITIAL_LOG_CHUNK = 50`, `LOAD_OLDER_CHUNK = 50`.
- **`raw-sse-buffer.ts`** (457): `RawSseDebugBuffer` (line 254), a bounded capture of provider responses and SSE events. Caps at lines 4-6: 1000 events, 512 000 chars total, 64 000 chars per event. Over-budget events first compact tool `parameters`/`input_schema`/`description` to 200 chars (`compactToolLines`, 125-140), then head+tail trim with `: omp-debug-elided` markers (`headTailTrim`, 146-188), then a `: omp-debug-truncated` marker (198-216). `resolveRawSseDebugBuffer` (440-457) attaches a buffer to an owner under a private symbol.
- **`raw-sse.ts`** (499): `RawSseViewerComponent` (line 79), the live viewer. Subscribes to the buffer. Pretty-prints `data:` JSON wider than 100 chars (`expandPrettyDataLines`, 35-61).
- **`viewer-frame.ts`** (174): `DebugViewerFrame`, the shared chrome and scroll adapter for the fullscreen viewers.
- **`protocol-probe.ts`** (258): terminal protocol test panel. Generates a deterministic RGB gradient PNG in process (`encodeRgbPng`, 47-69) with `zlib.deflateSync` and CRC via `Bun.hash.crc32`.
- **`terminal-info.ts`** (115): `collectTerminalState` / `formatTerminalState`.
- **`log-formatting.ts`** (53): helpers for expanded log lines and timestamp/pid parsing.

### 3e. Framing

- **`jsonrpc/message-framing.ts`**: `encodeMessageFrame` (35-43), `MessageFramer` (78-198) with `push`, `drain(onResync)`, `remainder`. Shared by LSP and DAP. Limits: header block at most 16 KiB (`MAX_HEADER_BYTES`, line 17); body at most 256 MiB (`MAX_CONTENT_BYTES`, line 18). Behaviour is described in section 5.

---

## 4. The model-facing tool surface

**Common arguments (all actions):**
- `timeout?` (seconds). `clampTimeout("debug", ...)` applies the optional positive `tools.maxTimeout` cap, then the tool's 5-300 range, default 30 (`tool-timeouts.ts` line 18; `tools/debug.ts` 597). The same value is passed to the manager as `timeoutSec * 1000`, and each request also gets an `AbortSignal.timeout` (598-599).
- The tool's `combinedSignal` (`AbortSignal.any([signal, timeoutSignal])`) is passed into every manager call.
- Every result is `toolResult(details).text(...).done()`. `done()` never truncates. The `meta` truncation setters (`output-meta.ts`) are not called by this tool (`tools/tool-result.ts` 23-26, 97-110).

**Approval tier** (`DebugTool.approval`, lines 545-549): `read` for `output`, `threads`, `stack_trace`, `scopes`, `variables`, `disassemble`, `read_memory`, `loaded_sources`, `modules`, `sessions`; `exec` for everything else. How the harness uses `read` and `exec` was not traced.

| # | action | required args | optional args | DAP request(s) | capability gate | tier | returns |
|---|---|---|---|---|---|---|---|
| 1 | `launch` | `program` | `args[]`, `cwd`, `adapter` | `initialize`, `launch`, `configurationDone` | none | exec | session snapshot text; `details.adapter` |
| 2 | `attach` | `pid` or `port` (or an explicit `adapter`) | `cwd`, `adapter`, `host` | `initialize`, `attach`, `configurationDone` | none | exec | session snapshot text |
| 3 | `set_breakpoint` | `file`+`line`, or `function` | `condition` | `setBreakpoints` / `setFunctionBreakpoints` (full list resent) | none | exec | `Breakpoints for <path>:` or function list with verified/pending |
| 4 | `remove_breakpoint` | `file`+`line`, or `function` | none | same, with remaining list | none | exec | remaining list |
| 5 | `set_instruction_breakpoint` | `instruction_reference` | `offset`, `condition`, `hit_condition` | `setInstructionBreakpoints` | `supportsInstructionBreakpoints` | exec | instruction list |
| 6 | `remove_instruction_breakpoint` | `instruction_reference` | `offset` | same | `supportsInstructionBreakpoints` | exec | remaining list |
| 7 | `data_breakpoint_info` | `name` | `variable_ref` or `scope_id`, `frame_id` | `dataBreakpointInfo` | `supportsDataBreakpoints` | exec | description, dataId, access types |
| 8 | `set_data_breakpoint` | `data_id` | `access_type`, `condition`, `hit_condition` | `setDataBreakpoints` | `supportsDataBreakpoints` | exec | data breakpoint list |
| 9 | `remove_data_breakpoint` | `data_id` | none | `setDataBreakpoints` (remaining) | `supportsDataBreakpoints` | exec | remaining list |
| 10 | `continue` | none | none | `continue` | none | exec | "Continue stopped at <loc>." / terminated / "Program is running." / timed-out text |
| 11 | `step_over` | none | none | `next` | none | exec | "Step over stopped at ..." etc. |
| 12 | `step_in` | none | none | `stepIn` | none | exec | "Step in ..." |
| 13 | `step_out` | none | none | `stepOut` | none | exec | "Step out ..." |
| 14 | `pause` | none | none | `pause` | none | exec | snapshot + "Program paused." (always, see section 8) |
| 15 | `evaluate` | `expression` | `context` (default `repl`; not validated), `frame_id` | `evaluate` | none | exec | `Result:`, `Type:`, `Variables ref:` |
| 16 | `stack_trace` | none | `levels` | `stackTrace` | none | read | `- #id name @ path:line:col` per frame |
| 17 | `threads` | none | none | `threads`, fanned out over the live tree | none | read | `- id: name` per thread |
| 18 | `scopes` | none | `frame_id` (defaults to current stop) | `scopes` | none | read | `- name: ref=N, expensive=yes/no, hint=` |
| 19 | `variables` | `variable_ref` or `scope_id` | none (no paging exposed) | `variables` | none | read | `- name = value (type) [ref=N]` |
| 20 | `disassemble` | `instruction_count` | `memory_reference` (else current IP), `instruction_offset`, `offset`, `resolve_symbols` | `disassemble` | `supportsDisassembleRequest` | read | aligned address, bytes, instruction, `<symbol>`, `[loc]` |
| 21 | `read_memory` | `memory_reference`, `count` | `offset` | `readMemory` | `supportsReadMemoryRequest` | read | hex dump, 16 bytes/row, ASCII column, unreadable count |
| 22 | `write_memory` | `memory_reference`, `data` (base64) | `offset`, `allow_partial` | `writeMemory` | `supportsWriteMemoryRequest` | exec | "Memory write completed." + bytes/offset |
| 23 | `modules` | none | `start_module`, `module_count` | `modules` | `supportsModulesRequest` | read | table: ID, Name, Path, Symbols, Range |
| 24 | `loaded_sources` | none | none | `loadedSources` | `supportsLoadedSourcesRequest` | read | `- path [ref=N]` |
| 25 | `custom_request` | `command` | `arguments` (object) | any | none | exec | `<command> response:` + pretty JSON |
| 26 | `output` | none | none | none (reads buffer) | none | read | buffered output text, or "(no output captured)" |
| 27 | `terminate` | none | none | `terminate` (if capable), `disconnect` with `terminateDebuggee: true`, per child then root | none | exec | snapshot + "Debug session terminated."; or "No debug session to terminate." |
| 28 | `sessions` | none | none | none | none | read | per session: id, status, adapter, cwd, program, location, reason |

**Output format.** All results are plain text. The `details` object (`DebugExecutionDetails`, lines 143-171) carries the structured data for the UI (`snapshot`, `stackFrames`, `variables`, and so on). The model gets only the text.

**Output limits (what the code does):**
- `output` returns the whole session output buffer, up to 128 KiB (`MAX_OUTPUT_BYTES`), because it calls `getOutput()` with no limit (`tools/debug.ts` 963-967; `session.ts` 1116-1128).
- `evaluate`, `variables`, `stack_trace`, `threads`, `modules`, `loaded_sources`, `custom_request`, and the disassembly and memory dumps have **no cap in the tool**. The text is whatever the adapter returned.
- `stack_trace` honours `levels` only when the caller passes it.
- `variables` cannot page: `start` and `count` exist in `types.ts` (lines 271-277) but the tool and manager never send them.
- The TUI preview is truncated (`tui/src/tools/debug.ts` 199-210) but the model is not.
- Whether the agent loop applies a global tool-output cap was **not traced**.

**Gaps in the surface (checked by grep over `dap/` and `tools/debug.ts`):**
- No exception breakpoints (`setExceptionBreakpoints`) and no exception info. `supportsExceptionInfoRequest` appears only in `types.ts` line 112.
- No `setVariable`, `setExpression`, `restart`, `restartFrame`, `goto`, `gotoTargets`, `completions`, or `exceptionInfo` request. Their capability flags appear only in `types.ts` (lines 103-112). Reach them only through `custom_request`.
- No `granularity` or `singleThread` on stepping (`types.ts` 210-214 has the fields; `session.ts` never sends them).
- No `logMessage` (log points) and no `hitCondition` for source breakpoints. `set_breakpoint` passes only `condition` (`tools/debug.ts` 671-676; `session.ts` 524-554).
- No `variables` paging; no `filter` argument.
- No progress reporting (`supportsProgressReporting` is not sent).

---

## 5. Session lifecycle

### 5a. Adapter selection and configuration

1. **Defaults.** `getDefaults()` (`config.ts` 53-64) normalises `defaults.json` through `normalizeAdapterConfig` (33-48). Invalid entries (no `command`) are dropped.
2. **Overrides.** `loadAdapterConfigs(cwd)` (100-108) walks `configFileSources("dap", cwd, adaptConfig).reverse()`, merging each file over the previous with `mergeAdapters` (66-98). Field-level merge for `launchDefaults` and `attachDefaults` (lines 77-87). Invalid overrides keep the previous config and log a warning. Accepted shapes: `{ "adapters": {...} }` or a top-level map (`normalizeConfig`, 18-22). Per the docs, file names are `dap.json`, `.dap.json`, `dap.yaml`, `.dap.yaml`, `dap.yml`, `.dap.yml`, found under the cwd, project config dirs, user dirs, plugin roots, and the home root. **[unverified]**: the search order is from `docs/tools/debug.md`, not from `config-file-sources.ts`, which was not read.
3. **Command resolution.** `resolveAdapterFromConfig` (173-205). A relative command (`./x`) resolves against the cwd (114-125). A bare command resolves via `resolveCommand` with `localRoots` (see section 8, this is a safety point). Returns `null` if the command is not found, which makes the adapter "unavailable" rather than "absent".
4. **js-debug special case.** `resolveDefaultJsDebugAdapter` (141-171) applies only when the adapter is `js-debug-adapter` and its command is still `js-debug-adapter`. It looks for `dapDebugServer.js` in this order: the `JS_DEBUG_DAP_SERVER` env var, `$XDG_DATA_HOME/nvim/mason/...`, `~/.local/opt/js-debug/src/...` (127-139). It runs it under `node` if found on PATH, else under `process.execPath` (Bun). It returns a tcp-mode adapter with hardcoded args `[serverPath, "${port}", "127.0.0.1"]`. **It ignores any user `args` override** (see section 8).
5. **Launch selection** (`selectLaunchAdapter` → `selectAutomaticLaunchAdapter`, 264-324):
   - An explicit `adapter` name wins (357-372).
   - If the program has an extension, adapters whose `fileTypes` include it are candidates. Ties are broken by root-marker ancestry, then by the rank list `EXTENSIONLESS_DEBUGGER_ORDER = ["gdb", "lldb-dap"]` (line 10), then alphabetically (`sortAdaptersForLaunch`, 326-354). If candidates exist but none resolve, the first one is reported as unavailable, rather than silently picking another debugger (284-286).
   - Extensionless programs consider only the root-marker matches plus `gdb` and `lldb-dap`.
   - Package directories prefer `acceptsDirectoryProgram` adapters (306-318).
   - `dlv` overrides its `mode` to `debug` for directories and `.go` files, and `exec` for other files (`resolveLaunchOverrides`, 403-418).
6. **Attach selection** (`selectAttachAdapter`, 374-388): explicit name first; then, if `port` is given, `debugpy`; then `gdb`, then `lldb-dap`; then the first available adapter.

### 5b. Start handshake (launch and attach)

Order in `DapSessionManager.launch` (`session.ts` 308-379); attach is the same with `attach` in place of `launch` (381-446):

1. `#ensureLaunchSlot()` (1324-1333). Reaps dead or terminated sessions. Throws `Debug session <id> is still active. Terminate it before launching another.` if a live root exists.
2. `DapClient.spawn({ adapter, cwd })` (314). Spawns per `connectMode` (section 2).
3. `#registerSession()` (1335-1462). Installs reverse handlers (`runInTerminal` 1370-1392, `startDebugging` 1393-1406), event handlers (1407-1437), and a 5 s heartbeat that marks the session terminated if the client is dead (1449-1454). Sets the session active if no live stopped session holds focus (1446-1448).
4. `client.initialize(...)` (317). Arguments (`#buildInitializeArguments`, 1464-1479): `clientID: "omp"`, `adapterID`, `linesStartAt1`, `columnsStartAt1`, `pathFormat: "path"`, `supportsRunInTerminalRequest`, `supportsStartDebuggingRequest`, `supportsMemoryReferences`, `supportsVariableType`, `supportsInvalidatedEvent`. The reply is cached as `capabilities`. `needsConfigurationDone = supportsConfigurationDoneRequest === true` (322).
5. `launchArguments = {...adapter.launchDefaults, ...extraLaunchArguments, program, cwd, args?}` (323-329). Note that `launchDefaults.args` survives when the caller passes no `args` (test: `dap-launch-failures.test.ts` 212-225).
6. Subscribe to the stop outcome **before** sending `launch` (332-337). The comment says this is so that `stopOnEntry` events are not missed.
7. Send `launch` without awaiting it (341-348), tracked by `trackDapStartRequest`.
8. `#completeConfigurationHandshake` (1486-1519):
   - Wait for the `initialized` event (or the timeout, which falls through silently at 1502-1509).
   - If the session is a **child**, bind the root's breakpoints first (1511-1513).
   - Send `configurationDone` (1514).
   - Set `configurationDoneSent`, and move `configuring` to `running`.
9. Await the `launch` response (354). If either the `launch` or the `configurationDone` request fails, `throwPreferredDapStartError` (161-171) waits up to 50 ms for the start request's own error and prefers it. Errors are combined with `combineDapStartErrors` (150-159).
10. Wait up to 5 s for an initial stop (`STOP_CAPTURE_TIMEOUT_MS`). If stopped, fetch the top frame (`#fetchTopFrame`, 1547-1564). Return a summary either way.
11. On any failure, `#disposeSession` (1859-1874) and `mapDebugpyMissingModule` (181-185) turn a `No module named debugpy` error into `adapter 'debugpy' is not available: install with 'pip install debugpy'`.

**Configuration-done ordering, as the code actually runs it [by reading]:**
- **Root session:** `initialized` → `configurationDone` → `launch` response. Breakpoints are *not* set before `configurationDone`. The agent can only add breakpoints after `launch` returns, because every breakpoint call needs an active session (`#touchActiveSession`, 1755-1762).
- **Child session:** `initialized` → bind root breakpoints → `configurationDone` → `launch` response (lines 1511-1514). This is the canonical DAP order, and it is the only place it happens.
- **Lazy path:** `#ensureConfigurationDone` (1684-1697) sends `configurationDone` before any later request if the handshake did not send it.
- **Consequence:** adapters that start running right after `configurationDone` (where no stop-on-entry is set) may run past breakpoints the agent sets later. The launch defaults in `defaults.json` set `stopOnEntry` for gdb, lldb-dap, codelldb, debugpy, dlv, js-debug, php, dart, and elixir. They do not set it for kotlin, rdbg, bash, and flutter. For those four the program runs before the agent can set a breakpoint.
- The `set_breakpoint` hint text (`tui/src/tools/debug.ts` 87) says "set breakpoints, then continue", which assumes a stop on entry.

### 5c. Breakpoint synchronisation

- Breakpoints are stored on the **root** session (`getRootSession`, 1789-1797). Every mutation goes through `#syncBreakpointTree` (467-522):
  1. Take the live tree (`#getTreeSessions`, 1799-1812), filtering out dead or terminated sessions.
  2. Mark each session's copy as `verified: false` (prepare callback).
  3. Serialise on the origin session with `#serializeBreakpointMutation` (454-466), so two mutations cannot drop each other's breakpoints. The DAP request is sent with `#sendRequestWithConfig`, so `configurationDone` goes first if needed.
  4. Apply the adapter's response to the origin, then send the same full list to each child in parallel. Child failures are logged, not thrown (513-519).
- DAP's `setBreakpoints` replaces the whole list for a source. The manager therefore always resends the full list for that file (537-543, 569-575).
- Verification is taken from the `setBreakpoints` **response only**, matched to the request by index (`#mapSourceBreakpoints`, 1699-1710). The `breakpoint` event is not handled, so breakpoints that an adapter verifies later (lazy loading) stay `pending` in the cache.
- Paths are normalised with `path.resolve` (`normalizePath`, 187-189).

### 5d. Stop, continue, and waiting for the agent

**Event handling** (all in `#registerSession`):

| DAP event | effect on cached state |
|---|---|
| `output` | `truncateOutput` appends `body.output` to the 128 KiB buffer (407-409). Category is ignored. |
| `initialized` | `initializedSeen = true`; `launching` becomes `configuring` (1410-1415). |
| `stopped` | `status = "stopped"`; `stop = {threadId, reason, description, text}`; `lastStackFrames = []`; this session becomes active; resolve tree waiters (1416-1420, `#handleStoppedEvent` 1521-1530). **Does not set `frameId`.** |
| `continued` | `status = "running"`; `stop = {threadId}`; frames cleared (1421-1426). |
| `exited` | `exitCode` recorded; `status = "terminated"`; reactivate a live tree member; resolve tree waiters (1427-1432). |
| `terminated` | `status = "terminated"`; reactivate; resolve (1433-1437). |
| anything else | not handled. `invalidated`, `thread`, `breakpoint`, `module`, `loadedSource`, `capabilities`, `process`, and progress events are all ignored. |

**Waiting for a stop** (`#prepareStopOutcome` 1583-1616, `#awaitStopOutcome` 1621-1653):
- Before sending `continue` or a step, the manager resets `stop` and `lastStackFrames`, sets `status = "running"`, and registers a **tree-wide waiter** keyed by the root session id. That waiter resolves on any stopped, exited, or terminated event in the tree, or rejects on the timeout or the abort signal.
- `#awaitStopOutcome` then fetches the top frame (up to 5 s) if the active session is stopped, and returns `{ state, timedOut }`.
- **Blocking model:** the tool call blocks until the event arrives or the timeout (`timeoutSec`) fires. There is no polling and no push to the model. If the program stops after a timed-out `continue`, the next tool call reads the updated cache, but nothing notifies the model.

**The timed-out branch is probably not reachable from the tool [by reading]:**
- `docs/tools/debug.md` (lines 354, 309) says a timed-out `continue` or step returns `timedOut: true` and `state: "running"` instead of throwing.
- In code, the tool passes `combinedSignal` (which includes `AbortSignal.timeout(timeoutSec * 1000)`, `tools/debug.ts` 598-599) and the same duration as `timeoutMs`. The tool's timer starts first. When it fires, `combinedSignal` aborts, the waiter rejects with the signal's reason (`session.ts` 1608-1611), and `#awaitStopOutcome` takes the `catch` branch. That branch rethrows when `signal?.aborted` (line 1643). The tool then throws instead of returning `timedOut`.
- The `timedOut: true` return is reached only when the manager's own timer fires first, or when the manager is called without a signal. The unit tests call the manager directly with `undefined` signals, so they would not show this.
- **Status: unverified by running.** Treat the documented non-fatal timeout as unconfirmed. A port should decide the intended behaviour and test it through the tool.

**`pause`** (950-973) sends `pause`, waits on a `stopped` waiter, and swallows any timeout or abort (`try/catch` at 965-971). It then returns the current summary. The tool always prints `Program paused.` (`tools/debug.ts` 815-817), even when no stop arrived. The docs also say `pause` "returns a stopped snapshot", which is wrong when the wait times out.

### 5e. Multi-session handling (js-debug and similar)

- An adapter in `tcp` mode can send reverse `startDebugging`. The handler calls `#startChildSession` (1192-1238). It requires `connectMode === "tcp"` and a known `port`, opens a second TCP connection to the same server (`DapClient.connect`), runs the same handshake (section 5b), and sends `{...configuration, cwd}` as `launch` or `attach` (1224-1225).
- Child sessions register with `parentSessionId` and are added to the parent's `childSessionIds`.
- The reverse handlers are installed on every session, so children can spawn grandchildren recursively.
- `threads()` (987-1035) fans out to every live tree member and merges the results. Duplicate `(session, id)` pairs are collapsed, but identical ids from different sessions are kept (the test "preserves per-session threads..." confirms this, `dap-multi-session.test.ts` 336-376).
- **Focus** only moves on a stop (1446-1448 and 1416-1419). A late child that attaches does not steal focus from a stopped sibling. `#reactivateAfterTermination` (1837-1848) moves focus to a stopped member, else a child, else any live member.
- `#ensureLaunchSlot` counts only **root** sessions, so children do not block a new launch.
- Breakpoint and stop operations use the root's tree, but `stop` and `frameId` are per active session.

### 5f. Teardown and crash

- **`terminate`** (1130-1138) and `#terminateSessionTree` (1140-1160): children first (recursively), then `terminate` if the adapter advertised it, then `disconnect` with `terminateDebuggee: true`. Errors are swallowed. Then `#disposeSession` runs in `finally`.
- **`#disposeSession`** (1859-1874): removes the session and its children, repairs parent links, reactivates the parent if this was active, then calls `client.dispose()` (1873). That method marks the client disposed, rejects pending requests with `DAP adapter <name> disposed`, ends the socket, calls `proc.kill()`, and awaits `proc.exited` (`client.ts` 580-598).
- **Idle cleanup** (`#runCleanupLoop` 1168-1176, `#cleanupIdleSessions` 1178-1190): every 30 s, dispose any session that is terminated, dead, or idle for more than 10 min. **Disposal does not send `disconnect`**; it just kills the adapter. The session's `lastUsedAt` is refreshed only by manager calls (`#touchActiveSession` and `#sendRequestWithConfig`), so a debuggee that runs for more than 10 min without a tool call is killed silently.
- **Adapter crash:** `proc.exited` triggers `#handleProcessExit` (`client.ts` 744-755), which fails pending requests with `DAP adapter exited (code N): <stderr tail>` (`peekStderr`). The session is marked terminated by the `exited.finally` handler (1455-1460).
- **Framing failure** (invalid `Content-Length`, oversize header or body): the framer fails, pending work is rejected, and the client disposes itself (`client.ts` 660-667). Behaviour is by reading; tested in `dap-launch-failures.test.ts` lines 33-67, which expects a rejection matching `Content-Length.*limit`.
- **Wedged stdin:** each write is bounded by `WRITE_MESSAGE_TIMEOUT_MS = 30_000` (`client.ts` 45). On timeout or exit the client disposes (521-570). A synchronous write or flush error also disposes it.
- **Process exit of omp:** no hook. `dapSessionManager` is referenced only by `tools/debug.ts`. There is no `process.on("exit")` for DAP in `coding-agent/src` (grep). `ptree` also registers no exit hook (grep of `ptree.ts`). **[by reading]**: adapters are spawned `detached: true` (their own process group) and are not killed when omp exits, so a crash leaves them running until their stdin closes. This is unverified.

---

## 6. State model

**Per session** (`DapSession`, `session.ts` 66-99):

| field | meaning | lifetime and invalidation |
|---|---|---|
| `id` | `debug-<n>` | monotonic counter |
| `adapter`, `cwd`, `program`, `port` | fixed at creation | |
| `client` | the `DapClient` | disposed with the session |
| `status` | `launching`, `configuring`, `stopped`, `running`, `terminated` | driven by events, the heartbeat, and process exit |
| `launchedAt`, `lastUsedAt` | timestamps | `lastUsedAt` feeds the 10 min idle cleanup |
| `breakpoints` | `Map<path, DapBreakpointRecord[]>` | on root; mirrored to children on sync |
| `functionBreakpoints`, `instructionBreakpoints`, `dataBreakpoints` | arrays | on root; mirrored to children (`#applyRootBreakpointsToSession`, 1240-1322); the child copies are replaced on each sync |
| `breakpointMutationQueue` | promise chain | serialises mutations |
| `outputChunks`, `outputBytes`, `outputBufferedBytes`, `outputTruncated` | bounded output | 128 KiB; oldest chunks dropped; `outputTruncated` set |
| `stop` | `{threadId, frameId, reason, description, text, frameName, instructionPointerReference, source, line, column}` | reset on `continued`, on each `stopped`, and before each step or continue. `frameId` is set only by `#fetchTopFrame` or `stackTrace` |
| `threads` | last known thread list | refreshed by `threads()` and `#resolveThreadId`; never invalidated on `thread` events (none are handled) |
| `lastStackFrames` | last frames | cleared on stop, continue, step |
| `capabilities` | from `initialize` | per session |
| `initializedSeen`, `needsConfigurationDone`, `configurationDoneSent` | handshake flags | |
| `parentSessionId`, `childSessionIds` | tree links | |
| `exitCode` | from `exited` | |

**Manager:** `#sessions` (map), `#activeSessionId` (the global focus), `#treeOutcomeWaiters` (set), `#nextId`. It is a **process-wide singleton**, not scoped per agent session or per thread (see section 9).

**Stale references, as handled [by reading]:**
- **Frame ids:** `stop.frameId` is cleared on every new stop and on continue or step. `scopes` and `evaluate` fall back to it. After `pause`, `frameId` is **not** set (no `#fetchTopFrame` in `pause`, lines 950-973), so `scopes` without `frame_id` throws `No active stack frame. Run stack_trace first or supply frame_id.` (1065-1067) and `evaluate` silently runs without a frame (1101-1112).
- **Variables references:** not tracked anywhere. The model supplies `variable_ref` or `scope_id`, and the manager forwards it. An expired reference is the adapter's problem; the error text comes back as a tool error.
- **Memory references and `dataId`:** not tracked. A `dataId` persists across continues even though the adapter may invalidate it.
- **Thread ids:** `#resolveThreadId` (1655-1669) uses the stop's `threadId`, then `session.threads[0]` (possibly stale after the thread exits), then queries `threads`. A stale cached thread id is used without revalidation.
- **Breakpoint ids:** taken from the last response. Not refreshed by `breakpoint` events.
- **Source-less frames:** `formatStackFrames` (`tools/debug.ts` 208-210) prints `<unknown>:line:col` when `source.path` is absent, even if `source.name` is present.

---

## 7. The `debug/` folder, and the TUI debug app

**All of `debug/` and `tui/src/apps/debug/` is about omp's own diagnostics, not about user programs.** The user-program debugger is `dap/` plus `tools/debug.ts`. The two are linked only through the selector menu and the raw SSE capture buffer.

| route (menu value) | what it does | about omp or the user program |
|---|---|---|
| `open-artifacts` | opens the session's artifact dir in the OS file manager | omp |
| `performance` | starts a CPU profile (`startCpuProfile`), waits for Enter or Escape, stops it, adds a 30 s work profile, writes a report bundle | omp |
| `work` | writes `/tmp/work-profile-<ms>.svg` and opens it | omp |
| `dump` | writes a report bundle now | omp |
| `memory` | `Bun.gc(true)` then numeric memory stats, writes a report bundle | omp |
| `logs` | opens `DebugLogViewerComponent` over today's log plus older daily logs | omp |
| `system` | prints OS, arch, CPU, memory, versions, cwd, shell, terminal | omp |
| `terminal` | prints terminal capability state | omp |
| `protocols` | fires a test desktop notification, then renders a probe panel | omp (terminal) |
| `raw-sse` | opens the live raw provider SSE viewer | omp (provider traffic) |
| `remote-debugger` | starts Bun's JavaScriptCore inspector on `127.0.0.1` and a reserved port; no stop | omp (JS runtime) |
| `transcript` | exports the visible TUI transcript | omp |
| `clear-cache` | after confirmation, deletes artifact dirs older than 30 days | omp |

**Per file:**
- **`profiler.ts`**: CPU profile as CDP JSON plus a markdown summary of the top 30 functions by self time (`formatProfileAsMarkdown`, 42-111). Memory stats are numeric only, by design, so no credentials are captured (comment at line 163-166).
- **`remote-debugger.ts`**: exposes a process-wide Bun inspector socket. Experimental per Bun's own docs (cited in the file). Not a stoppable server.
- **`report-bundle.ts`**: `.tar.gz` with `session.jsonl`, the session's artifact subtree (recursive, including nested subagent transcripts), `logs.txt` (tail of every same-day PID log, 1000 lines each, 2 MB tail cap), `session-logs.txt` (every same-day record whose `sessionId` is this session or one of its subagents, last 5000), `system.json`, `env.json` (names matched by `sanitizeEnv`), `config.json`, `raw-sse.txt`, and the profile or memory or work files when present. Binary artifacts are read as text, so they are not preserved byte for byte (docs line 389). Sensitive content is possible; the UI warns (`debug/index.ts` lines 322-324).
- **`system-info.ts`**: as in section 3c. `sanitizeEnv` redacts by key name only, not by value. The docs say so (line 273).
- **`tui/apps/debug/raw-sse-buffer.ts`** is a capture buffer for provider HTTP/SSE traffic, not for DAP. The DAP adapter's own output does not flow through it.

**Tests for this folder:** `profiler.test.ts` (1 test: the `setFlagsFromString` regression on Bun); `raw-sse-report-bundle.test.ts` (1: the raw SSE text, including the dropped-record marker, survives into `raw-sse.txt`); `report-bundle-logs.test.ts` (4: all same-day PID logs collected; the batching transport's buffered records included; session records gathered past the tail window; the 2 MB tail cap respected); `report-bundle-sessions.test.ts` (2: privacy of numeric memory stats with a planted credential in heap counters; the bundle includes only the session's own subtree).

---

## 8. Safety and performance

**Gating and approval**
- The model call is classified by action only (`DebugTool.approval`). `launch` and `attach` are `exec`. The approval details show `Action:` and `Program:` (`tools/debug.ts` 550-557). They do **not** show the adapter command or args. How the harness presents or enforces this was not traced.
- The debug tool is off when `debug.enabled` is false (default `true`).

**Repo-controlled execution (important for a port) [by reading]:**
- Adapter definitions are read from `dap.json`, `.omp/dap.yaml`, and related files in the cwd and its project config dirs, with no trust check in `config.ts`. A cloned repo can therefore define an adapter whose `command` is any executable.
- For **bare** command names, `resolveAdapterForLaunch` passes `localRoots = [rootDir, cwd]` (`config.ts` 251-252), and `resolveAdapterFromConfig` passes those roots to `resolveCommand` before PATH (186-190). A repo-local `bin/dlv` therefore wins over the global `dlv`. The test "prefers a nested module adapter over cwd and PATH" (`dap-config.test.ts` 267-280) asserts this as intended.
- Combined with the approval details showing only `Program:`, a launch of a repo file can run a repo-supplied binary without the user seeing its path.

**Adapter-initiated process spawn**
- `runInTerminal` (`session.ts` 1370-1392) runs any `args` the adapter sends, with `cwd` resolved relative to the session cwd (absolute paths are accepted), and env `{...Bun.env, ...NON_INTERACTIVE_ENV, ...adapterEnv}`. The adapter can override anything in `NON_INTERACTIVE_ENV`, including `PAGER` and `GIT_EDITOR`.
- `startDebugging` (1393-1406) opens a child session on the same server. Adapter-chosen `configuration` is passed through verbatim (`{...configuration, cwd}`).
- Neither path is gated beyond the top-level `launch` approval.
- **Null env values are dropped, not unset** (1375-1377). DAP defines `null` as "unset the variable". Here the entry is filtered out of the override, so the inherited value stays. This is a spec deviation.
- omp keeps **no handle** to `runInTerminal` processes: the `proc` returned by `ptree.spawn` is used only for `pid` and stdout draining (1378-1391). omp never kills them, so they outlive the session unless the adapter terminates them.

**Environment**
- Adapters and debuggees inherit the full `Bun.env` (`client.ts` 117-120; `session.ts` 1380-1383). Secrets in the omp process environment reach them. The docs say the OTEL variables are excluded (`docs/environment-variables.md` line 737), but nothing else is.
- `NON_INTERACTIVE_ENV` (`exec/non-interactive-env.ts`) disables pagers and prompts (`GIT_TERMINAL_PROMPT=0`, `GIT_EDITOR=true`, `CI=true`, `TERM=dumb`, and others).

**Killing and runaway processes**
- Adapters are spawned `detached: true`. In `ptree` (read only at 376-400 and grep), a kill on POSIX sends `SIGKILL` to the process group (`process.kill(-pid)`), and on Windows it does a tree kill. **[by reading]**.
- `dispose()` sends `proc.kill()` and awaits exit (`client.ts` 580-598). `ptree.kill(reason, gracefulMs)` also exists; the DAP path calls the no-argument form.
- No exit hook (section 5f).
- The 10 min idle cleanup kills silently (section 5f).

**Timeouts (all configurable or constants):**

| bound | value | where |
|---|---|---|
| tool timeout (per call) | default 30 s, range 5-300 s, optional `tools.maxTimeout` cap | `tool-timeouts.ts:18` |
| DAP request default (client direct use) | 30 s | `client.ts:39` |
| per-write bound | 30 s | `client.ts:45` |
| socket or banner readiness | 10 s | `client.ts:47` |
| initial stop capture after launch or attach | 5 s | `session.ts:116` |
| top-frame fetch | 5 s | `session.ts:1547-1550`, `1633` |
| heartbeat | 5 s | `session.ts:114` |
| cleanup loop | 30 s | `session.ts:113` |
| idle session | 10 min | `session.ts:112` |
| TCP connect retry | 50 ms | `client.ts:874` |
| `startDebugging` child connect | 30 s | `session.ts:1196` |

**Output and memory caps**
- Session output: 128 KiB, oldest chunks dropped (`session.ts` 115, 191-220).
- DAP message: at most 256 MiB body and 16 KiB header (`message-framing.ts` 17-18). A single oversized `variables` reply can be large in memory, and nothing caps it earlier.
- Breakpoint lists: unbounded.
- Raw SSE buffer: 1000 events, 512 000 chars, 64 000 per event (section 3d).
- Log viewer: 50 000 entries (`log-viewer.ts` 36).
- Report logs: 1000 lines per file, 5000 session lines, 2 MB tail per file (`report-bundle.ts` 19-22).

**Concurrency**
- The tool is `exclusive`. Breakpoint mutations are serialised per root (`session.ts` 454-466). Other operations on one session are **not** serialised: a `continue` and an `evaluate` can be in flight together, and the adapter arbitrates.

**Windows and portability (by reading)**
- `/tmp` paths are hardcoded: the Linux unix socket (`client.ts` 250) and the work-profile SVG (`debug/index.ts` 243). The docs repeat the `/tmp` path for the work profile (`docs/tools/debug.md` 126, 275). On Windows, `/tmp` resolves to the drive root, and the SVG write may fail. Unverified.
- Unix-socket tests return early when `process.platform !== "linux"` (`dap-launch-failures.test.ts` 391, 527). They pass vacuously on Windows. The tests that spoof `process.platform` to `darwin` (563-602) run the same code path but do not exercise a real Windows process.
- The unix socket file is never unlinked (no `unlink` in `client.ts`), so socket files accumulate in `/tmp`.

**Config defects that look real [by reading; adapter behaviour unverified]:**
1. **`codelldb`** is declared as `["--port", "0"]` with `connectMode` defaulting to `stdio` (`defaults.json` 31-44). Passing `--port` makes codelldb serve DAP over TCP and print the port, so a stdio client would not get DAP responses. **Likely misconfigured.**
2. **`rdbg`** is declared as `["--open", "--command", "--"]` with stdio (`defaults.json` 124-138). `--open` opens a debug console or socket rather than speaking DAP on stdio. **Likely misconfigured.**
3. **`flutter-debug-adapter`** has `args: ["debug_adapter", "--flutter-sdk-path", ""]` (line 185). `normalizeStringArray` (`config.ts` 24-27) filters out empty strings, so the flag arrives without its value. **Confirmed by reading.**
4. **js-debug args overrides are ignored.** `resolveDefaultJsDebugAdapter` hardcodes `args` and `connectMode` (`config.ts` 158-170). A user `args` or `connectMode` override in `dap.json` has no effect when the js-debug server is found. **Confirmed by reading.**

**Ordering risk.** As described in section 5b, the root session's breakpoints are set after `configurationDone`. Programs that do not stop on entry can run past breakpoints the agent sets later.

**Wrong or misleading output**
- `pause` always prints `Program paused.` (`tools/debug.ts` 815-817), even if the stop never arrived.
- The documented non-fatal timeout for `continue` or steps is probably not reached through the tool (section 5d).
- `output` may return up to 128 KiB into the context.

---

## 9. TypeScript-only versus Rust

- **The entire DAP stack is TypeScript.** `dap/*`, `tools/debug.ts`, `debug/*`, `tui/apps/debug/*`, `jsonrpc/message-framing.ts`, and the process layer (`packages/utils/src/ptree.ts`) are TS. No DAP symbol exists under `crates/` (grep, excluding `vendor/` and `target/`).
- **One Rust touchpoint:** the work profile. `debug/index.ts` imports `getWorkProfile` from `@oh-my-pi/pi-natives` (line 8) and calls `getWorkProfile(30)` (lines 209, 235). The Rust implementation is `crates/pi-natives/src/prof.rs`, `pub fn get_work_profile(last_seconds: f64) -> WorkProfile` (line 182). The body was not read. The `WorkProfile` shape (`folded`, `summary`, `svg`, `sampleCount`) is consumed by `report-bundle.ts` (lines 170-178).
- **Not checked:** whether `ptree`, `$which`, `writeArchive` (`@oh-my-pi/pi-utils/ar`), or `materializeString` have native backing. They are imported by the DAP and report code.
- **Bun and Node builtins a Rust/Tauri port must replace:**
  - `bun:jsc` `heapStats`, `startRemoteDebugger` (UI only).
  - `node:inspector/promises` CPU profiler (`Profiler.start` at 100 µs).
  - `Bun.listen` and `Bun.connect` (TCP and unix). Used for TCP, unix, and the dial-back listener.
  - `Bun.spawn` via `ptree.spawn` with `detached` (process group).
  - `Bun.Archive` and `writeArchive` (tar.gz).
  - `Bun.hash.crc32` (PNG chunks).
  - `Bun.sleep`, `Bun.env`, `Bun.file`.
- **Mapping hints (not in the code):**
  - Stdio adapter: `tokio::process::Command` with piped stdin and stdout; a `Content-Length` framer equivalent to `MessageFramer`, including the 16 KiB header and 256 MiB body caps and the resync-on-junk-header rule.
  - TCP and dial-back: `tokio::net::TcpListener` and `TcpStream`.
  - Unix socket: `tokio::net::UnixStream` on Unix. Windows needs a named pipe or TCP, which the TS code does not provide.
  - Process groups: `setsid` or `process_group(0)` on Unix (`kill(-pgid)`), and a job object or tree kill on Windows.
  - Per-session tasks in place of the `setInterval` heartbeat and cleanup loop.

---

## 10. Snippets worth keeping (non-obvious logic only)

**Output buffer trimming** (`session.ts` 202-208): drops whole front chunks only while the remainder still holds a full cap, so `[120KB, 10KB]` keeps 10 KB, not the full cap. Then `session.ts` 209-219 byte-slices the front chunk so exactly the cap remains.

**Child-first breakpoint apply for a child session** (`session.ts` 1511-1514):

```ts
if (session.parentSessionId) {
    await this.#applyRootBreakpointsToSession(session, signal, timeoutMs);
}
await session.client.sendRequest("configurationDone", {}, signal, timeoutMs);
```

This is the only place breakpoints precede `configurationDone`. The root session has no such branch (1496-1499).

**Null env handling** (`session.ts` 1375-1377), the spec deviation:

```ts
const env = Object.fromEntries(
    Object.entries(args.env ?? {}).filter((entry): entry is [string, string] => entry[1] !== null),
);
```

**js-debug hardcoded args** (`config.ts` 158-170): `args: [serverPath, DAP_PORT_ARGUMENT, "127.0.0.1"]` ignores `config.args`.

---

## 11. Tests (7 files plus one outside the folder; none run)

| file | tests | what it pins down |
|---|---|---|
| `test/debug/dap-config.test.ts` | 13 | `dap.json`, `.omp/dap.yaml`, plugin `.dap.json` loading; partial override merge; invalid entries ignored; relative command resolution; dlv resolution for nested modules, `go.work`, and fresh installs (mocks `$which`) |
| `test/dap-config-precedence.test.ts` | 1 | a cwd `dap.json` overrides the project `.omp/dap.yaml` field by field |
| `test/debug/dap-multi-session.test.ts` | 6 | recursive js-debug child routing; breakpoint fan-out to all sessions; attach-child targeting before a stop; reactivation after a child terminates; focus stays on a stopped child; per-session thread ids preserved; `runInTerminal` stdout drained (128 KiB+) with output cap |
| `test/debug/dap-launch-failures.test.ts` | 32 | framing failure rejects pending work; launchDefaults args kept; combined attach and configurationDone errors; preattached skip; no unhandled rejections; ENOENT message; debugpy hints; delayed launch error preferred; unix-socket readiness (Linux only); wedged stdin timeout; sync flush throw disposes; detached adapters killed on socket and TCP timeouts; ghost socket resilience; banner gate; `DebugTool` validation (directory programs, dlv modes, debugpy unavailable, attach validation, install hints) |
| `test/debug/profiler.test.ts` | 1 | CPU profile works without `setFlagsFromString` on Bun |
| `test/debug/raw-sse-report-bundle.test.ts` | 1 | raw SSE text, with the dropped-record marker, lands in the bundle |
| `test/debug/report-bundle-logs.test.ts` | 4 | same-day PID logs collected; buffered records included; session records past the tail window; 2 MB cap held |
| `test/debug/report-bundle-sessions.test.ts` | 2 | numeric-only memory stats (planted credential absent); only the session's subtree bundled |

The launch-failures fixture `test/fixtures/malformed-jsonrpc-peer.ts` exists and was not read.

---

## 12. Summary of confirmed or likely findings for the port

1. **Repo-controlled adapter commands** resolve before PATH and the approval shows only the program. (Section 8, confirmed by reading.)
2. **Adapter-initiated spawns** (`runInTerminal`, `startDebugging`) are ungated beyond the launch, and omp keeps no handle on debuggee processes. (Section 8.)
3. **Null env** is dropped, not unset (`session.ts` 1375-1377). (Section 8, confirmed.)
4. **Breakpoints after `configurationDone`** for the root session. Programs that run without stop-on-entry can pass breakpoints. (Section 5b.)
5. **Timed-out `continue` or step** probably throws through the tool, contrary to the docs. (Section 5d, by reading, unverified.)
6. **`pause`** always prints "paused" and does not fetch the top frame. (Sections 5d, 6.)
7. **No `frameId` after `pause`**, so `scopes` throws and `evaluate` silently drops the frame. (Section 6, confirmed by reading.)
8. **Breakpoint verification** only from the `setBreakpoints` response; the `breakpoint` event is ignored. (Section 5c.)
9. **Stale thread ids** from `session.threads[0]`. (Section 6.)
10. **Unbounded tool output** except `output` (128 KiB). (Section 4.)
11. **Idle 10-min kill** with no `disconnect`; long debuggee runs can be killed between tool calls. (Section 5f.)
12. **No exit hook**: adapters are not killed if omp exits. (Section 5f, by reading.)
13. **Config defects:** codelldb and rdbg args (likely misconfigured), the flutter empty arg, and js-debug args ignored. (Section 8.)
14. **Windows:** `/tmp` paths, unix sockets never unlinked, and Linux-only tests that pass vacuously on Windows. (Section 8.)
15. **Singleton state:** one debug session for the whole process, shared by every agent session that uses the tool. (Section 6.)
16. **Missing DAP features** for a serious debugger: exceptions, setVariable, restart, goto, completions, variable paging, log points, hit conditions on source breakpoints, step granularity, progress, and handling of `thread` and `invalidated` events. (Section 4.)

Nothing was executed. Items 5, 12, and the adapter-level claims in section 8 need a live run before they are relied on.
