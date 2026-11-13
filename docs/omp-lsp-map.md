# omp (oh-my-pi) LSP support: architecture map

Source: `C:\Users\v3dqn\Work\Project Versa\oh-my-pi-breakdown` (read-only; git HEAD `4ad629ea51d96c18bbdfcc2252fd13b301f57f9f`). Paths below are relative to that repo unless absolute. Line numbers are from the files as read.

Purpose: enough detail to port the LSP subsystem to a Rust/Tauri app. Items marked **[TS-only]** are pure TypeScript/Bun; items marked **[native]** call into `crates/pi-natives` (Rust, exposed through NAPI).

---

## 0. Coverage

**Read in full** (every line):
- `packages/coding-agent/src/lsp/`: `batch.ts`, `client.ts` (2607 lines), `config.ts`, `defaults.json`, `deferred-diagnostics.ts`, `diagnostics-ledger.ts`, `diagnostics.ts`, `edits.ts`, `format-options.ts`, `index.ts`, `lspmux.ts`, `servers.ts`, `settings.ts`, `startup-events.ts`, `tool.ts` (1647 lines), `types.ts`, `utils.ts`, `workspace-diagnostics.ts`, `writethrough.ts`
- `packages/coding-agent/src/lsp/clients/`: `index.ts`, `biome-client.ts`, `lsp-linter-client.ts`, `swiftlint-client.ts`
- `packages/coding-agent/src/lsp/mux/`: `protocol.ts`, `daemon.ts`, `server.ts`
- `docs/lsp-config.md` (278 lines), `packages/coding-agent/src/prompts/tools/lsp.md`, `packages/coding-agent/src/prompts/tools/lsp-late-diagnostic.md`
- All 8 files in `packages/coding-agent/test/lsp/` (test names and assertions read)

**Read only in the regions that matter for the call graph**: `src/sdk.ts` (around 470-500, 5538-5562, 5664-5738), `src/edit/index.ts` (165-190, 365-395, 535-565, 690-823), `src/tools/write.ts` (643-668, 1110-1220), `src/tools/index.ts` (185-200), `src/thread/services/lsp.ts` (all, 90 lines), `src/tools/tool-timeouts.ts` (grep plus lines 1-70), `src/modes/interactive-mode.ts` (2160-2172).

**Not read in full** (outside the LSP folder, named only where the LSP code depends on them): `src/jsonrpc/message-framing.ts` (`MessageFramer`, `encodeMessageFrame`), `src/launch/{client,ensure,paths}.ts` (broker client), `src/subprocess/worker-client.ts`, `src/cli/worker-selectors.ts`, `src/tools/path-utils.ts`, `src/tools/file-write-fallback.ts`, `src/utils/config-file-sources.ts`, `src/capability/fs.ts` (`findRepoRoot`), `src/discovery/helpers.ts`, `src/config/registry.ts`, `packages/utils/src/ptree.ts` (only its imports), `packages/utils/src/tab-spacing.ts` (`getEditorConfigFormatting`, grep only), `@oh-my-pi/pi-tui` (`tools/lsp` types and renderers, `theme`), `crates/pi-natives/src/ipc/*.rs` and `glob.rs` (signatures only), `test/fixtures/fake-lsp-server.ts`, `test/helpers/rejection.ts`. Adjacent tests outside `test/lsp/` were only identified by grep (names listed in section 12).

---

## 1. Context (C4 level 1)

**Actors and internal callers**

| Caller | Where | How it uses LSP |
|---|---|---|
| Agent loop / tool registry | `src/tools/index.ts:613` (`lsp: LspTool.createIf`), `:787` (gating by `lsp.enabled`) | Registers the `lsp` tool. Disabled when `session.enableLsp === false` or the setting is off. |
| Model (via `lsp` tool) | `src/lsp/tool.ts` | Calls the 14 actions listed in section 4. Approval tier: read-only actions are `"read"`, everything else `"write"` (`tool.ts:183-187`). |
| `write` tool | `src/tools/write.ts:652-668`, `:1194-1200`, `:1120` | Builds an LSP writethrough from `lsp.*` settings on each write. Passes batch ids for multi-call responses. |
| `edit` tool | `src/edit/index.ts:173-188`, `:547-564`, `:705-823` | Same writethrough, plus native edit-engine integration. Diagnostics come back as JSON from the write callback. `concurrency = "exclusive"` (`:366`), so a slow LSP call blocks later edits. |
| Native edit engine | `crates/pi-natives/src/edit.rs:9,195`, `crates/pi-edit/src/session.rs:64` | Only comments found. The `lspBatchId` / `lspFlush` flags are carried through the native edit session. The LSP work itself stays in TS ("LSP writethrough / ACP bridge stay in TypeScript", edit.rs:9). |
| Session startup | `src/sdk.ts:5664-5738` | Holds the shared-LSP mode, discovers servers, starts warmup or marks them lazy, emits startup events. |
| Subagent worker threads | `src/thread/services/lsp.ts` | Keeps the worker's shared-LSP mode equal to main's. |
| Shared-LSP hold | `src/cleanse/agent.ts:121` | Holds the shared-LSP mode for its own agent. |
| ACP bridge | `src/tools/acp-bridge.ts:12,99` | Calls `notifyWorkspaceWatchedFiles` when it writes files. |
| DAP | `src/dap/config.ts:5` | Reuses `hasRootMarkers`, `parseConfigContent`, `resolveCommand` from `lsp/config.ts`. |
| CLI | `src/cli.ts:157` (`smokeTestLspMux`), `:321-322` (`startLspMuxFromEnvironment` when argv is `__omp_worker_lsp_mux`) | Mux smoke test and mux worker entry. |
| UI | `src/modes/interactive-mode.ts:2165` | Subscribes to the `lsp:startup` event channel and renders startup status (skipped when `cfgStartupQuiet`). |

**External systems**

- Language server child processes (about 60 entries in `defaults.json`; see section 5).
- Linter CLIs: `biome` (`lint --reporter=json`, `format --write`), `swiftlint` (`lint --quiet --reporter json`). Called directly with `Bun.spawn`, not through LSP.
- Workspace checker CLIs for `file: "*"`: `cargo check`, `npx tsc --noEmit`, `go build`, `pyright` (`workspace-diagnostics.ts`).
- The third-party `lspmux` binary, if it is on PATH (only wraps rust-analyzer; see section 8).
- The in-house LSP mux daemon, started through the project daemon broker (`src/lsp/mux/daemon.ts`; see section 8).
- Filesystem: `.editorconfig` (via `getEditorConfigFormatting`), root-marker probes, `node_modules/.bin`, `.venv/bin`, etc. (`config.ts:240-255`).

---

## 2. Containers (C4 level 2)

| Container | Process | Boundary | Notes |
|---|---|---|---|
| omp main process (Bun) | main | Owns `clients` map, configs, tool, writethrough | `client.ts` module state is process-global (`clients`, `clientLocks`, `fileOperationLocks`, `pendingDiskWrites`, `initFailures`). Exit hook at `client.ts:2602-2607`. |
| Language server child (private) | child of main via `ptree.spawn` (`client.ts:1428`) | stdio pipes, Content-Length framed JSON-RPC | Default when shared mode is off, or when the mux is unavailable. |
| Language server child (shared) | child of the mux daemon | Mux relays frames over IPC | Spawned by `mux/server.ts` `#spawnServer` (`:382-391`). |
| LSP mux daemon (`omp lsp mux`) | separate worker process per project scope | IPC endpoint `ipcEndpoint(daemonRuntimeDir(projectDir), "lsp-mux")` (`daemon.ts:238`), named `omp.lsp.mux` | Started through the broker (`daemon.ts:236-292`). Keeps one or more servers, relays frames, replays state. |
| Broker-client link | main ↔ mux | Raw framing (`framing: "raw"`) over a Unix socket or named pipe | Implemented in Rust `crates/pi-natives/src/ipc/` (NAPI `IpcServer`, `IpcConnection`, `ipcEndpoint`). |
| lspmux (third-party) | external binary | `lspmux client` with `LSPMUX_SERVER` env | Only for rust-analyzer, and only if installed. |
| Linter CLI children | child processes per call | stdin/stdout JSON | Biome and SwiftLint, one spawn per lint or format call. |

Trust boundary: the mux takes an arbitrary `command` and `args` in `omp/muxConnect` (`mux/server.ts:238-258`, `ConnectParams`) and spawns it. Anything that can connect to the endpoint can run commands as the mux user. Access control therefore depends on the IPC endpoint permissions. I did not verify those permissions in the Rust IPC code.

---

## 3. Components (C4 level 3)

One entry per file. "Exports" lists the main exported names.

### Core protocol and state

**`client.ts` (2607 lines)**: JSON-RPC transport and client registry. This is the core of the module.
- Exports: `getOrCreateClient`, `getActiveOrPendingClient`, `shutdownClientInstance`, `shutdownClientServer`, `shutdownClient`, `shutdownAll`, `shutdownStaleClients`, `clearInitializationFailure`, `sendRequest`, `sendNotification`, `ensureFileOpen`, `syncContent`, `notifySaved`, `refreshFile`, `reopenFile`, `rebuildFile`, `refreshOpenFiles`, `reconcileFileFromDisk`, `reconcileOpenFilesFromDisk`, `waitForProjectLoaded`, `ensureWorkspaceFolder(s)`, `applyWorkspaceEditWithLsp`, `notifyWorkspaceWatchedFiles`, `beginPendingDiskWrite` / `endPendingDiskWrite`, `supportsDocumentDiagnostics`, `listCargoManifests`, `isRustAnalyzerClient`, `isIdleClient`, `checkIdleClients`, `setIdleTimeout`, `holdSharedLsp`, `isSharedLspEnabled`, `onSharedLspChange`, `getActiveClients`, `CLIENT_CAPABILITIES` (not exported), `FileChangeType` enum, `LspResponseError`, `LspRequestTimeoutError`, `WARMUP_TIMEOUT_MS = 5000`.
- Depends on: `config.ts` (`getConfig`), `edits.ts` (`applyWorkspaceEdit`), `lspmux.ts`, `mux/daemon.ts` (`connectSharedLspTransport`), `mux/protocol.ts`, `utils.ts` (`EquivalentUriMap`, `detectLanguageId`, `fileToUri`, `uriToFile`, `mergeDiagnosticLists`, `readTextFromDisk`), `jsonrpc/message-framing.ts`, `tools/tool-errors.ts`, `tools/path-utils.ts` (`resolveSyscallTarget`), `capability/fs.ts` (`findRepoRoot`), `pi-natives` (`glob`), `pi-utils` (`ptree`, `postmortem`, `stableStringifyJson`, `untilAborted`).

**`types.ts` (522)**: all LSP wire types (`Position`, `Range`, `Location`, `LocationLink`, `Diagnostic`, `PublishedDiagnostics`, `TextEdit`, `WorkspaceEdit`, `DocumentChange`, `CodeAction`, `Command`, `DocumentSymbol`, `SymbolInformation`, `Hover`, `SymbolKind` names), `ServerConfig`, `ServerCapabilities`, `LspTransport`, `LspWriteSink`, `OpenFile`, `LspClient`, `LinterClient`, `LinterClientFactory`, `lspSchema` (the tool's TypeBox-style schema via `@oh-my-pi/omptype`). Pure TS.

**`config.ts` (545)**: config loading, root-marker detection, binary resolution, server selection.
- Exports: `loadConfig`, `getConfig`, `configCache`, `parseConfigContent`, `hasRootMarkers`, `hasRootMarkerAncestor`, `resolveCommand`, `getServersForFile`, `getServerForFile`, `hasCapability`, `LspConfig`, `ResolveCommandOptions`.
- Depends on: `clients/biome-client.ts`, `clients/swiftlint-client.ts` (factories), `defaults.json`, `utils/config-file-sources.ts`, `pi-utils` (`$which`, `logger`), `bun` (`YAML`, `TOML`, `Bun.Glob`).

**`servers.ts` (296)**: orchestration helpers over clients for a list of servers.
- Exports: `LSP_READONLY_ACTIONS`, `discoverStartupLspServers`, `warmupLspServers`, `getLspStatus`, `syncFileContent`, `notifyFileSaved`, `splitServers`, `getLspServers`, `getLspServersForFile`, `getLspServerForFile`, `isProjectAwareLspServer`, `isMethodNotFoundError`, `reloadConfigurationParams`, `reloadServer`, `LspStartupServerInfo`, `LspWarmupResult`, `LspWarmupOptions`.

**`settings.ts` (114)**: `lsp.*` settings registered with `config/registry`. Defaults: `lsp.enabled` true, `lsp.lazy` true, `lsp.shared` true, `lsp.formatOnWrite` false, `lsp.diagnosticsOnWrite` true, `lsp.diagnosticsOnEdit` false, `lsp.diagnosticsDeduplicate` true, `lsp.requestTimeout` 10 (seconds; options 10/20/30/60/120/300).

**`index.ts` (10)**: barrel. Exports `LspTool`, `createLspWritethrough`, `flushLspWritethroughBatch`, `writethroughNoop`, `discoverStartupLspServers`, `warmupLspServers`, `getLspStatus`, `LSP_READONLY_ACTIONS`, and types.

**`startup-events.ts` (13)**: `LSP_STARTUP_EVENT_CHANNEL = "lsp:startup"` and the `LspStartupEvent` union (`completed` with per-server `ready`/`error`, or `failed` with an error string).

### Model-facing tool

**`tool.ts` (1647)**: the `lsp` tool. `class LspTool implements AgentTool`.
- `name = "lsp"`, `loadMode = "discoverable"`, `strict = true`. `static createIf(session)` returns null when `session.enableLsp === false`.
- `approval(args)`: `"read"` for `LSP_READONLY_ACTIONS`, else `"write"`.
- `execute(...)`: a single method, roughly 1400 lines, with one branch per action (section 4). It does the per-call budget, reconciliation, server selection, and output formatting.
- Depends on almost every other file in the module, plus `tools/tool-timeouts.ts`, `pi-tui` renderers, `workspace-diagnostics.ts`.

**`batch.ts` (24)**: `getLspBatchRequest(toolCall)` returns `{id, flush}` when the model response has more than one `edit`/`write` call. `flush` is true only for the last such call. Depends on `pi-agent-core` `ToolCallContext`.

### Diagnostics

**`diagnostics.ts` (828)**: waiting for and collecting diagnostics, and formatting through LSP.
- Exports: `waitForDiagnostics`, `waitForRebuiltDiagnostics`, `requestDocumentDiagnostics` (not exported; a `textDocument/diagnostic` pull), `getDiagnosticsForFile`, `captureDiagnosticVersions`, `captureOpenFileVersions`, `formatContent`, `normalizeLocationResult`, `formatLocationWithContext`, `isOnlyQueriedDeclaration`, `hasRustWorkspaceAncestor`, `unreportedSummary`, `limitDiagnosticMessages`, the wait and cap constants, and `PROJECT_INDEXED_ACTIONS`.
- Depends on `client.ts`, `clients/index.ts` (`getLinterClient`), `config.ts` (`hasRootMarkerAncestor`), `edits.ts` (`applyTextEditsToString`), `format-options.ts`, `servers.ts`, `utils.ts`.

**`deferred-diagnostics.ts` (68)**: `class DeferredDiagnostics`. One per mutation tool instance. `begin(path)` returns a handle. Aborts any earlier pending fetch for the same path. Uses a per-path version counter (`session.bumpFileMutationVersion` / `getFileMutationVersion`) to mark late results stale. On delivery it applies the ledger (if dedup is on) and calls `session.queueDeferredDiagnostics`.

**`diagnostics-ledger.ts` (51)**: `class DiagnosticsLedger` and `getDiagnosticsLedger(owner)`. Per-session, per-file set of seen diagnostic identities. `reduce(path, result)` keeps only messages whose identity is new. The identity is the message with its leading `path:line:col ` removed (`DIAGNOSTIC_LOCATION_PREFIX_RE`). It returns the original object when nothing is removed. When the ledger sees no diagnostics, it deletes that file's entry, so a later reappearance shows again.

**`workspace-diagnostics.ts` (267)**: `runWorkspaceDiagnostics(cwd, signal)` for `diagnostics` with `file: "*"`. This is not LSP at all. It runs CLI checkers chosen by root marker (see section 6). Exports `detectProjectTypes`, `runWorkspaceDiagnostics`, `interpretEmptyDiagnosticsResult`.

### Edits and formatting

**`edits.ts` (452)**: text-edit and workspace-edit application.
- Exports: `applyTextEditsToString`, `sortAndValidateTextEdits`, `rangesOverlap`, `flattenWorkspaceTextEdits`, `applyTextEdits`, `applyEditsThenRename`, `applyWorkspaceEdit`, `RenameReferenceEdit`, `ExecutedWorkspaceChange`, `WorkspaceEditResult`.
- Depends on `utils.ts` (`readTextFromDisk`, `uriToFile`), `tools/path-utils.ts`, `pi-utils` (`isEexist`, `isEnoent`).

**`writethrough.ts` (760)**: the write-path orchestrator. Exports `createLspWritethrough`, `writethroughNoop`, `flushLspWritethroughBatch`, types `WritethroughOptions`, `WritethroughCallback`, `WritethroughDeferredHandle`, `WritethroughResult`. Depends on `client.ts`, `config.ts`, `diagnostics.ts`, `servers.ts`, `utils.ts`, `tools/file-write-fallback.ts`.

**`format-options.ts` (136)**: `resolveFormatOptions(filePath, content)` and `detectIndentFromContent(content)`. Precedence: `.editorconfig` > content sniffing > fallback (tabSize 2, spaces). Always sets trim and final-newline flags. Depends on `pi-utils` `getEditorConfigFormatting`.

### Utilities

**`utils.ts` (748)**: URI handling, `EquivalentUriMap`, diagnostic formatting and summaries, location and symbol formatting, workspace-edit formatting, hover extraction, code-action listing and apply (`applyCodeAction`, `mergeCodeActions`), glob target resolution (`resolveDiagnosticTargets`, `collectGlobMatches`), `resolveSymbolColumn` (symbol to column), `readLocationContext`. Depends on `theme` (pi-tui) for icons and `lang-from-path` (pi-tui) for `detectLanguageId`.

### Linters

**`clients/index.ts` (50)**: `getLinterClient(name, config, cwd)`, cached by `name:cwd`. Uses `config.createClient` or falls back to `LspLinterClient`. `clearLinterClientCache` is exported but has **no callers**.

**`clients/biome-client.ts` (226)**: `BiomeClient` (section 9).

**`clients/swiftlint-client.ts` (125)**: `SwiftLintClient` (section 9).

**`clients/lsp-linter-client.ts` (86)**: `LspLinterClient`. Wraps a normal LSP client as a `LinterClient`. Reachable only through `getLinterClient`, which is only called when `serverConfig.createClient` is set. So in the shipped config this class is effectively unused (section 9).

### Mux

**`lspmux.ts` (231)**: third-party `lspmux` detection and command wrapping (section 8).

**`mux/protocol.ts` (123)**: the mux wire contract. Method names (`omp/muxConnect`, `omp/muxPing`, `omp/muxRestartServer`), `MuxConnectParams`, `MuxConnectResult`, `muxServerKey` (section 8).

**`mux/daemon.ts` (357)**: client side. `connectSharedLspTransport(opts)` returns an `LspTransport` (or null for fallback). Also `ensureLspMuxDaemon`, `dialMuxServer`, `probeMux`, `linkTransport`, `smokeTestLspMux`. Depends on `pi-natives` (`IpcConnection`, `ipcEndpoint`), `launch/*` (broker), `subprocess/worker-client`.

**`mux/server.ts` (717)**: the mux worker. `class LspMuxServer` (exported) and `startLspMuxFromEnvironment()`. Pure TS on top of `pi-natives` `IpcServer`.

---

## 4. The model-facing tool surface

Schema: `types.ts:8-22` (`lspSchema`, `strict: true`). The `action` enum has **14** values.

Argument fields: `action` (required), `file`, `line` (1-indexed), `symbol` (`name` or `name#N` for the Nth occurrence on the line), `query`, `new_name`, `apply` (default true for `rename`, `rename_file`, `code_actions`; `code_actions` applies only when `apply: true`), `timeout` (seconds, 5-300), `payload` (JSON string; only for `request`).

Timeout budget (`tools/tool-timeouts.ts:49-70`): with an explicit `timeout`, the whole call and the server wait both use that value. Without one, the call uses `max(20, lsp.requestTimeout)` clamped to 5-300 seconds, and the server wait uses `lsp.requestTimeout` (default 10). Per-call deadline: `AbortSignal.timeout` combined with the caller signal (`tool.ts:229-238`). A timeout becomes a `ToolError` naming the budget and server.

Read-only tier (`servers.ts:26-36`): `diagnostics`, `definition`, `type_definition`, `implementation`, `references`, `hover`, `symbols`, `status`, `capabilities`. Everything else, including `code_actions` even when only listing, is write tier. Session flag `lspReadOnly` rejects write-tier actions (`tool.ts:226-228`).

Results are plain text in `content[0].text`, plus `details` (`{action, serverName, success, request}`). Empty lookups set `useless: true`, which lets compaction drop them. Clean diagnostics runs do not set it.

### The 14 operations

1. **`diagnostics`** (read). Args: `file` (path, glob, or `*`). Returns, per target: `OK` (fast single-file case), or a summary line plus one line per diagnostic, or `no issues`. For `*`, see section 6. Glob cap: `MAX_GLOB_DIAGNOSTIC_TARGETS = 20`, with a warning line when truncated. Multiple targets or a glob use "detailed" output with a `relPath:` header. Unreported servers are listed with a hint to retry. Per-server failures are noted. Note: this action does **not** apply the 50-message cap that `getDiagnosticsForFile` and `mergeDiagnostics` apply (`limitDiagnosticMessages`).
2. **`definition`** (read). Args: `file`, `line`, `symbol`. Returns `Found N definition(s):` plus `path:line:col` and one line of context (`readLocationContext`, 1 line either side). `No definition found` when empty. For project-aware servers, `symbol` is required when `line` is given (`tool.ts:1254-1264`).
3. **`type_definition`** (read). Same shape as `definition`.
4. **`implementation`** (read). Same shape as `definition`.
5. **`references`** (read). Args: `file`, `line`, `symbol`. Sends `includeDeclaration: true`. For project-aware servers that return only the queried declaration, it retries up to `REFERENCES_RETRY_COUNT = 2` times with `REFERENCES_RETRY_DELAY_MS = 250`, waiting for project load in between (`tool.ts:1371-1397`). Output: first `REFERENCE_CONTEXT_LIMIT = 50` with one line of context, then `... N additional reference(s) shown without context`, then bare `path:line:col` lines.
6. **`hover`** (read). Args: `file`, `line`, `symbol`. Returns `extractHoverText` (raw markdown or plaintext). `No hover information` when empty.
7. **`symbols`** (read). Two modes. Document: `file` only. Returns a tree (`DocumentSymbol`, indented by depth, with icon and `@ line N`) or a flat list (`SymbolInformation`). Workspace: `file: "*"` with `query`. Sends `workspace/symbol` to every non-linter server, then dedupes (`dedupeWorkspaceSymbols`), filters client-side by substring (`filterWorkspaceSymbols`), and caps at `WORKSPACE_SYMBOL_LIMIT = 200` with an elision line. Failed servers are listed in `Server failures:`.
8. **`rename`** (write). Args: `file`, `line`, `symbol`, `new_name`, `apply` (default true). Sends `textDocument/rename`. Applies the returned `WorkspaceEdit` through `applyWorkspaceEditWithLsp` and reports `Applied rename:` with per-file lines. With `apply: false` returns a preview from `formatWorkspaceEdit`.
9. **`rename_file`** (write). Args: `file` (source), `new_name` (destination), `apply` (default true). Works on directories too: enumerates up to `MAX_RENAME_PAIRS = 1000` files and builds one `{oldUri, newUri}` pair per file. Sends `workspace/willRenameFiles` only to servers whose `fileTypes` match a source or destination file. Ensures each relevant checkout is in the server's workspace first (`ensureWorkspaceFolders`). If any server supports the request and fails, it aborts **before** moving anything (issue #8380). Otherwise it coalesces the returned text edits per URI (a project-aware server wins on overlap), validates them, applies all reference edits and the move as one unit (`applyEditsThenRename`, with rollback), then sends `didClose` for moved open documents and `workspace/didRenameFiles`.
10. **`code_actions`** (write tier). Args: `file`, `line`, `symbol`; `query` (kind filter when listing, title substring or index when applying); `apply`. Requests `textDocument/codeAction` with `triggerKind: 1`, twice: once unfiltered and once with `only: ["source"]`, then merges (`mergeCodeActions`). Reason: some servers (typescript-language-server) return source actions only when `context.only` asks. Listing format: `index: [kind] title (preferred)`. Applying: `codeAction/resolve` if there is no edit, then the edit, then `workspace/executeCommand`. Output is a summary of applied edits and commands.
11. **`status`** (read). No file needed. Lists configured servers as `name (configured, not started)` or `name (<status>)` based on live clients, plus an lspmux status line if detected. The note explains that "configured" only means the binary resolves.
12. **`reload`** (write tier). `file: "*"` (or omitted): drops the config cache for `cwd`, reloads config, stops clients whose identity changed or was removed (`shutdownStaleClients`), and calls `reloadServer` for each server. With a file: restarts or reloads just that file's server. `reloadServer` (`servers.ts:259-289`) uses `rust-analyzer/reloadWorkspace` for rust-analyzer when the crate set has not changed. Otherwise it sends `workspace/didChangeConfiguration` as a **notification** with the configured settings, then `refreshOpenFiles`. If sending fails, it restarts the server.
13. **`capabilities`** (read). Optional `file` (filters servers). Dumps `client.serverCapabilities` as JSON per server.
14. **`request`** (write tier). `query` is the method name (required). `payload` is JSON params. If `payload` is absent: with `file` and `line`, params are `{textDocument, position}`; with only `file`, `{textDocument}`; otherwise `{}`. Output: `server ← method:` followed by the result (JSON pretty-printed, or the raw string). On error, shows the method, the message, and a 400-char preview of the params sent. Raw escape hatch for `rust-analyzer/expandMacro` and similar.

---

## 5. Lifecycle

### 5.1 Discovery and config (`config.ts`, `defaults.json`)

1. `loadConfig(cwd)` (`config.ts:441-472`) starts from `defaults.json` (`coerceServerConfigs`), then merges config sources from lowest to highest priority via `configFileSources("lsp", cwd, adaptConfig, {pluginRootSource})`. Per `docs/lsp-config.md`: `~/lsp.*`; plugin roots; user config dirs (`~/.omp/agent/`, `~/.claude/`, `~/.codex/`, `~/.gemini/`, opt-in); cwd dirs (`.omp/`, `.claude/`, `.codex/`, `.gemini/`); cwd root `lsp.*` and `.lsp.*` (highest).
2. Merge is shallow per server: `{ ...base, ...override }` (`config.ts:124-146`). Object fields like `settings` and `initOptions` replace the whole value (`docs/lsp-config.md:34`). An invalid override keeps the previous server; an invalid new server is dropped with a warning.
3. Accepted file shape: `{ servers: {...}, idleTimeoutMs }` or a flat map, JSON or YAML (`parseConfigContent`, `config.ts:40-46`). Compatibility: `initializationOptions` is accepted as an alias for `initOptions`, and `extensionToLanguage` (VS Code-style) supplies `fileTypes`.
4. Runtime filter (`config.ts:458-469`): keep the server if it is not `disabled`, `hasRootMarkers(cwd, rootMarkers)` is true (cwd only, one-level readdir for `*` patterns), and `resolveCommand` finds the binary. `resolveCommand` checks project-local bins first (`node_modules/.bin`, `.venv/{bin,Scripts}`, `venv/...`, `.env/...`, `vendor/bundle/bin`, `bin` for Gemfile and go.mod projects) and then `$PATH`. On Windows it also tries `.exe`, `.cmd`, `.bat` (`config.ts:257-270`). The result is stored as `resolvedCommand`.
5. `selectTypescriptServer` (`config.ts:362-370`): exactly one TypeScript server is kept. If the resolved `tsc` has no `lib/tsserver.js` (TypeScript 7+), `typescript-native` (`tsc --lsp --stdio`) wins. Otherwise `typescript-native` is dropped. For installs with no npm layout, it runs `tsc --version` (3-second timeout).
6. `applyRuntimeDefaults` (`config.ts:148-165`) replaces `createClient` for `biome` and `swiftlint` (so those are never spawned as LSP), and substitutes `$PID` in omnisharp args with the process pid.
7. Cached per cwd in `configCache`. `reload *` deletes the cache entry (`tool.ts:1172`).

`defaults.json` has about 60 entries. Two notes: `biome` has `args: ["lsp-proxy"]` but never runs as LSP because of `createClient`. Two servers can share a binary: `typescript-language-server` and `typescript-native` (both match .ts), and `pyright`/`basedpyright`/`pylsp`/`ty`/`ruff` all match Python.

Server selection (`getServersForFile`, `config.ts:494-526`): matches on extension (with or without dot), or on basename (`Dockerfile`, `.gitignore`-like names), case-insensitively. Results are sorted so non-linters come first (`isLinter`). `getServerForFile` returns the first.

### 5.2 Lazy start and warmup (`sdk.ts:5688-5737`, `servers.ts:76-117`)

- Startup discovery (`discoverStartupLspServers`) runs only when `lsp.enabled`, `hasUI` is true, and an agent session is created. Servers appear as `available` (lazy) or `connecting`.
- `lsp.lazy` true (default): no warmup. Servers start on first use: the `lsp` tool, or an edit or write to a matching file type (via `getOrCreateClient`).
- `lsp.lazy` false: `warmupLspServers` runs `getOrCreateClient` for every non-custom-linter server in parallel (`Promise.allSettled`), each with `serverConfig.warmupTimeoutMs ?? WARMUP_TIMEOUT_MS` (5000 ms; marksman uses 2000 ms in `defaults.json`).
- Turning `lsp.lazy` off mid-session triggers warmup once (`cfgLspLazy.listen`).
- Results go out on the event bus as `lsp:startup` (section 5.8).

### 5.3 Cold start (`client.ts:1347-1556`, `getOrCreateClient`)

1. **Identity key**: `clientKey(config, cwd)` = `${resolvedCommand ?? command}:${cwd}:${stableStringify([args, initOptions, settings, languageId])}` (`client.ts:1248-1257`). A changed config gets a fresh client. Same binary with different args does not share.
2. **Dedupe**: a live entry in `clients` is returned directly (updates `lastActivity`). A concurrent start is shared through `clientLocks` (a pending promise plus a token).
3. **Reload barrier**: a cold start waits for any `clientReloadBarriers` promise for the cwd, and fails if the key was invalidated (`invalidatedClientKeys`).
4. **Negative cache**: `initFailures` records deterministic init failures for `INIT_FAILURE_BACKOFF_MS = 3 min`. Timeouts under a caller-supplied deadline and aborts are not cached (`client.ts:1535-1548`). Recent failures fail fast with `LSP server X failed to initialize recently`.
5. **Transport**: if `lspmux` wraps the command (rust-analyzer only), that wrapper is used. If shared mode is on and the command was not wrapped, `connectSharedLspTransport` is tried first (section 8). On any failure it falls back to `ptree.spawn([command, ...args], {cwd, stdin: "pipe", env})` (`client.ts:1428`).
6. **Process-exit hook** (`client.ts:1459-1484`): removes the client from `clients`, rejects every pending request with `LSP server exited (code N): <stderr>` (info lines of the form `[HH:MM:SS INF]` are stripped), and resolves project-load waiters.
7. **Reader** starts (`startMessageReader`, section 5.5).
8. **`initialize`** request (`client.ts:1492-1505`): `processId: process.pid`, `rootUri` and `rootPath` (both = cwd), `capabilities: CLIENT_CAPABILITIES`, `initializationOptions` (for clangd, `{clangdFileStatus: true, ...config.initOptions}`), `workspaceFolders`. The timeout is the caller's `initTimeoutMs` if given, otherwise the caller's `signal`, otherwise `DEFAULT_REQUEST_TIMEOUT_MS = 30000`.
9. **`initialized`** notification, then **`workspace/didChangeConfiguration`** with `{settings: config.settings ?? {}}` (`client.ts:1514-1520`).
10. Publish: `status = "ready"`. rust-analyzer also starts `listCargoManifests`. The client is added to `clients` only after this succeeds. A superseded key throws. The cache entry is cleared on success.
11. On any failure during init, the process is killed, `status = "error"`, and the failure may be negative-cached.

**Client capabilities advertised** (`CLIENT_CAPABILITIES`, `client.ts:273-376`):
- `textDocument`: `synchronization` with `didSave: true` (no `willSave`, no `willSaveWaitUntil`). `hover` (markdown and plaintext). `definition`, `typeDefinition`, `implementation` with `linkSupport: true`. `references`. `documentSymbol` with hierarchical support and all 26 symbol kinds. `rename` with `prepareSupport: true`. `codeAction` with the 8 literal kinds (quickfix, refactor, refactor.extract, refactor.inline, refactor.rewrite, source, source.organizeImports, source.fixAll) and `resolveSupport: ["edit"]`. `formatting` and `rangeFormatting`. `publishDiagnostics` with relatedInformation, versionSupport, tags 1 and 2, codeDescriptionSupport, dataSupport. `diagnostic` (pull) with `dynamicRegistration: true`.
- `window.workDoneProgress: true`.
- `workspace`: `applyEdit: true`, `workspaceEdit` with documentChanges, resource ops create/rename/delete, `failureHandling: "abort"`. `configuration: true`. `workspaceFolders: true`. `symbol` with all kinds. `fileOperations`: `willRename` and `didRename` only.

Note: `formatting` is advertised, but the code checks `serverCapabilities.documentFormattingProvider` before sending formatting (`diagnostics.ts:792`).

### 5.4 Document sync (full-text, not incremental)

The client always sends **full document text** in `didChange` (`contentChanges: [{text}]`). It never sends ranges.

- `ensureFileOpen` (`client.ts:1669-1733`): reads from disk; if the file is missing, returns silently. Then `ensureWorkspaceFolder` (see below), `closeIdleFiles`, then `didOpen` with `version = (idleClosedFiles.get(uri) ?? 0) + 1`.
- `syncContent` (`client.ts:1929-1997`): used for in-memory content before a save. If the document is not open, `didOpen` with the given text. If open, `version = ++info.version` and `didChange`. Sets `syncedHash = Bun.hash(content)`.
- `notifySaved` (`client.ts:2014-2030`): `didSave`. The text is included only when the server advertises `textDocumentSync.save.includeText` (`saveIncludesText`). Otherwise the text is already on the server from `didChange`.
- `refreshFile` (`refreshDocument` with `purpose: "use"`): `didChange` plus `didSave` with the disk text.
- `reopenFile` (`"reopen"`): `didClose`, then `didOpen` at a resumed version, then `didSave`. Used for servers that compute diagnostics on open and do not republish on identical-text change.
- `rebuildFile` (`"rebuild"`): clangd only. Sends `didChange` with `content + "\n"` and `wantDiagnostics: true`, then restores `content` with `wantDiagnostics: true`. This forces clangd to rebuild and re-check headers (`client.ts:2218-2243`).
- `refreshOpenFiles`: re-sends every open document as `"resync"` (used after watched-file changes that alter resolution).
- `reconcileFileFromDisk` and `reconcileOpenFilesFromDisk` (`client.ts:1754-1905`): the external-edit reconciler. Before a query, each open document is compared with disk. A `diskStamp` (`ctimeNs:mtimeNs:size` from `fs.stat`) is checked first; if it matches and the hash equals `syncedHash`, nothing is read. Otherwise the file is read, hashed with `Bun.hash`, and a full `didChange` is sent if it differs. A file that is gone is closed. A file that is unreadable throws, or is closed when `unreadable: "close"` is passed.
- A document whose OMP write is in flight (`beginPendingDiskWrite`, refcounted) is skipped by reconciliation, so the overlay (which is ahead of disk) is not reverted.

**Idle close** (`client.ts:1618-1646`): documents not used by an OMP operation within `OPEN_FILE_IDLE_MS = 5 min` are closed (`didClose`) before the next `didOpen`. The stated reason is that a TypeScript server drops a project whose documents are all closed. Closed documents are remembered in `idleClosedFiles` so the next open continues the version and is flagged `reopenedAt`.

**Workspace folders** (`client.ts:718-764`): if a file lives in a different git checkout from the client's root, and the server advertises `workspace.workspaceFolders.changeNotifications`, the checkout is added with `workspace/didChangeWorkspaceFolders`. Never a checkout at or above `$HOME`. Removed again when the mux session closes (mux only).

### 5.5 Reader and server-initiated requests (`client.ts:514-668`, `1005-1057`)

The reader parses Content-Length frames (`MessageFramer`) and routes:
- A message with `method` and `id`: a **server request**, answered in `handleServerRequest`. Handled: `workspace/configuration` (returns `client.config.settings[section]`, an exact top-level key lookup, not a dotted path), `workspace/workspaceFolders`, `workspace/applyEdit` (runs `applyWorkspaceEditWithLsp`), `window/workDoneProgress/create` (null), `client/registerCapability` and `client/unregisterCapability` (tracked in `dynamicCapabilityRegistrations`, then null), `window/showMessageRequest` (null), `window/showDocument` (`{success: false}`), the seven `workspace/*/refresh` methods (null). Anything else: `-32601 Method not found`.
- A message with `method` and no `id`: a **notification**. Handled: `textDocument/publishDiagnostics` (stored, merged with pulled results for pull-capable servers), `$/progress` (begin and end tokens tracked for project load), `textDocument/clangd.fileStatus` (build status per URI with sequence numbers).
- A message with `id` and no `method`: a **response**. Matched to `pendingRequests`. Errors become `LspResponseError(code, data, message)`.
- Reader death: pending requests are rejected. If the process is still alive, `status = "error"`, the client is removed, and the process is killed.

### 5.6 Writes, timeouts, cancellation, wedged writers (`client.ts:406-504`, `2387-2541`)

- Writes are serialized per client through `writeQueue`. A write whose caller already aborted is skipped, unless it is a `didClose` (queued with `deliverQueued`), which is still written within `DELIVER_QUEUED_TIMEOUT_MS = 2000`.
- `writeMessage` races `sink.write` and `sink.flush` against the caller's signal. If a drain is interrupted by an abort, the client is torn down as wedged (`teardownWedgedClient`). Otherwise an abort only releases the caller.
- `sendRequest` timeout policy (`client.ts:2370-2386`): an explicit `timeoutMs` is used. Otherwise, if a signal is present, there is no internal timer, since the caller owns the deadline. Otherwise 30 s. On timeout, a `$/cancelRequest` is sent and `LspRequestTimeoutError` is thrown. On abort, `$/cancelRequest` is sent and the caller's reason is rethrown.
- `ContentModified` (`-32801`) during workspace indexing is retried once the project-load promise settles, within the original budget (`client.ts:2401-2424`). Commands are never retried.
- Notifications are not timed out on the reader side. They are bounded by their signal, or by `WATCHED_FILES_NOTIFY_TIMEOUT_MS = 2000` for watched-file notifications.

### 5.7 Crash recovery, restart, shutdown

- **Crash**: the process-exit hook removes the client. The next call respawns through `getOrCreateClient`. There is no automatic restart loop.
- **Reload**: `lsp reload` (section 4) restarts or reloads. `shutdownClientInstance` (`client.ts:2311-2337`): `shutdown` request (5 s budget), then `exit` notification, then wait 1 s, then `kill`. Returns `true` only when the process has actually exited. A restart reports failure if the process outlives the budget.
- **Shared-server teardown**: `shutdownClientServer` first sends `omp/muxRestartServer` on a mux link (`client.ts:2348-2353`), so the shared server dies too.
- **Idle reaping**: opt-in. `idleTimeoutMs` (config or `setIdleTimeout`) starts a 60-second sweep (`IDLE_CHECK_INTERVAL_MS`). A client is idle only when it has no pending requests and `now - lastActivity > timeout` (`isIdleClient`, `client.ts:188-191`). Off by default (`docs/lsp-config.md:72`).
- **Process exit**: `shutdownAll()` runs on `beforeExit` and on `postmortem` cleanup (`client.ts:2602-2607`). It also covers clients still mid-init (`clientLocks`).

### 5.8 Startup events

- `LSP_STARTUP_EVENT_CHANNEL = "lsp:startup"` (`startup-events.ts:3`).
- `sdk.ts:5715,5727` emit `{type: "completed", servers: [{name, status: "ready"|"error", fileTypes, error?}]}` or `{type: "failed", error}`. Emitted only when `cfgStartupQuiet` is off.
- `interactive-mode.ts:2165` subscribes and renders the status, unless quiet.
- The `startupLspServers` array is mutated in place as warmup finishes, so the UI can read live status.

---

## 6. Diagnostics pipeline

### 6.1 Sources of diagnostics

- **Push**: `textDocument/publishDiagnostics` goes into `publishedDiagnostics` and `diagnostics`, with the document `version` if the server echoes it. Sets `publishesDocumentVersions`.
- **Pull**: `textDocument/diagnostic` (LSP 3.17). Used when the server advertises `diagnosticProvider` or dynamically registers `textDocument/diagnostic` (`supportsDocumentDiagnostics`, `client.ts:990-1000`). A `kind: "full"` report yields items. `null` means "declined" (ruby-lsp does this for documents it does not handle). A `ContentModified` or `ServerCancelled` error with `retriggerRequest` is retried after 100 ms (`diagnostics.ts:286-291`).
- **Merge**: for pull-capable servers, the stored list is the pull result merged with pushed results (`mergeDiagnosticLists`, deduplicated by range, source and message).
- **CLI checkers**: custom linters (biome, swiftlint) via `LinterClient.lint` (section 9).
- **Workspace**: `diagnostics` with `file: "*"` runs CLI checkers (section 6.4).

### 6.2 Waiting for a fresh report (`waitForDiagnostics`, `diagnostics.ts:331-494`)

The problem: after an edit, the server's previous report is stale. The module makes sure it returns a report that matches the new text, or reports the file as unverified. The acceptance rules:

- **Push, versioned**: if `published.version === expectedDocumentVersion` (captured after sync), accept immediately.
- **Push, unversioned**: accept only after the same publish has been the newest for `settleMs` (250 ms default), and only if it is not a "provisional cold publish" (an empty unversioned publish from a server started under 3 s ago, or an empty or older publish for a reopened document). Also requires `diagnosticsVersion > minVersion`, where `minVersion` was captured before sync.
- **Pull**: the wait sends pulls on a schedule. A `declined` answer shortens the deadline to one settle window. Pull failures are remembered; if no push fallback arrives, the function throws the failure rather than returning clean.
- **Project-aware pull servers** (`afterProjectLoad`): the wait is held until `projectLoaded`, and the budget restarts after that (so a pull is not answered from a half-loaded project).
- **clangd**: `waitForRebuiltDiagnostics` (`diagnostics.ts:513-544`) polls build status every 20 ms until a busy-then-idle transition after the rebuild, then sends a `documentSymbol` request (which queues behind the AST build) and checks the published version.
- Returns `undefined` for "no fresh report within budget". Callers must not treat that as clean. It is shown as `no diagnostics report yet from <servers>`.

Constants: `SINGLE_DIAGNOSTICS_WAIT_TIMEOUT_MS = 3000` (single file), `BATCH_DIAGNOSTICS_WAIT_TIMEOUT_MS = 400` (glob or `*`), `DIAGNOSTICS_POLL_MS = 100`, `DIAGNOSTICS_SETTLE_MS = 250`, `BUILD_STATUS_POLL_MS = 20`. In the `lsp` tool, project-aware servers use `waitMs` (the call's wait budget) instead of 3 s.

### 6.3 Write and edit inline versus deferred

The writethrough (section 7) fetches diagnostics in two phases (`writethrough.ts:328-389`):
- **Inline**: wait up to `INLINE_DIAGNOSTICS_WAIT_TIMEOUT_MS = 500` ms for a fresh result.
- **Deferred**: if still not ready, the in-flight fetch continues with a budget of `deferredDiagnosticsWaitMs(lsp.requestTimeout)`, which is `max(12 s, lsp.requestTimeout)`. When it settles, `onDeferredDiagnostics` fires, and the result is delivered late to the model.
- Without a deferred channel (direct callers, CI), it blocks for the standard budget and returns inline.

Late delivery (`deferred-diagnostics.ts`): each result goes through `DeferredDiagnostics.#inject`, which applies the ledger if dedup is on and drops the entry if nothing new remains. Then `session.queueDeferredDiagnostics(entry)` (`sdk.ts:2693`) queues it on `session.yieldQueue` under `LSP_LATE_DIAGNOSTIC_MESSAGE_TYPE`. The queue's `isStale` check calls `entry.isStale()`, which returns true when the file's mutation version moved on. The message is rendered by `lsp-late-diagnostic.md` as a `<system-notice>` ("Late LSP diagnostics arrived after the edit returned:") with per-file summaries and message lines.

### 6.4 `file: "*"` workspace diagnostics (not LSP)

`runWorkspaceDiagnostics` (`workspace-diagnostics.ts`) detects checkers from root markers at `cwd`. Every matching marker is collected, so a polyglot root runs all of them:
- `Cargo.toml` → `cargo check --message-format=short`
- `tsconfig.json` → `npx tsc --noEmit`
- `go.work` → `go build` with patterns from `go work edit -json`; `go.mod` → `go build ./...`
- `pyproject.toml` or `pyrightconfig.json` → `pyright`

Up to `MAX_CONCURRENT_CHECKERS = 2` run at once. Output is capped at 50 lines per checker (`[…Nln elided…]`). A non-zero exit with no output is reported as a failure ("the workspace was not verified"), never as "No issues found". Sections are labelled `=== <description> ===` only when more than one checker ran.

### 6.5 Filtering, formatting, deduplication, limits

- **Orphan TypeScript diagnostics** (`diagnostics.ts:131-164`): codes 1375, 1378, 2307, 2580, 2591, 2792, 2867 are dropped for TypeScript-family servers when no ancestor directory has the server's root markers. Explains "cannot find module" noise in stray files.
- **Dedupe**: `getDiagnosticsForFile` and the `diagnostics` action dedupe by `range + message` across servers. `mergeDiagnosticLists` dedupes by `range + source + message`.
- **Sort**: `sortDiagnostics`: severity, then line, then column, then message.
- **Format**: `formatDiagnostic` → `path:line:col [severity] [source] message (code)`. `stripDiagnosticNoise` drops "for further information visit" lines and bare URL lines (`utils.ts:205-218`).
- **Summary**: `N error(s), N warning(s), N info(s), N hint(s)`, else `no issues`. The writethrough summary is `OK` when diagnostics are off.
- **Message cap**: `limitDiagnosticMessages` keeps the first `DIAGNOSTIC_MESSAGE_LIMIT = 50` messages (`getDiagnosticsForFile`, `mergeDiagnostics`). Not applied in the `diagnostics` action (see section 4).
- **Ledger dedup** (`lsp.diagnosticsDeduplicate`, default on): after an edit, only messages whose identity (message without location) was not already shown for that file are returned. Applies to inline and deferred results. Identity is `DIAGNOSTIC_LOCATION_PREFIX_RE = /^.*?:\d+:\d+\s+/`. Line shifts from earlier edits do not make an old error look new.
- **Pipeline bound**: `getDiagnosticsForFile` wraps each server in `untilAborted` with `pipelineBudgetMs = waitBudget + DIAGNOSTICS_PIPELINE_GRACE_MS (10 s)`. A hung server is skipped rather than blocking the edit (issue #4910).
- **Stale tokens**: `captureDiagnosticVersions` before sync and `captureOpenFileVersions` after sync bound what "fresh" means. `expectedDocumentVersions` must match a versioned publish.

---

## 7. Write-through and edits

### 7.1 Writethrough (`writethrough.ts`)

`createLspWritethrough(cwd, opts)` returns `WritethroughCallback(dst, content, signal, file, batch, getDeferred)` → `{finalContent, diagnostics?}`.

1. `changeType = Bun.file(dst).exists() ? Changed : Created`.
2. **No formatting and no diagnostics**: plain write, then `notifyWorkspaceWatchedFiles`.
3. Otherwise, `servers = getServersForFile(config, dst)`. If none, write and notify.
4. `splitServers` separates custom-linter servers (biome, swiftlint) from LSP servers. A custom linter formats only when `enableFormat` is on.
5. `minVersions` captured with a 5 s client-creation budget.
6. `beginPendingDiskWrite(dst)` so reconciliation skips the file.
7. A **5 s operation timer** (`timeoutSignal`) bounds the whole LSP phase. `operationSignal` combines it with the caller's signal.
8. Inside `untilAborted(operationSignal, ...)`:
   - If diagnostics are on, `reconcileOpenFilesFromDisk` runs for the other open documents (not the target).
   - **Custom formatter path**: write the content, run `formatContent` through the linter, commit, notify, then sync formatted text to LSP servers.
   - **LSP path**: for `Created` files, commit and notify first, then `reloadTypeScriptProjectsAfterCreate` (sends `workspace/executeCommand` with `typescript.tsserverRequest`/`reloadProjects` when advertised). Then `syncFileContent(original)`, then `formatContent` (LSP format). Sync formatted text again if it changed. Then commit the write, notify, `captureOpenFileVersions`, and `notifyFileSaved`.
9. Errors inside the LSP phase are **swallowed** (`catch {}`, `writethrough.ts:562`). If the operation timed out, the deferred diagnostics fetch is scheduled. The write is still committed (`commitWrite`), so an LSP problem never loses the agent's content.
10. `endPendingDiskWrite(dst)` in `finally`.
11. `fetchDiagnosticsWithDeferral` (section 6.3) if synced and diagnostics are on.
12. Result: `formatter` set to `FORMATTED`, `UNCHANGED`, `UNSUPPORTED` or `FAILED`. When diagnostics were requested and none arrived, the result carries `unreported` for every server.

Batching: `batch` (from `getLspBatchRequest`) changes the flow. Each `write`/`edit` in the same response is committed to disk immediately (`writethroughNoop`, per file). Only the batch's last call (`flush: true`) runs `flushWritethroughBatch`, which runs LSP post-processing once per file, reading back each file from disk (falling back to the committed content on permission errors). Batches are keyed by `toolCall.batchId` and kept in the module-level `writethroughBatches` map.

Format on write: `formatContent` (`diagnostics.ts:750-828`) orders servers so `isLinter` ones come first. It returns on the first server that advertises `documentFormattingProvider` and produces a result. Servers without the capability are skipped. Initialization failures count as `failed`. The formatting result is applied in memory with `applyTextEditsToString` before the file is written, so the server and disk agree. Formatting is off by default (`lsp.formatOnWrite` false).

### 7.2 Format options (`format-options.ts`)

The `FormattingOptions` sent with `textDocument/formatting` are chosen per file:
1. `.editorconfig` in the file's chain: `indent_style`, `indent_size`, `tab_width` (via `getEditorConfigFormatting`).
2. Otherwise, sniffed from the content about to be written. The first indented line decides spaces vs tabs. For spaces, the most frequent positive indent step (ties go to the smaller) is the unit. Lines inside `/* … */` blocks are skipped (`detectIndentFromContent`, `format-options.ts:57-104`).
3. Otherwise, fallback: `tabSize: 2`, `insertSpaces: true`.

Always sent: `trimTrailingWhitespace: true`, `insertFinalNewline: true`, `trimFinalNewlines: true`. Comment at the top of the file says the previous fixed `tabSize: 3` damaged 2-space files (issue #2329).

### 7.3 Applying server edits (`edits.ts`)

- **Text edits** (`applyTextEditsToString`): sorted bottom-to-top (`sortAndValidateTextEdits`, `edits.ts:92-129`). Equal starts tie-break by original index. Byte-identical non-empty duplicates collapse. Any overlap throws a `ToolError` ("overlapping LSP edits … LSP produced inconsistent edits"). Snippet-format edits (`insertTextFormat: 2`) are rejected.
- **Workspace edits** (`applyWorkspaceEdit`): `documentChanges` are planned in order (`planDocumentChanges`, `edits.ts:222-290`). Text edits accumulate per URI and flush before any resource op on the same URI or subtree. Ops: `create` (`wx`, or overwrite), `rename` (displaces an overwritten target into a sibling `.omp-displaced-*` directory, restores it on failure, and detects a case-only rename on case-insensitive filesystems by comparing dev and ino), `delete` (`rmdir` or recursive `rm`). The legacy `changes` map is applied after validating every file. Returns the executed ops so reconciliation sees only what really ran.
- **Overlay reconciliation** (`applyWorkspaceEditWithLsp`, `client.ts:925-948`): after the apply, open documents that were deleted or renamed are closed; open documents that changed are refreshed; watched-file notifications go out. Also runs when the apply fails partway, for the prefix that did run.
- **Rename with rollback** (`applyEditsThenRename`, `edits.ts:187-204`): reference files are snapshotted, edited, then the move runs. On move failure, every snapshot is restored.

---

## 8. The mux (lspmux) and the in-house LSP mux daemon

There are two different things, both named "mux".

### 8.1 Third-party lspmux (`lspmux.ts`)

- Only rust-analyzer is wrapped (`DEFAULT_SUPPORTED_SERVERS`, `lspmux.ts:46-49`).
- Detected with `$which("lspmux")`. Liveness is `lspmux status` with a 1 s timeout. Config is parsed from the platform config dir (`%APPDATA%\lspmux\config.toml` on Windows). The result is cached for 5 minutes. `PI_DISABLE_LSPMUX` turns it off.
- Wrapping (`wrapWithLspmux`, `lspmux.ts:189-218`): bare `rust-analyzer` with no args becomes `lspmux` with no args. With args, it becomes `lspmux client -- <args>` with `LSPMUX_SERVER=<original command>`.
- When wrapped, the in-house shared mux is **not** used (`client.ts:1425`: `command === baseCommand` is required).

### 8.2 In-house mux (`mux/`)

**Why it exists** (from `protocol.ts` header and `client.ts:98-103`): one language server per connection gives isolation (each omp session has its own processes), while keeping servers warm after a session ends. A released server lingers for reuse, so the next session that asks for the same key skips cold start. Server identity covers the process, not just the command.

**Startup** (`daemon.ts:236-292`, `ensureLspMuxDaemon`):
1. `daemonClientForProject(projectDir)` gets a broker client. The broker connection doubles as a presence lease, so the daemon stays up while omp is running.
2. Endpoint: `ipcEndpoint(daemonRuntimeDir(projectDir), "lsp-mux")`.
3. If a probe (`omp/muxPing` → `"pong"`, 1.5 s timeout) succeeds, use it.
4. Otherwise up to `ENSURE_ATTEMPTS = 3` rounds: re-probe (a concurrent starter may have won), describe the broker record, wait for readiness if it has not been ready, or stop a wedged record (live record, nothing listening) and retry. Start with `op: "start"`: name `omp.lsp.mux`, the worker command with `LSP_MUX_WORKER_ARG`, env `OMP_LSP_MUX_SOCKET` and `OMP_LSP_MUX_PROJECT_DIR`, `ready.log` pattern `omp lsp mux listening on \S+` (`protocol.ts:26`), 15 s ready timeout, `restart: "no"`, `persist: false`, `detached: false`.
5. The worker (`mux/server.ts:694-717`) reads the env, deletes it, calls `setProcessName`, listens, prints the ready banner, and shuts down on idle (`MUX_IDLE_MS = 15 min`) through `onIdle`.

**Per-connection handshake** (`daemon.ts:197-211` and `mux/server.ts:233-262`):
- The client connects (`IpcConnection.connect`, raw framing, 3 s), then sends `omp/muxConnect` with `{command, args, cwd, env?, initializationOptions?}`.
- The mux validates params (`ConnectParams`, arktype-like `type()`), computes `key = muxServerKey(params)`, and looks for an idle server with the same key (`sessions.size === 0`). If none, it spawns one (`ptree.spawn([command, ...args], {cwd, stdin: "pipe", env: {...Bun.env, ...env}})`).
- Result: `{key, spawned, pid}`. The link is then the LSP transport (`linkTransport`, `daemon.ts:142-192`). A close maps to exit code 0, so the client's crash path handles a dead mux like a dead server.
- Links that did not bind return `session already bound` (`-32600`), `muxConnect must be first` (`-32002`), or `invalid mux connect params` (`-32602`).

**`muxServerKey`** (`mux/protocol.ts:114-123`): `sha256:` of `stableStringifyJson([command, args, cwd, env ?? {}, initializationOptions ?? {}])`. Key order is normalized at every depth. The raw env and init options are hashed, so secrets do not appear in logs or the handshake result. Test coverage: `test/lsp/mux-server-key.test.ts` (10 tests).

**Protocol relay** (`mux/server.ts`):
- `initialize` (request): the first session sends it to the server, with `processId` overwritten by the mux's pid so servers that self-terminate on a dead client pid do not die. The result is cached and returned to later sessions (`initializeCached`). Concurrent `initialize` requests wait on one in-flight call (`initializeWaiters`).
- `initialized` (notification): forwarded once per server. Later sessions get `replayState` instead: dynamic registrations, last diagnostics per URI, and in-progress `$/progress` tokens.
- `shutdown` (request): answered locally by the mux with `null`. `exit` (notification): destroys the link. The server stays up.
- `textDocument/didOpen`, `didChange`, `didClose`: forwarded; the mux tracks open URIs per session and per server.
- `workspace/didChangeWorkspaceFolders`: tracked per session and forwarded.
- `$/cancelRequest`: the id is rewritten to the mux id.
- Other requests: the client id is rewritten to a mux id (`server.muxId()`) so ids from different sessions cannot collide. The response is routed back to the original session.
- Server notifications: `publishDiagnostics` is stored (for replay) and broadcast to every initialized session. `$/progress` is tracked and broadcast.
- Server requests: `client/registerCapability` and `client/unregisterCapability` are recorded and sent to every initialized session. Client-side requests (such as `workspace/applyEdit`) go to **one** session: the most recently active one. With no session, the mux answers `-32601 no client attached`.
- Custom `omp/muxRestartServer` (notification): kills the server regardless of linger.

**Lifecycle** (`mux/server.ts`):
- When a session closes (`#closeSession`, `:586-635`): orphan `didClose` for every URI it still had open, `$/cancelRequest` for its pending requests, and `workspace/didChangeWorkspaceFolders` removal for its added folders. The server then lingers `SERVER_LINGER_MS = 5 min` if no session remains, and is stopped when the timer fires.
- Server exit (`#serverExited`): all bound sessions are destroyed. Their clients see the link close as a process exit.
- Initialize error: the server is killed.
- Stop (`#stopServer`): `shutdown` with a 2 s budget, then `exit`, then kill.
- Shutdown of the mux (`#performShutdown`): destroys every session, waits for every child to exit, closes the listener.

**Failure modes and fallbacks**:
- Any failure in `connectSharedLspTransport` returns null, logs at debug, and the caller spawns locally (`daemon.ts:299-326`). Only an abort is rethrown.
- Connect timeout 3 s; handshake timeout 10 s; probe 1.5 s; ready 15 s.
- Framing resync on bad headers (`LSP mux client framing resync`); malformed JSON from a session is logged and skipped; a session framing error destroys its connection.
- `daemon.ts` `smokeTestLspMux` (`cli.ts:157`) spawns the worker and waits for a ping. It is a distribution smoke test.

**Things I could not verify**: socket permissions and who may connect; behavior of `IpcServer` when a client sends an oversized frame (`max_frame_bytes` is in the Rust code, not checked here); how the broker reclaims the mux daemon when the last omp exits (`launch/*`, not read).

---

## 9. Non-LSP linter clients

All three implement `LinterClient` (`types.ts:304-313`): `format(filePath, content) → Promise<string>`, `lint(filePath, signal?) → Promise<Diagnostic[]>`, optional `dispose()`. A server is marked as a custom linter by `ServerConfig.createClient` (`types.ts:368`), set only in `applyRuntimeDefaults` for `biome` and `swiftlint`. Custom linters are excluded from `getLspServers`, so warmup, `capabilities`, and workspace-symbol search never start them.

**Biome** (`clients/biome-client.ts`):
- `lint(file)`: `biome lint --reporter=json <file>` in the project cwd. Exit status is ignored when stdout is non-empty (Biome exits non-zero when it finds issues). Only an empty stdout counts as a run failure, logged once per cwd (`warnBiomeOnce`).
- Parse: `diagnostics[]` with `location.path` (relative to cwd), `start` and `end` (1-based `line`, `column`). Only diagnostics for the target file are kept. Severity maps `error`→1, `warning`→2, `info`→3, `hint`→4. `source: "biome"`, `code: category`.
- Schema drift: if Biome returns diagnostics with no usable location, a one-time warning is logged instead of reporting "no issues" (`biome-client.ts:212-218`).
- `format(file, content)`: **writes content to disk** (`Bun.write`), runs `biome format --write <file>`, reads the file back. Returns the original content if the command fails. This does touch disk directly and bypasses the writethrough. The writethrough already writes the content first in the custom-formatter path, so the net effect is the same there.

**SwiftLint** (`clients/swiftlint-client.ts`):
- `lint(file)`: `swiftlint lint --quiet --reporter json <file>`. Success means non-empty stdout. Violations are parsed as JSON array items `{line, character, reason, rule_id, severity: "Error"|"Warning"}`. Each becomes a zero-width diagnostic at `(line-1, character-1)`, clamped at 0. Severity: Error→1, else 2. `source: "swiftlint"`, `code: rule_id`.
- `format`: returns the content unchanged.

**`LspLinterClient`** (`clients/lsp-linter-client.ts`): wraps `getOrCreateClient` as a `LinterClient`. `format` syncs content and calls `textDocument/formatting`. `lint` sends `didSave` then polls the diagnostics map for 3 s. **Reachability**: `getLinterClient` is only called from code that first checks `serverConfig.createClient` (`tool.ts:383`, `diagnostics.ts:645`, `diagnostics.ts:780`). So for configs in the repo this class is never constructed. It only matters if a future server config sets `createClient` to nothing, or an embedder calls `getLinterClient` directly.

**Caching**: `getLinterClient` caches by `name:cwd` (`clients/index.ts:22-40`). `clearLinterClientCache` has no callers.

**Plug-in model**: `createClient` is a `LinterClientFactory` `(config, cwd) => LinterClient`. A new linter is a factory plus an entry in `defaults.json` with `isLinter: true`. It is then routed through `getServersForFile`, `splitServers`, and `formatContent`, which prefers linters for formatting (`diagnostics.ts:773-775`).

---

## 10. Performance and safety tricks worth copying

Each item is followed by its location.

**Concurrency and identity**
- Identity-keyed clients (`clientKey`, `client.ts:1248`): command, args, cwd, init options, settings and language id together.
- In-flight dedupe for client creation (`clientLocks` with a token) and per-document operation locks (`fileOperationLocks`, keyed `client:uri`) so concurrent opens, syncs, and refreshes serialize (`client.ts:1682`, `:1805`, `:2156`).
- Reload barrier per cwd (`clientReloadBarriers`, `invalidatedClientKeys`): a superseded server is confirmed dead before a new one with the same key starts (`client.ts:1268-1330`).
- Write queue per client (`writeQueue`), with signal-bounded drains and wedged-client teardown (`client.ts:406-504`).

**Caching and staleness**
- Config cache per cwd (`configCache`), linter client cache per name+cwd.
- Negative init-failure cache for 3 min (`client.ts:89`).
- Stat-stamp skip before reading disk for reconcile (`diskStamp`, `client.ts:1775-1782`, `:1822-1824`).
- `Bun.hash` signature of the document text to detect divergence (`syncedHash`, `disk.hash`).
- Diagnostics version counters (`diagnosticsVersion`, `minVersion`, `expectedDocumentVersion`).
- Per-file mutation version for late diagnostics (`DeferredDiagnostics`, `isStale`).
- Diagnostics ledger for dedupe across edits (`diagnostics-ledger.ts`).
- Stale diagnostics dropped when a file changes (`client.diagnostics.delete(uri)` in `syncContent`, `refreshDocument`, `notifyWorkspaceWatchedFiles`).

**Bounding time**
- Per-server pipeline budget with a grace period (`DIAGNOSTICS_PIPELINE_GRACE_MS = 10 s`, `diagnostics.ts:74`, `:638-667`).
- Writethrough 5 s operation timer (`writethrough.ts:471`), 500 ms inline wait, 12 s deferred budget (`diagnostics.ts:61`).
- Per-request timeout with `$/cancelRequest` (`client.ts:2470-2478`).
- Watched-file notify timeout 2 s (`client.ts:2040`).
- `didClose` delivered after abort, within 2 s (`DELIVER_QUEUED_TIMEOUT_MS`, `client.ts:464`).
- Rust project-ready polling with settle, and a status-request timeout (`client.ts:1089-1093`).
- Project-load fallback timer 15 s (`PROJECT_LOAD_TIMEOUT_MS`, `client.ts:1172`), extended by each new load.

**Batching and size limits**
- Multi-call batch flush (`batch.ts`, `writethrough.ts:163-190`, `:620-676`): one LSP post-process per file per model response.
- Diagnostic messages capped at 50 (`DIAGNOSTIC_MESSAGE_LIMIT`, `diagnostics.ts:41`). Not applied in the `diagnostics` action.
- Glob targets capped at 20 (`MAX_GLOB_DIAGNOSTIC_TARGETS`, `diagnostics.ts:85`).
- References: first 50 with context (`REFERENCE_CONTEXT_LIMIT`, `diagnostics.ts:167`).
- Workspace symbols: 200 (`WORKSPACE_SYMBOL_LIMIT`, `diagnostics.ts:86`).
- Rename directory: 1000 files (`MAX_RENAME_PAIRS`, `tool.ts:118`).
- CLI checker output: 50 lines per checker; checkers run 2 at a time (`workspace-diagnostics.ts:19`, `:233`).
- Request params echoed in errors: 400 chars (`tool.ts:1032`).
- Code-action `codeAction/resolve` only when the edit is missing (`utils.ts:550`).

**Safety**
- Rename aborts before any mutation when a relevant server supports `willRenameFiles` and fails (issue #8380, `tool.ts:746-764`).
- Rename rollback on move failure (`edits.ts:187-204`).
- Overlap validation for all edit batches before any write (`edits.ts:316-332`, `tool.ts:829-833`).
- Snippet-format edits rejected (`edits.ts:84-90`).
- Symlink-aware path checks; `resolveSyscallTarget` (`client.ts:707`).
- Special files refused for reads (FIFO, device, socket) in `readTextFromDisk` (`utils.ts:24-28`).
- Writes never lost to LSP failure: LSP errors are swallowed and the write commits anyway (`writethrough.ts:562-584`).
- Mux: env and init options hashed in the key (`protocol.ts:114-123`). Frames resynchronized on bad headers. Client requests routed to one session only.
- Process exit cleanup through `postmortem` and `beforeExit` (`client.ts:2602-2607`).

**File watching**: the module installs **no** filesystem watcher (grep for `fs.watch`, `chokidar`, `watch(` found nothing in `src/lsp`). Changes are announced by harness tools: write, edit (including native `move` and `delete`), rename, and the ACP bridge (`notifyWorkspaceWatchedFiles`, `workspace/didChangeWatchedFiles`). External edits are detected lazily: stat-stamp plus hash reconcile before each query (section 5.4). The only timer in the module is the optional idle sweep.

---

## 11. TypeScript-only versus Rust-backed

**TypeScript-only** (no native call):
- Everything in `src/lsp/` except the imports listed below: the JSON-RPC client and reader, config loading and merge, root-marker logic, tool, diagnostics waiting and formatting, writethrough, text and workspace edit application, format-option detection, the ledger, the mux protocol, server and daemon client, lspmux wrapping, biome and swiftlint parsers and CLI calls, startup events, settings.
- `src/jsonrpc/message-framing.ts` (`MessageFramer`, `encodeMessageFrame`). Not read in full, but it is in TypeScript.
- Linter and checker CLIs are external binaries, invoked with `Bun.spawn`.

**Rust-backed through `@oh-my-pi/pi-natives`** (NAPI, `crates/pi-natives`):
- `glob` (`crates/pi-natives/src/glob.rs:144`, signature `glob<'env>`). Used by `listCargoManifests` (`client.ts:1113-1119`, gitignore on) and by path utilities.
- IPC: `IpcServer` (`listen`, `close`, `endpoint`), `IpcConnection` (`connect`, `start`, `start_raw`, `send`, `drained`, `end`, `destroy`, `closed`), and `ipcEndpoint` (`crates/pi-natives/src/ipc/mod.rs:149`, `:196`, `:240`, `:338`, `:372`, `:388`, `:407`, `:441`, `:452`, `:459`, `:477`; platform files `ipc/unix.rs` and `ipc/windows.rs`). Used by `mux/server.ts` (`IpcServer.listen`) and `mux/daemon.ts` (`IpcConnection.connect`, `ipcEndpoint`). The `raw` framing mode and `max_frame_bytes` are in Rust.
- `Process` class used by `ptree` (`packages/utils/src/ptree.ts:11` imports `Process` from `pi-natives`). `ptree.spawn` is what starts every language server child (`client.ts:1428`, `mux/server.ts:103`). I did not trace the Rust symbol behind `Process` (the process-tree code in `crates/pi-shell/src/proctree.rs` is a likely candidate, but I did not verify it).

**Rust crates that mention LSP**: no LSP protocol code found. `pi-natives/src/edit.rs` and `pi-edit/src/session.rs` only carry the LSP batch flush flag through the native edit session (comments at `edit.rs:9,195`, `session.rs:64`). `pi-builtins/src/ast_grep.rs:39` lists `lsp` as an unsupported ast-grep subcommand, which is unrelated.

**Other packages used**: `@oh-my-pi/pi-tui` (TS: `tools/lsp` types and `FileFormatResult`, theme icons, `lang-from-path` for `detectLanguageId`, render helpers), `@oh-my-pi/pi-utils` (TS: logger, `postmortem`, `$which`, `isEnoent`, `stableStringifyJson`, `getEditorConfigFormatting` in `packages/utils/src/tab-spacing.ts:296`), `@oh-my-pi/omptype` (TS schema).

**Bun built-ins used heavily**: `Bun.spawn`, `Bun.spawnSync`, `Bun.file`, `Bun.write`, `Bun.hash`, `Bun.SHA256`, `Bun.pathToFileURL` / `fileURLToPath`, `Bun.glob`, `Bun.sleep`, `Bun.env`, `Bun.which`-like `$which`, and Bun's YAML and TOML parsers. A Tauri port would need replacements for all of these.

---

## 12. Exact snippets where the logic is not obvious

Snippets are short and quoted from the files as read.

**a) Idle guard** (`client.ts:188-191`):
```ts
export function isIdleClient(client: LspClient, now: number, timeoutMs: number): boolean {
	if (client.pendingRequests.size > 0) return false;
	return now - client.lastActivity > timeoutMs;
}
```
Why it matters: without the in-flight check, a long request could look idle and get its client torn down mid-flight (issue #8390). `lastActivity` is also refreshed when a request settles, not only when it is sent (`client.ts:2494-2508`).

**b) Client identity key** (`client.ts:1248-1257`): `${spawnCommand}:${cwd}:${stableStringifyJson([args, initOptions, settings, languageId])}`. Two configs that name the same binary differently still match, while the same name resolving to different binaries does not.

**c) Acceptance rule for a push-only report** (`diagnostics.ts:368-389`, simplified): accept a publish that matches the exact expected document version immediately. For an unversioned publish, accept only when it has been the newest for `settleMs` **and** it is not a provisional cold publish. Empty unversioned publishes from a server started under 3 s ago are assumed to be placeholders.

**d) Pull retry classification** (`diagnostics.ts:286-291`): `ContentModified` (-32801) always retries. `ServerCancelled` (-32802) retries unless `data.retriggerRequest === false`.

**e) Format options precedence** (`format-options.ts:127-136`): `tabSize: fromConfig.tabSize ?? detected.tabSize ?? 2`, `insertSpaces: fromConfig.insertSpaces ?? detected.insertSpaces ?? true`. Editorconfig wins, then content sniffing, then the fallback. Sniffing returns `undefined` for fields the content does not pin, so a higher-precedence source is never overwritten.

**f) Diagnostics ledger identity** (`diagnostics-ledger.ts:4-8`): `^.*?:\d+:\d+\s+` is stripped from each message, so an error keeps its identity when its line moves.

**g) Mux server key** (`mux/protocol.ts:114-123`): `Bun.SHA256.hash(stableStringifyJson([command, args, cwd, env ?? {}, initializationOptions ?? {}]), "hex")`, prefixed `sha256:`.

**h) Config server-name quirk** (`client.ts:814-822`, `handleConfigurationRequest`): `workspace/configuration` returns `client.config.settings[section]` as an exact top-level key. Servers that ask for dotted sections such as `python.analysis` get `null` from this code. Worth fixing in a port by walking dotted paths.

---

## 13. Things that do not exist, look wrong, or are unclear

- **`clearLinterClientCache`** (`clients/index.ts:45`): exported, no callers.
- **`LspLinterClient`** (`clients/lsp-linter-client.ts`): not reachable in the shipped config (section 9). The name `lsp-linter` in the request is this class; there is no separate `lsp-linter` server entry.
- **`biome` `args: ["lsp-proxy"]`** in `defaults.json`: dead, because `createClient` replaces the spawn path.
- **Orphaned doc comment** (`client.ts:1103-1113`): the JSDoc for `isRustAnalyzerClient` sits above `listCargoManifests`, and `isRustAnalyzerClient` has its own doc further down (`:1103-1108`, `:1121`). Cosmetic.
- **`diagnostics` action without the 50-message cap** (section 4 vs section 6.5).
- **Workspace configuration lookup is by exact key** (`client.ts:818-820`). See snippet h.
- **`getActiveClients`** reports `name = client.config.command`, and `status` matches on command, so two configs with the same command collapse in status output (`tool.ts:274-281`).
- **Writethrough swallows LSP-phase errors silently** (`writethrough.ts:562`). Intentional (the write must land), but the user sees only a missing diagnostics line.
- **`biome` `format` writes to disk** outside the writethrough (section 9).
- **Mux trust boundary**: any client on the mux endpoint can spawn an arbitrary command (`mux/server.ts:238-258`). Mitigation, if any, is the endpoint permissions. Not verified.
- **`edit` and `write` argument split**: the native edit engine gets `lspBatchId`/`lspFlush` but the LSP work is in TypeScript (section 1). A port must keep that split or move the writethrough into the Rust edit engine.
- **Not found**: any file-system watcher, any incremental `didChange` (every change is full text), any server-side `semanticTokens`, `inlayHint`, `codeLens`, `foldingRange`, `completion`, `signatureHelp`, or `documentLink` requests. `workspace/semanticTokens/refresh` and the others are acknowledged but never requested.
- **Not found**: `textDocument/prepareRename` (rename advertises `prepareSupport: true` but never calls it), `willSave` / `willSaveWaitUntil`, `workspace/willCreateFiles`, `workspace/willDeleteFiles`.
- **Idle reaping** is opt-in and off by default, so an unused client stays alive for the life of the process unless the mux reclaims it.

---

## 14. Tests

Test names and what they check. All are in `packages/coding-agent/test/lsp/`.

- **`edits.test.ts`** (2): "applies reference edits and moves the source when the move succeeds"; "rolls back reference edits when the move fails". Covers `applyEditsThenRename`.
- **`idle.test.ts`** (5, issue #8390): "never reports a client with an in-flight request as idle"; "reports a quiet client as idle once the window elapses"; "keeps a client below the idle window alive"; "refreshes lastActivity when a request settles so a fresh answer is not reaped"; "refreshes lastActivity when a request settles with an error". Covers `isIdleClient` and `sendRequest` bookkeeping with a hand-built client double.
- **`issue-4910-diagnostics-bounded.test.ts`** (3): "a linter that never settles cannot hang getDiagnosticsForFile"; "passes the pipeline deadline to cancellable linter work"; "a healthy linter still reports normally under the same budget". Covers the pipeline budget.
- **`lspmux.test.ts`** (5): Windows and POSIX path recognition for rust-analyzer; rejection of gopls; wrapping of an absolute Windows path (`client` subcommand with `LSPMUX_SERVER`); bare rust-analyzer uses the default lspmux entry.
- **`mux-server-key.test.ts`** (10): stable across equal values; separates by command and cwd; by args; split vs joined args; by env; env values not exposed in the key; env order irrelevant; absent env equals empty; init-option key order irrelevant at every depth; absent init options equal `{}`.
- **`reconcile-open-files.test.ts`** (6): re-syncs an edited open document, then stays quiet; re-syncs an unchanged file when the server's hash diverged; leaves matching documents alone; closes a document deleted outside OMP; throws on an unreadable document unless told to close it; `refreshFile` keeps a missing document open while an OMP write is pending.
- **`reload-configuration-params.test.ts`** (2): reload echoes configured settings (issue #8383); falls back to `{}`.
- **`shared-client-log-context.test.ts`** (1): "names no session on records of an LSP client shared across sessions". Uses `test/fixtures/fake-lsp-server.ts`, which I did not read.

Adjacent tests outside `test/lsp/` (names only, from grep; not read): `lsp-format-options.test.ts`, `lsp-mux.test.ts`, `lsp-mux-reload.test.ts`, `lsp-writethrough-gate.test.ts`, `sdk-shared-lsp-binding.test.ts`, `thread/lsp-mode.test.ts`, `interactive-mode-lsp-startup.test.ts`, `workspace-diagnostics.test.ts`, `biome-client.test.ts`, `tools/lsp-batching.test.ts`, `tools/lsp-diagnostics-dedup.test.ts`, `tools/lsp-diagnostics-freshness.test.ts`, `tools/lsp-regressions.test.ts`, `tools/tool-timeouts.test.ts`, `edit-auto-repair-cost-gate.test.ts`, `edit-patch-unchanged-error.test.ts`, `sdk-file-write-fallback-extension.test.ts`.

**Coverage gaps in `test/lsp/`**: `tool.ts`, `writethrough.ts`, `diagnostics.ts` waiting logic, `config.ts` merge and TypeScript selection, `mux/server.ts` relay, `mux/daemon.ts`, and the two CLI linter clients have no direct tests in this folder. Several of these may be covered by the adjacent files above (which I did not read).

---

## 15. Porting checklist (short)

1. JSON-RPC over Content-Length stdio (`MessageFramer`, `client.ts:514-668`). Single write queue per client.
2. Client registry keyed by identity, with dedupe, reload barrier, negative cache.
3. Document sync as full-text `didOpen`/`didChange`/`didSave`, plus stat-and-hash reconcile before queries; 5-minute idle close.
4. Diagnostics: push plus pull, version-aware acceptance, settle window, provisional-cold-publish rule, unreported vs clean distinction.
5. Writethrough: inline 500 ms, deferred 12 s, 5 s operation budget, errors swallowed, write always commits, batch flush per model response.
6. Edits: bottom-to-top application, overlap validation, workspace-edit planning with resource ops, rename rollback, overlay reconciliation after edits.
7. Config: defaults table, shallow merge, root-marker and binary filter, TypeScript server selection, local bin dirs.
8. Mux: optional. Needs IPC, a handshake keyed by a hash of identity, linger, replay of diagnostics and registrations, one-session routing for client requests. If not ported, the fallback (private spawn) is complete on its own.
9. Linters: keep them as CLI-backed `LinterClient`s, not LSP. Biome and SwiftLint are simple.
10. Tool surface: 14 actions, read or write tiers, budgets, output caps as listed in section 4.

Files: all paths above are in `C:\Users\v3dqn\Work\Project Versa\oh-my-pi-breakdown` (`packages/coding-agent/src/lsp/...`, `packages/coding-agent/src/edit/index.ts`, `packages/coding-agent/src/tools/write.ts`, `packages/coding-agent/src/sdk.ts`, `packages/coding-agent/src/thread/services/lsp.ts`, `docs/lsp-config.md`, `crates/pi-natives/src/ipc/mod.rs`, `crates/pi-natives/src/glob.rs`).
