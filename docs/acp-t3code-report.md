# t3code: how Claude runs over ACP

Repo: `C:\Users\v3dqn\Work\Project Versa\t3code` (read-only; nothing was edited or installed there).
Stack: TypeScript monorepo (pnpm), Effect (`effect` v4 style: `Context.Service`, `Effect.fn`, `Schema`), Node 26 per `package.json` engines. Desktop shell is Electron (`apps/desktop`); server is `apps/server`. Protocol transport is ACP JSON-RPC over stdio, NDJSON framed.

**Coverage caveat.** This is a partial read, not the full read requested. Coverage is in section 13. The biggest gap is `packages/provider-acp/src/server/adapter.ts` (9,234 lines): about 15-20% was read by range, the rest was grepped.

---

## 0. Which Claude path is which (read this first)

There are two Claude integrations in this repo. Only one is ACP.

| Path | Entry | Transport | Files |
|---|---|---|---|
| **ACP (this report)** | generic `acpRegistry` provider driver, `agentId: "claude-acp"` or local command basename `claude-agent-acp` | spawns external `claude-agent-acp` process, ACP v2 JSON-RPC over stdio | `packages/provider-acp-registry/**`, `packages/provider-acp/**`, `packages/effect-acp/**` |
| **Native (NOT ACP)** | `apps/server/src/provider/Drivers/ClaudeDriver.ts`, `apps/server/src/provider/ClaudeProvider.ts` | in-process `@anthropic-ai/claude-agent-sdk` (`query`, `forkSession`, `resolveSettings`) | `apps/server/src/orchestration-v2/Adapters/ClaudeAdapterV2.ts` and related |

Evidence:
- `apps/server/src/provider/builtInDrivers.ts:2-4` imports `AcpRegistryDriver` from `@t3tools/provider-acp-registry/server`, separately from the Claude driver.
- `apps/server/src/orchestration-v2/Adapters/ClaudeAdapterV2.ts:62` imports from `"@anthropic-ai/claude-agent-sdk"`. `ProviderOrchestrationAdapterInfrastructure.ts:3,12,21` wires `ClaudeAdapterV2` (SDK path).
- `docs/user/providers-acp.md:8-10` says Claude is "supported as [its] own provider... Enable them in Settings -> Providers" and "don't appear in the Registry list". Yet `packages/client-runtime/src/providerClients.ts:31` has `acpRegistryAgentIds: ["claude-acp"]`, so the ACP route also exists.
- `claudeUsageLimits.ts` is imported only by `apps/server/src/provider/Drivers/ClaudeDriver.ts:26` (native) and `apps/server/src/usage/cliproxyApi.ts:18`. **Not on the ACP path.**
- `ClaudeTurnTokenUsage.ts` is imported by `ClaudeAdapterV2.ts:26-33` (native). **Not on the ACP path.**
- `claudeAcp.ts` has no unit test. A grep for `claudeAcp|extractClaudeSubagent|isClaudeProvisional` in `packages/**` and `apps/**` `*.test.ts` returned nothing.

The port must decide which path it is copying. This report covers the ACP one only.

---

## 1. File map

| File | Lines | Role | Read |
|---|---|---|---|
| `docs/user/providers-acp.md` | 144 | User docs; ACP v2 default for Claude, `CLAUDE_AGENT_ACP_EXPERIMENTAL_V2=0` falls back to v1 (lines 17-19). | full |
| `packages/provider-acp-registry/src/settings.ts` | 70 | `AcpRegistrySettings`: `source` (registry/local), `agentId`, `commandPath`, `commandArgs`, `authMethodId`, `distribution` (auto/binary/npx/uvx), `customModels`. | full |
| `packages/provider-acp-registry/src/server/claudeAcp.ts` | 117 | Claude-only helpers: Agent/Task subagent extraction, `_t3_unknown` `subagent_update` and `session_message` decoding. | full |
| `packages/provider-acp-registry/src/server/adapter.ts` | 362 | Registry driver wiring. Detects Claude (`isClaude`), sets env default, sets Claude flavor flags, builds the ACP adapter. | full |
| `packages/provider-acp-registry/src/server/AcpRegistrySupport.ts` | 2020 | Registry index fetch and cache, npx/uvx/binary install, locks, receipts, `resolve()` (produces spawn spec), `inspect()`. | full |
| `packages/provider-acp-registry/src/server/driver.ts` | 920 | Provider status and readiness snapshots. Read only ranges 246-260 and 379-470. | partial |
| `packages/provider-acp-registry/src/server/probe.ts` | 789 | Probe (initialize plus test session), session list/delete, provider and logout management. Grepped. | grep only |
| `packages/provider-acp-registry/src/server/auth.ts`, `authenticationState.ts`, `AcpRegistryRuntimeCoordinator.ts` | 358 / 82 / 357 | Sign-in state, URL elicitation publication, live command and config publication. | not read |
| `packages/provider-acp/src/server/AcpSessionRuntime.ts` | 2947 | Owns the child process, initialize, auth, new/load/resume, prompt dispatch, cancel, session updates, termination. | ranges 1-230, 1285-1484, 1596-2730, 2730-2947 |
| `packages/provider-acp/src/server/adapter.ts` | 9234 | `makeAcpAdapterV2`: turn lifecycle, prompt building, permission/elicitation callbacks, tool/plan/usage projection, MCP context, stop/close. | ranges 141-330, 520-530, 812-880, 1272-1406, 2280-2420, 4860-5560, 6360-6560, 7690-7840, 8120-8270, 8640-8660, 9080-9100, plus greps |
| `packages/provider-acp/src/server/runtimeModel.ts` | 1620 | `parseSessionUpdateEvent` (session update to event mapping), tool-call state merge, text extraction. | 1363-1620 full; rest grepped |
| `packages/provider-acp/src/server/clientPolicy.ts` | 250 | Permission disposition (`allow`/`ask`/`deny`) from runtime policy and tool kind; command-approval grants. | full |
| `packages/provider-acp/src/server/mcpOverAcpBridge.ts` | 242 | Bridge that serves the T3 MCP endpoint over ACP (`acp` MCP transport). Header only. | header |
| `packages/provider-acp/src/server/clientTerminals.ts` | 428 | Client-side terminal handling. Only Devin enables it. | not read |
| `packages/provider-acp/src/server/sessionConfig.ts` | 144 | Session config options helpers. | not read |
| `packages/provider-acp/src/server/stderr.ts` | 36 | Bounded stderr tail, sanitisation. | not read |
| `packages/provider-acp/src/server/nativeLogging.ts` | 76 | Native request and protocol logging factory. | not read |
| `packages/effect-acp/src/_internal/stdio.ts` | 75 | Child stdio to ACP stream (stdin/stdout NDJSON). | full |
| `packages/effect-acp/src/client.ts` | 1407 | ACP client: request/notification dispatch, `_t3_unknown` fallback (lines 291, 304, 500, 520), permission and elicitation handler registration. | grep only |
| `packages/effect-acp/src/protocol.ts` | 1042 | JSON-RPC protocol, NDJSON, method table. | grep only |
| `packages/effect-acp/src/compat.ts` | 457 | v1/v2 schema bridge. `_t3_unknown` union members at 18, 266, 369. | grep only |
| `packages/effect-acp/src/errors.ts` | 454 | `AcpRequestError`, `AcpTransportError`, `AcpProcessExitedError`, `AcpSpawnError`. | grep only |
| `packages/effect-acp/src/rpc.ts`, `agent.ts`, `_generated/*.gen.ts` | 410 / 563 / large | RPC schema, agent side (mock), generated method schemas. | grep only |
| `packages/shared/src/AgentScope.ts` | - | `AgentScope.wrap` process containment hook (default is pass-through). | grep only |
| `packages/provider-core/src/server/runtimeInstructions.ts`, `orchestrationInstructions.ts` | - | T3 instruction text injected into prompts. | grep only |
| `apps/server/src/orchestration-v2/Adapters/AcpRegistryAdapterV2.test.ts` | - | Claude replay fixtures (ACP v2, `claude-agent-acp 0.90.0`), launch-env test. | partial |
| `apps/server/src/orchestration-v2/Adapters/AcpAdapterV2.test.ts` | - | ACP adapter tests (generic). | grep only |
| `apps/server/scripts/acp-mock-agent.ts` | - | Mock ACP agent for tests. | not read |

---

## 2. Process launch

### 2.1 Where the command comes from

Registry source (default). `packages/provider-acp-registry/src/server/AcpRegistrySupport.ts:51`:
```ts
const ACP_REGISTRY_URL = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
```
HTTPS is required (`assertHttpsUrl`, line 797). The index is cached at `<cacheDir>/acp-registry/registry.json` (`readCachedRegistry`, lines 777-779). Fetch limits: 1 MiB body (line 53), 30 s request and read timeouts (line 61, `REGISTRY_REQUEST_TIMEOUT`). On network failure `refreshRegistry` falls back to the disk cache (lines 867-879). `search` reuses the index for 5 min (line 63). `prepare` always refetches.

**Version pinning.** The app does not pin `claude-agent-acp`. It takes whatever version the registry index lists as latest. The pin is only enforced inside the registry entry itself:
- npx packages must be exact: `EXACT_NPX_PACKAGE` (lines 86-90) accepts `name@x.y.z` (optional `v`, prerelease, build). Ranges are rejected by schema.
- uvx packages must be exact: `EXACT_UVX_PACKAGE` (line 91), `name==x.y.z` or `name@x.y.z`.
- binary archives may carry `sha256` (line 137). It is verified while streaming (lines 1485-1530). If absent, only HTTPS is the guarantee.

**Distribution choice.** `resolveAcpRegistryDistribution` (lines 423-462). With `distribution: "auto"` it tries binary, then npx, then uvx (line 433). Binary keys are `darwin|linux|windows` x `aarch64|x86_64` (`resolveAcpRegistryPlatformTarget`, lines 234-248). `windows-aarch64` falls back to `windows-x86_64` (lines 255-257).

**npx install (`ensureGlobalPackage`, lines 1224-1327).**
- Manager must be on PATH: `resolveExecutable(managerName, platform, environment)`. Missing gives `runner_unavailable` (lines 1230-1237).
- Install root: `<toolsDir>/<agentId>/<encodeURIComponent(version)>/npm` (lines 660-667). `toolsDir` is `<baseDir>/tools` (line 538).
- Command: `runCommand(managerPath, ["install", "--global", packageSpec], { env: installEnvironment, timeout: PACKAGE_INSTALL_TIMEOUT })` (lines 1282-1287). The env sets `npm_config_prefix = packageRoot` (line 1254).
- Timeouts: `PACKAGE_INSTALL_TIMEOUT = "20 minutes"` (line 65), `PACKAGE_QUERY_TIMEOUT = "30 seconds"` (line 66).
- The installed command is discovered from the global `package.json` bin map (`npmCommandName`, lines 1099-1115, `discoverNpmGlobalPackage`, lines 1117-1176). Receipt written to `<cache>/acp-registry/package-installs/` (lines 1058-1081).

**uvx install (lines 1178-1222, 1289-1293).** `uv tool install --force <spec>` with `UV_TOOL_DIR` and `UV_TOOL_BIN_DIR` (lines 1255-1259).

**Binary install (`installBinary`, lines 1396-1595).** Download with 20 min timeout (line 64). Hash check (line 1522). Archive entries are validated: no absolute paths, no `..`, no drive letters (`normalizeRegistryCommandPath`, lines 560-572; `validateArchiveEntries`, 574-580). Extracted to a temp dir, then `rename` into `<toolsDir>/<agent>/<version>/<platform>` (lines 1579-1580). `chmod 0o755` on non-Windows (line 1577).

**Install lock.** `<installRoot>.lock` via `flag: "wx"` (lines 1329-1354). Stale after 5 min (line 553). 300 retries x 100 ms (lines 551-552). Per-agent semaphore `withInstallPermit` (lines 675-680), so long installs block only the same agent.

**Spawn spec (`resolve`, lines 1827-1919).**
- Registry path, no override (lines 1877-1887 and 1899-1917):
  ```ts
  const baseSpawnEnvironment = { ...effectiveEnvironment, ...distribution.env };
  const spawnEnvironment = commandBinDirectory === undefined ? baseSpawnEnvironment
    : withPreferredPath(baseSpawnEnvironment, commandBinDirectory, platform);
  return { agent, distribution: distribution.kind,
    spawn: { command, args, cwd, env: spawnEnvironment } };
  ```
  Note: no `shell` field. See 2.3 for what that means.
- Local path (lines 1829-1849): `spawn: { command, args: settings.commandArgs, cwd, env, shell: false }`.
- Command override (`commandPath`, lines 1866-1876): replaces the executable and keeps the distribution `args` and `env`.

**Claude-specific launch tweak** (`packages/provider-acp-registry/src/server/adapter.ts:195-210`):
```ts
const isClaude =
  registryAgentId === "claude-acp" ||
  (options.settings.source === "local" &&
    /^claude-agent-acp(\.exe|\.cmd)?$/i.test(
      options.settings.commandPath.split(/[\\/]/).pop() ?? "",
    ));
...
const runtimeOptions = isClaude
  ? { ...options, environment: { ...options.environment,
        CLAUDE_AGENT_ACP_EXPERIMENTAL_V2: options.environment.CLAUDE_AGENT_ACP_EXPERIMENTAL_V2 ?? "1" } }
  : options;
```
So a Claude child gets `CLAUDE_AGENT_ACP_EXPERIMENTAL_V2=1` unless the instance environment already sets it. The test `AcpRegistryAdapterV2.test.ts:1031-1043` asserts `"1"` by default, `"1"` for the local path `/opt/bin/claude-agent-acp`, and `"0"` when overridden.

### 2.2 Windows handling

- `readEnvironmentPath` (lines 302-311) matches `PATH` case-insensitively on win32.
- Local `.cmd`/`.bat` are rejected (lines 1702-1711):
  > "Local ACP commands launch without a shell. For a Windows batch wrapper, configure the underlying executable (for example node.exe) and pass the script path as an argument."
- npm global bin dir on win32 is the prefix itself, not `prefix/bin` (lines 1156, 665-667).
- Windows process tree kill uses `taskkill /PID <pid> /T /F` (`terminateWindowsProcessTreeWithTaskkill`, `AcpSessionRuntime.ts:1026-1045`). It runs only when `ownDetachedProcessGroup === true` (see 8).

### 2.3 Spawn in the runtime (`AcpSessionRuntime.ts`)

- `spawnCommand` (lines 1374-1380). If `options.spawn.shell === false` it is used as-is. Otherwise `resolveSpawnCommand(command, args, { env, extendEnv: true })` runs (from `@t3tools/shared/shell`). **So Claude's registry spawn (no `shell` field) goes through shell-resolution, while the local-command spawn is `shell:false`.**
- AgentScope containment (lines 1417-1433): `agentScope.wrap({ command, args, name: "acp", threadId, env })`. The default implementation is pass-through (`packages/shared/src/AgentScope.ts:48`), so the wrap is a hook, not a sandbox, unless another layer supplies one.
- Spawn (lines 1450-1471):
  ```ts
  const child = yield* spawner.spawn(
    ChildProcess.make(containedSpawnCommand.command, containedSpawnCommand.args, {
      ...(options.spawn.cwd ? { cwd: options.spawn.cwd } : {}),
      ...(spawnEnvironment ? { env: spawnEnvironment } : {}),
      extendEnv: options.spawn.extendEnv ?? true,
      ...(options.ownDetachedProcessGroup === undefined ? {} : { detached: options.ownDetachedProcessGroup }),
      shell: containedSpawnCommand.shell,
    }),
  )
  ```
- stderr is read into a bounded tail (`appendAcpStderrTail`, `stderr.ts`) and forwarded to `onStderr` (lines 1638-1657).
- Protocol stdio: `EffectAcpClient.layerChildProcess(child, ...)` (lines 1659-1685). Framing is in `effect-acp/src/_internal/stdio.ts:13-22`: child `stdout` is read as the agent's output, `stdin` is written with `TextEncoder`.

### 2.4 Install and readiness check (status probe)

- `inspect()` (`AcpRegistrySupport.ts:1713-1825`) reports `unconfigured | not_found | unsupported | missing_runner | unprepared | ready`. For npx/uvx it checks only that the manager (`npm`/`uv`) is on PATH (lines 1806-1823). It does **not** check that the package is installed. For binary it checks that the executable exists and is valid (lines 1765-1800).
- `checkAcpRegistryProviderStatus` (`driver.ts:379-433`): if `inspect` is not ready, the snapshot is built from that inspection. Otherwise it runs `probeConfiguration(...)`.
- `checkAcpRegistryProviderReadiness` (`driver.ts:435-470`): cheap check only. It returns the message "Checking ACP authentication, models, and commands in the background..." when ready.
- Probe timeout: `PROBE_TIMEOUT_SECONDS = 60` (`probe.ts:46`). Message at `probe.ts:438`: "The ACP agent did not resolve and create a test session within 60 seconds. Package installation or agent startup may be slow; this check retries on the next provider refresh." Management RPC timeout: `MANAGEMENT_TIMEOUT = Duration.seconds(60)` (`probe.ts:462`).

### 2.5 Kill and cleanup

- Normal teardown: `retireRuntime` calls `child.kill({ forceKillAfter: "1 second" })` (`AcpSessionRuntime.ts:2359-2364`). Stderr-failure path does the same (line 1649).
- Scope finalizers run in reverse order: `stopScope` (AgentScope unit), then process-tree or cgroup termination if owned (lines 1430-1434, 1619-1637).
- Process-group termination (`terminateOwnedProcessGroupImpl`, lines ~1600-1615): SIGTERM, grace (default 1 s), then SIGKILL. On Windows it uses `taskkill /T /F` (line 1623).
- **For the registry Claude path, `ownDetachedProcessGroup` is not set anywhere** (grep over `packages/provider-acp/src/server/adapter.ts` and `packages/provider-acp-registry/src/server/*.ts` returned nothing). So for Claude the child is not detached, and the process-tree kill branch at lines 1619-1637 is **not** taken. Only `child.kill` plus the AgentScope stop apply. Grandchildren spawned by Claude are therefore not explicitly reaped by T3 on this path.

---

## 3. Protocol handshake

### 3.1 initialize request

`packages/provider-acp/src/server/AcpSessionRuntime.ts:2085-2100`:
```ts
const initializeOnce = Effect.gen(function* () {
  const initializePayload = {
    protocolVersion: 2,
    clientCapabilities: initializeClientCapabilities,
    clientInfo: options.clientInfo,
    ...(options.initializeMeta === undefined ? {} : { _meta: options.initializeMeta }),
  } satisfies EffectAcpSchema.InitializeRequest;
  const initializeResult = yield* runLoggedRequest("initialize", initializePayload, acp.agent.initialize(initializePayload));
  yield* options.onInitialized?.(initializeResult) ?? Effect.void;
  return initializeResult;
});
```
- `protocolVersion: 2` is hard-coded (the v2 preview). Claude v1 fallback is decided by the agent's reply, not by the client (see 12).
- `initializeMeta` is not set for Claude (grep: no `initializeMeta` in adapter or registry).
- `clientInfo` is `{ name: "t3-code", version: "0.0.0" }` (`adapter.ts:2323`).

### 3.2 clientCapabilities sent

Built in `AcpSessionRuntime.ts:1789-1803`:
```ts
const initializeClientCapabilities = {
  fs: { readTextFile: false, writeTextFile: false, ...options.clientCapabilities?.fs },
  terminal: options.clientCapabilities?.terminal ?? false,
  session: { ...options.clientCapabilities?.session, notices: {} },
  ...(options.clientCapabilities?.auth ? { auth: options.clientCapabilities.auth } : {}),
  ...(options.clientCapabilities?.elicitation ? { elicitation: options.clientCapabilities.elicitation } : {}),
  ...(options.clientCapabilities?._meta ? { _meta: options.clientCapabilities._meta } : {}),
} satisfies NonNullable<EffectAcpSchema.InitializeRequest["clientCapabilities"]>;
```
The adapter sets these for every flavor (`adapter.ts:2314-2322`):
```ts
clientCapabilities: {
  fs: {
    readTextFile: flavor.clientFileSystem !== undefined,
    writeTextFile: flavor.clientFileSystem?.writeTextFile !== undefined,
  },
  terminal: clientTerminals !== undefined,
  elicitation: { form: {}, ...(flavor.onUrlElicitation ? { url: {} } : {}) },
  ...(flavor.clientCapabilitiesMeta ? { _meta: flavor.clientCapabilitiesMeta } : {}),
},
```
For Claude:
- `clientFileSystem` is not set in the registry flavor, so **fs read and write are both `false`**. Claude does its own file IO and reports it through session updates.
- `clientTerminals` is set only for Devin (`adapter.ts:299-306` in the registry), so **`terminal: false`**. Claude runs its own shell and reports output through updates.
- `elicitation.form` is always advertised. `url` is advertised when `onUrlElicitation` is set, which the registry sets whenever a runtime coordinator exists (`registry adapter.ts:275-285`).
- `clientCapabilitiesMeta` is not set for Claude (Copilot and Devin only).

### 3.3 What is read from agentCapabilities / initialize result

| Field | Where read | Effect |
|---|---|---|
| `protocolVersion` | `adapter.ts:1906-1910` | `followsAgentWorkState = flavor.followAgentWorkState === true && continuationRequests !== undefined && setup.initializeResult.protocolVersion === 2`. Claude gets this flag only on v2. |
| `agentCapabilities.loadSession` | `AcpSessionRuntime.ts:2219-2220` | `=== true` picks `session/load` on resume. |
| `agentCapabilities.sessionCapabilities.resume` | `AcpSessionRuntime.ts:2230` | `!= null` picks `session/resume` when load is not available. |
| `agentCapabilities.sessionCapabilities.list/delete` | `probe.ts:267-271`, `AcpSessionRuntime.ts:2494-2537` | Enables session list and delete. |
| `agentCapabilities.mcpCapabilities.acp` / `.http` | `AcpSessionRuntime.ts:2139-2148` | Chooses MCP transport (see 9). |
| `agentCapabilities.promptCapabilities` | `adapter.ts:7058`, flavor `buildPrompt` input | Gates image blocks (see 5). |
| `agentCapabilities.providers` | `AcpSessionRuntime.ts:2540-2577` | Enables providers/list, set, disable. |
| `agentCapabilities.auth.logout` | `AcpSessionRuntime.ts:2594-2604` | Enables logout. |
| `authMethods` | `AcpSessionRuntime.ts:2168-2194` | See 3.4. |
| `agentInfo.version` | `adapter.ts:7757` | Passed to flavor `buildPrompt`. |

### 3.4 Authentication

`AcpSessionRuntime.ts:2164-2208` (`authenticateAfterRequired`):
1. `configuredAuthMethodId = options.authMethodId?.trim()`. For the registry this comes from settings `authMethodId` (`registry adapter.ts:167`).
2. `selectAcpAgentAuthMethod(initializeResult.authMethods, configuredAuthMethodId)` picks the method (function defined outside the read range).
3. If a configured id is not advertised, it fails with `AcpTransportError` (lines 2173-2182).
4. A method whose `type` is not `"agent"` fails: "...requires ${authMethod.type} authentication, which cannot run inside a headless provider session" (lines 2183-2192). **So browser or terminal auth types are rejected at runtime here.**
5. Otherwise `authenticate({ methodId })` is sent (lines 2199-2206).

Triggering: `setupSession` runs first. If it fails with an auth-required error (`isAcpAuthenticationRequired`), authentication runs and setup is retried once (lines 2278-2284). `authenticateOnAuthRequired: false` disables this (used by probes, `probe.ts:353,486`).
`authenticateEagerly` (lines 2266-2277) sends `authenticate` before setup. Not set for Claude.

Sign-in UI (from the docs, `providers-acp.md:54-64`): browser sign-in shows the agent's URL, and T3 waits for the user before telling the agent to proceed. Elicitation URLs are routed through `runtimeCoordinator.requestUrlAuthentication` (`registry adapter.ts:275-285`, which is in the registry runtime coordinator, not read).

---

## 4. Session lifecycle

### 4.1 session/new

`AcpSessionRuntime.ts:2249-2260`:
```ts
const createPayload = {
  cwd: options.cwd,
  mcpServers: sessionMcpServers(initializeResult),
  ...additionalDirectories,
} satisfies EffectAcpSchema.NewSessionRequest;
const created = yield* runLoggedRequest("session/new", createPayload, acp.agent.createSession(createPayload));
sessionId = created.sessionId;
sessionSetupResult = created;
```
- `cwd` is `input.runtimePolicy.cwd ?? process.cwd()` (`adapter.ts:2304`).
- `additionalDirectories` is set only when `options.additionalDirectories` is non-empty (lines 2151-2159). Not set by the registry path (grep).
- `sessionMeta` is not set for Claude (grep: none).

### 4.2 session/resume or session/load

`AcpSessionRuntime.ts:2217-2247`. Taken when `options.resumeSessionId` is set (from `adapter.ts:2312`, from the provider thread):
- If `agentCapabilities.loadSession === true` and `resumeMethod !== "resume"`: `session/load` with `{ sessionId, cwd, mcpServers }`, through `runLoadSessionWithReplayIdle` (lines 1949-2028). Replay gate: default 90 s total timeout (`defaultSessionLoadTimeout`, line 81), 2 s idle gap (`defaultSessionLoadReplayIdleGap`, line 82). Replayed updates are quarantined while loading (lines 1716-1735, 1755-1765, and `sessionUpdateIsReplay` at 1736).
- Else if `sessionCapabilities.resume != null`: `session/resume` (lines 2230-2241).
- Else: `AcpRequestError` code `-32601`, "ACP agent does not advertise session/load or session/resume support" (lines 2242-2246). **There is no fallback to a new session on resume failure in this runtime.**

Native session id is the ACP `sessionId`. It is persisted on the T3 provider thread as `nativeThreadRef.nativeId` (read by `nativeThreadId`, `adapter.ts:865-879`, which fails with "Provider thread ... is missing its ACP session id" if absent). Where that id is written on first session creation was not verified (the write is in the parts of `adapter.ts` that were not read).

### 4.3 Mode and model selection

- Mode: `setMode` (`AcpSessionRuntime.ts:2674-2697`). Uses `session/set_mode` only when there is no `category:"mode"` config option and a legacy `modes` state exists. Otherwise it writes the mode config option through `setConfigOption`.
- Model: `setModel` (2710-2714) writes `started.modelConfigId ?? "model"` through `setConfigOption` (2030-2083). `setSessionModel` (2698-2708) sends `session/set_model` directly.
- `setConfigOption` (2030-2083) validates the value against the advertised options first (`validateConfigOptionValue`, 1817-1873). Custom models pass if listed in `options.customModels` (line 1858-1862). It skips the RPC when the current value already matches (2041-2045).
- Claude does not set `applyModelSelection`, `resolveModelId`, or `modelOptionArrivesLate` in the registry flavor (grep: none). The model comes from the generic config-option path.
- Config and mode updates arriving on the session (`config_option_update`, `current_mode_update`) update `configOptionsRef` and `modeStateRef` (`AcpSessionRuntime.ts:2791-2797`).

### 4.4 Per-thread session storage and reuse

- One ACP session per T3 provider thread. The runtime is created per session by `spawnAcpRuntime(threadId, resumeSessionId)` (`adapter.ts:6796-6799`) and `makeRuntime(makeRuntimeInput(...))` (`adapter.ts:6774-6784`). The verified part is that `resumeSessionId` comes from the thread's `nativeThreadRef`.
- Reuse: the runtime survives across turns while `activeSessionId` matches. `setActiveSessionSetup` (`adapter.ts:1902-1914`) resets `agentWork`, `agentRequests` and `nativeChildren` on each new setup.
- Rollback (`adapter.ts:9080-9096`): `awaitRuntimeTeardown`, then `startReplacementAcpRuntime(...).pipe(Effect.retry({ times: 1 }))`. Rollback therefore starts a fresh ACP session (consistent with `providers-acp.md:131-133`: "ACP agents cannot rewind their own conversation").
- `session/list` and `session/delete` are exposed through probe and runtime (`probe.ts:589-660`).

---

## 5. Prompt turn

### 5.1 Building the prompt

`resolvePromptParts` (`packages/provider-acp/src/server/adapter.ts:7697-7829`). Output is `Array<ContentBlock>`, in this order:

1. **Text block** (lines 7710-7737). `userText` (after optional `flavor.rewriteUserText`; Claude does not set it) is combined with attachment paths (`providerMessageTextWithAttachmentPaths`, lines 7721-7726), then wrapped by `t3AcpPromptWithInstructions` (lines 7728-7734) with the instruction state (`interactionMode`, `hasT3Mcp`, `hasDeviceTools`, lines 7703-7707). Instructions are sent again only when the state changed (`previousInstructionState`, line 7708).
   ```ts
   if (text.length > 0) { prompt.push({ type: "text", text }); }
   ```
2. **Images** (lines 7738-7804). Only attachments passing `isProviderNativeImageAttachment`. If any exist and `supportsImagePrompts` is false, the turn fails with "ACP driver did not negotiate image prompt support" (lines 7741-7746). Otherwise each attachment is read from disk and sent as base64:
   ```ts
   prompt.push({ type: "image", data: Buffer.from(bytes).toString("base64"), mimeType: attachment.mimeType });
   ```
   Non-image attachments are only referenced by path in the text (step 1). `supportsImagePrompts` is `promptCapabilities.image === true` (line 7058), or `flavor.supportsImagePrompts` (type at line 527).
3. **Runtime instructions text block** (lines 7811-7823), appended unless the text starts with `/` (native slash command). Built by `buildRuntimeInstructions({ harness, model, pullRequestLinking })` (from `packages/provider-core/src/server/runtimeInstructions.ts`, not read).
4. Empty prompt is an error: "ACP turn requires non-empty text or attachments" (lines 7805-7810).

Flavor `buildPrompt` is not set for Claude, so the attachment code path above is the one used.

### 5.2 Context and history

ACP has no history parameter. The agent keeps conversation state in its own session. T3 sends only the new prompt blocks. History is preserved only through the agent's session (`session/load` or `session/resume`) and never re-sent. When instructions changed or after compaction (`compactionMarkers`, lines 7708-7709, 8170-8181), they are re-sent in the text block.

### 5.3 Dispatch

`adapter.ts:8146-8153`:
```ts
const promptGeneration = yield* Ref.get(runtimeCallbackGeneration);
const promptOptions = { dispatched: context.promptDispatched };
context.promptSentAt = yield* Clock.currentTimeMillis;
...
runtime.prompt({ prompt: promptParts!.prompt }, promptOptions)
```
`AcpSessionRuntime.prompt` (`AcpSessionRuntime.ts:2605-2665`) serialises prompts (`promptSerializationSemaphore`), sends `session/prompt` with `{ sessionId, prompt }`, and records the fiber as the active prompt. Dispatch is signalled through `promptOptions.dispatched`.

### 5.4 Completion: the RPC response does not end a Claude turn

This is the surprising part. When `followsAgentWorkState` is true (Claude on ACP v2), the `session/prompt` response handler returns without finalizing:
```ts
Effect.flatMap((result) =>
  runRuntimeCallbackAtGeneration(promptGeneration, Effect.gen(function* () {
    if (context.finalized || followsAgentWorkState) return;
```
(`adapter.ts:8198-8202`). Turn end is driven by the agent's own `state_update` with `state: "idle"` (see 6.2 and `settleIdleWork`, `adapter.ts:4790-4818`):
- `state: "idle"`, `stopReason: "error"` -> finalize `failed` with `idleErrorFailure(...)`.
- `stopReason: "cancelled"` -> `cancelled` (or `interrupted` if this was a user Stop, `pendingStopCloseTurn`).
- otherwise -> `completed`.
- The turn does not finalize while `agentRequests.size > 0` (pending approvals or elicitations), line 4796-4798.

The `session/prompt` RPC stop reason is ignored for Claude except for logging and usage capture. Stop reasons used elsewhere in the adapter: `"end_turn"`, `"cancelled"`, `"error"` (`adapter.ts:8156, 8204-8224`).

### 5.5 Stop reasons in the runtime

`AcpSessionRuntime.ts:2633-2640`: if the prompt fiber was interrupted (not `wait-for-prompt` mode), the runtime returns `{ stopReason: "cancelled" }`. Native close rejects prompts cancelled before insertion with `-32800` (`adapter.ts:8263-8268`).

### 5.6 Streaming

- Text deltas are coalesced: first chunk and segment close are immediate, later chunks flush every `ACP_TEXT_FLUSH_INTERVAL = "50 millis"` (`adapter.ts:146-150`), full-text snapshots.
- Deferred finalize debounce: `ACP_DEFERRED_FINALIZE_DEBOUNCE = "3000 millis"` (`adapter.ts:144`), used only when `flavor.deferFinalizeForBackgroundWork`. Not set for Claude (grep: none).
- Assistant text is segmented: `AssistantItemStarted` / `AssistantItemCompleted` events, item id `assistant:<session>:runtime:<id>:segment:<n>` (`AcpSessionRuntime.ts:2884-2947`).

---

## 6. Incoming session/update mapping

### 6.1 Runtime projection (`runtimeModel.ts:1363-1620`, `parseSessionUpdateEvent`)

| `sessionUpdate` | Event produced | Lines |
|---|---|---|
| `config_option_update` | `ModeChanged` (if a mode option is found) + `ConfigOptionsUpdated` | 1372-1384 |
| `available_commands_update` | `AvailableCommandsUpdated` | 1385-1392 |
| `current_mode_update` | `ModeChanged` (if non-empty) | 1393-1402 |
| `plan` | `PlanUpdated` `kind: "items"` (legacy id `"legacy"`) | 1403-1418 |
| `plan_update` | `PlanUpdated` with `items` / `markdown` / `file` / `unknown` kind | 1419-1469 |
| `plan_removed` | `PlanUpdated` `kind: "removed"` | 1470-1480 |
| `tool_call` | `ToolCallUpdated` (fallbackStatus `pending`) | 1481-1493 |
| `tool_call_update` | `ToolCallUpdated` | 1494-1504 |
| `agent_message_chunk` | `ContentDelta` (with `messageId` if present) | 1505-1516 |
| `agent_thought_chunk` | `ThoughtDelta` (text only) | 1517-1526 |
| `compaction_update` | `ToolCallUpdated` with synthetic id `acp-compaction:<id>`, title "Compact context" | 1527-1552 |
| `compaction_summary_chunk` | same synthetic tool call, `inProgress` | 1553-1566 |
| `usage_update` | `UsageUpdated` `{ usedTokens: used, maxTokens: size if >0, cost: {amount, currency} }` | 1568-1580 |
| `session_info_update` | `SessionInfoUpdated` (title, updatedAt) | 1581-1593 |
| `notice` | `NoticeReceived` (severity, title, description) | 1594-1606 |
| `_t3_unknown` | `UnknownUpdate` (`updateType` = `originalSessionUpdate`) | 1607-1614 |

Tool-call merge and emission throttling: `mergeToolCallState` and `decideToolCallUpdateEmission` (called from `AcpSessionRuntime.ts:2807-2830`). Terminal tools are removed from the tracking map on `completed` or `failed` (line 2818-2819).

### 6.2 Adapter projection to app events (`adapter.ts:4864-5447`, and 5256-5442)

The adapter does the per-turn projection. Relevant branches:

- `usage_update` and `session_info_update` (`adapter.ts:4877-4931`): stored in `contextUsageBySessionId` and `nativeMetadataBySessionId`. If the thread is known, a `provider_thread.updated` event is emitted with `contextUsage` and `nativeMetadata`. **Not a per-turn token count** (see 10).
- `state_update` (`adapter.ts:4938-4960` and 5256-5275):
  - Stored as `agentWork`. `user_message` / `user_message_chunk` set it to `running` and mark `workAccepted`.
  - `state: "idle"` while a turn is attached calls `settleIdleWork`, which completes or fails the turn (see 5.4).
  - `state: "requires_action"` emits a tool `"Action required"` with status `requiresAction`; it becomes `completed` on a later state (lines 5256-5275).
- `agent_message_chunk` (lines 5276-5294): `appendText(context, "assistant", text, messageId)`.
- `agent_thought_chunk` (lines 5295-5301): `appendText(context, "reasoning", ...)`.
- `user_message`, `agent_message`, `agent_thought` (lines 5302-5323).
- `tool_call_content_chunk` (lines 5324-5342).
- `compaction_summary_chunk` (5343-5357).
- `user_message_chunk` (5358+).
- Default branch (`adapter.ts:5425-5444`):
  - `ToolCallUpdated` -> `emitTool(context, toolCall)`.
  - `PlanUpdated` -> `emitPlan(context, payload)`.
  - `NoticeReceived` -> `emitNotice`.
  - `UnknownUpdate` -> a tool item with id `<turn>:unsupported:<updateType>`, title "Unsupported ACP update: ...", status `completed`.
- `plan`, `plan_update`, `tool_call`, `tool_call_update` also appear in the `tool`-related branches at `adapter.ts:5144-5166` (not read in full).
- `available_commands_update` and `config_option_update` / `current_mode_update` are published to the registry runtime coordinator (`adapter.ts:6199-6207`; `registry adapter.ts:263-274`). That is how slash commands and live model/mode options reach the picker.

### 6.3 Claude-specific subagent routing

Claude Agent sends nested-session state as a `_t3_unknown` update whose `originalSessionUpdate` is `subagent_update` or `session_message`. `claudeAcp.ts` decodes both (lines 79-117):
- `subagent_update` -> `{ type: "state", childSessionId, parentSessionId, status: running | failed | cancelled | completed }` (lines 85-103). `running` if state is `running` or `requires_action`; `failed` if `stopReason === "error"`; `cancelled` if `stopReason === "cancelled"` or state `unknown`; else `completed`.
- `session_message` -> `{ type: "delegation", childSessionId, parentSessionId, delegationId: messageId, prompt }` (lines 104-116).
- The `Agent`/`Task` tool calls become subagents via `extractClaudeSubagentUpdate` (lines 33-57). Status `failed`, `completed` (unless background), or `running`. Background is `run_in_background === true` or `_meta.claudeCode.toolResponse.isAsync === true` (lines 39-42).
- `isClaudeProvisionalSubagentTool` (lines 24-31): an Agent/Task tool with no decodable input yet stays provisional.
- Wiring: `registry adapter.ts:231-237` sets `followAgentWorkState: true`, `extractSubagentUpdate`, `isProvisionalSubagentTool`, `extractSubagentSessionEvent`.
- Child-session updates are routed by `followNativeChild` (`adapter.ts` ~3560-3600) and never flattened into the root turn (`AcpSessionRuntime.ts:1766-1767` comment).

### 6.4 Test evidence

`AcpRegistryAdapterV2.test.ts:125-200` has the Claude v2 replay: `initialize` returns `protocolVersion: 2`, `info.name: "@agentclientprotocol/claude-agent-acp"`, `version: "0.90.0"`, `capabilities: { session: {} }`, `authMethods: []`. It then sends `session/new` with `sessionId`, and `state_update` running / idle with a `stopReason`. Lines 1390-1420 show a `usage_update` and an idle `state_update` with `stopReason: "end_turn"`. Lines 1548-1628 test `session/request_permission` and `elicitation/create` approval and question flows.

---

## 7. Permissions

### 7.1 Inbound `session/request_permission`

`adapter.ts:6371-6545` registers `targetRuntime.handleRequestPermission(...)`. Order:

1. Attach the tracked tool call (`withTrackedPermissionToolCall`, 6374-6380).
2. **Question-as-permission** (`flavor.extractPermissionQuestion`, lines 6382-6401). Not set for Claude.
3. Disposition from policy (`dispositionFor`, lines 6402-6405, default `acpPermissionDisposition`, `clientPolicy.ts:192-200`). Uses the tool `kind`, or the kind seen on the earlier `tool_call` if the request omits it (lines 6420-6426).
4. If `followsAgentWorkState` and the disposition is `ask`, wait for the request turn to attach. Cancelled if it never does (`awaitRequestTurn`, lines 6406-6409, 4816-4840).
5. Branch on disposition:
   - `allow` -> `selectAutoApprovedPermissionOption(params)` (lines 6427-6436). Immediate `selected`.
   - `deny` -> `selectPermissionOptionId(params, "decline")` (lines 6437-6446). Immediate `selected`.
   - `ask` -> `beginApprovalRequest(...)` (line 6447-6455), which creates a runtime request, an `approval_request` turn item (status `waiting`) and emits it. The callback waits on `Deferred.await(pendingDecision)` (line 6487).

### 7.2 Option kinds chosen

From `adapter.ts:1272-1293`:
```ts
function selectPermissionOptionId(request, decision): string | undefined {
  const optionIdOfKind = (kind) => request.options.find((option) => option.kind === kind)?.optionId.trim() || undefined;
  if (decision === "acceptForSession") {
    return optionIdOfKind("allow_always") ?? optionIdOfKind("allow_once");
  }
  return optionIdOfKind(decision === "accept" ? "allow_once" : "reject_once");
}
// Prefer allow-once; allow-always can persist beyond the session.
function selectAutoApprovedPermissionOption(request) {
  return selectPermissionOptionId(request, "accept") ?? selectPermissionOptionId(request, "acceptForSession");
}
```
| T3 decision | ACP option kind | Notes |
|---|---|---|
| `accept` | `allow_once` | Auto-approve prefers this. |
| `acceptForSession` | `allow_always`, falls back to `allow_once` | Falls back so an agent offering only `allow_once` is not stopped as cancelled. |
| `decline` / `reject` | `reject_once` | Deny path. |
| `cancel` | response `{ outcome: "cancelled" }` | Lines 6519-6522. |
If no matching option exists, the response is `cancelled` (lines 6432-6433, 6524-6525).

Session-scoped grants: `clientPolicyGrants.recordApproval` (lines 6509-6518). Only `command` kind grants execution (`clientPolicy.ts:240-248`).

If the agent's card did not offer "this session", `acceptForSession` is downgraded to `accept` (lines 6502-6508, `flavor.approvalOptions`). Claude does not set `approvalOptions`, so the approval item has no `options` list. Option mapping still applies as above.

### 7.3 Disposition rules (`clientPolicy.ts`)

`acpOperationDisposition` (lines 143-186):
- Reads (`read`, `search`, `think`) are `allow` unless the sandbox type is unknown (lines 82-98, `acpReadDisposition`). **Reads never ask.**
- If approval is required (`acpPolicyRequiresApproval`: `approvalPolicy !== "never"`, or `runtimeMode === "approval-required"` when unset, lines 76-80), non-read tools are `ask`.
- Otherwise by sandbox type: `readOnly` -> deny; `workspaceWrite` -> mutations (`edit`, `delete`, `move`) allowed only if every location is under cwd or a writable root (`acpWorkspaceWriteAllowsMutation`, lines 100-141); `dangerFullAccess` / `externalSandbox` -> allow.
- Unset sandbox: `approval-required` -> deny; `auto-accept-edits` (with no `approvalPolicy`) -> `ask` for non-mutation kinds, else allow.

`acpMcpToolApprovalElicitationDisposition` (lines 202-216) handles MCP tool elicitations: `ask` only when approval is required, otherwise `allow`. The comment says T3's authenticated MCP endpoint enforces scope.

### 7.4 Elicitation (form and URL)

`adapter.ts:6546-6560` and `registry adapter.ts:275-285`. Form elicitations go through `flavor.extractElicitationForm`. URL elicitations go through `onUrlElicitation` -> `runtimeCoordinator.requestUrlAuthentication`, with the URL normalised by `normalizeAcpRegistryWebUrl` and the id trimmed to 256 and message to 1024 characters.

### 7.5 Timeouts and UI flow

- **No timeout on pending approvals was found.** The callback waits on `Deferred.await(pendingDecision)` (`adapter.ts:6487`). Grep for `Duration|timeout|sleep` in `beginApprovalRequest` and nearby returned nothing. The pending request ends when it is answered, or when the turn or runtime is torn down (`cancelPendingRuntimeRequests`, line 2286, and `holdingAgentRequest`).
- Answering: the web UI sends an `accept | acceptForSession | decline | cancel` decision for the `requestId`. The `pendingRuntimeRequests` map and `ProviderApprovalDecision` type are in `adapter.ts` (`pendingRuntimeRequests`, around 5496-5510). Not read in full.
- Policy surface to the user: `docs/user/providers-acp.md:116-119` says that under **Supervised** the request is shown in the conversation, under **Auto-accept edits** edits are approved and the rest shown, and under **Auto** and **Full access** everything is approved. File reads and searches never wait for approval.

---

## 8. Cancellation and stop

### 8.1 Cancel behaviour for Claude

`AcpSessionRuntime.ts:2366-2412`:
```ts
const cancel = Effect.gen(function* () {
  const started = yield* getStartedState;
  const activePrompt = yield* Ref.get(activePromptRef);
  const cancelPayload = { sessionId: started.sessionId, ...(options.cancelMeta === undefined ? {} : { _meta: options.cancelMeta }) };
  if (options.cancelBehavior !== "wait-for-prompt") {
    if (options.interruptPromptOnCancel === false) {
      return yield* acp.agent.cancel(cancelPayload);
    }
    if (Option.isSome(activePrompt)) { yield* Fiber.interrupt(activePrompt.value.fiber).pipe(Effect.ignore); }
    yield* acp.agent.cancel(cancelPayload).pipe(Effect.ignore);
    return;
  }
  ...
```
Adapter: `interruptPromptOnCancel: flavor.interruptPromptOnCancel ?? false` (`adapter.ts:2313`). Claude does not set it, so it is `false`. Claude has no `cancelBehavior` in the registry. **Result for Claude: `cancel` sends `session/cancel` and returns at once. It does not interrupt the local prompt fiber, and it has no 15 s wait or kill fallback.** The `cancelTimeout` (`defaultCancelTimeout = 15 s`, line 83) applies only to `wait-for-prompt` mode, which Claude does not use.

### 8.2 Stop at the turn level (`adapter.ts:8600-8910`)

- `runtime.cancel` is called at `adapter.ts:8893`.
- For `followsAgentWorkState` plus `turnInput.requestRuntimeRestart === true` (a session-wide Stop), the adapter calls `runtime.closeSession()` wrapped in `Effect.timeoutOption(ACP_STOP_CONFIRM_TIMEOUT_MS)` (`adapter.ts:8645-8655`, constant `10_000` at line 1631). Comment: "ACP cancel leaves native children alive; close confirms session-wide Stop." If the close does not confirm in time: "The agent refused to close its session; background work may still run" (`adapter.ts:8660-8667` area).
- `closeSession` sends `session/close` (`AcpSessionRuntime.ts:2510-2525`), gated on capability.
- Stop uses `stopReason: "cancelled"` idle from the agent when available (see 5.4).
- `ACP_STOP_CONFIRM_TIMEOUT_MS` is also used at `adapter.ts:8900` (second confirm path, not read).

### 8.3 Hung turn and kill fallback

- `retireRuntime` kills the child with `forceKillAfter: "1 second"` (`AcpSessionRuntime.ts:2359-2364`). It is used by `wait-for-prompt` cancel timeout, and by `wait-for-prompt` interrupt (lines 2652-2658). Neither applies to Claude.
- Claude-specific termination: if the child exits, `recordTermination` -> `ConnectionTerminated` event, the active turn is finalized `failed` with `makeProviderFailure({ class: "transport_error" })` (`adapter.ts:2286-2294`), and the stderr tail is attached (`enrichProcessExitWithStderr`, `AcpSessionRuntime.ts:1298-1319`, 250 ms wait for stderr drain).

---

## 9. MCP injection

`packages/provider-acp/src/server/adapter.ts:812-863` (`acpMcpContext`):
```ts
servers: [{
  name: "t3-code",
  command: self.command,
  args: [...selfInvocationArgs(self, ["acp-mcp-bridge"])],
  env: [
    { name: "ELECTRON_RUN_AS_NODE", value: "1" },
    { name: "T3_ACP_MCP_ENDPOINT", value: session.endpoint },
    { name: "T3_ACP_MCP_AUTHORIZATION", value: session.authorizationHeader },
  ],
}],
acpServers: [{ type: "acp", name: "t3-code", serverId: "t3-code" }],
httpServers: mcpOverHttp === true ? [{ type: "http", name: "t3-code", url: session.endpoint, headers: [{ name: "Authorization", value: session.authorizationHeader }] }] : [],
```
- The server is **the app itself** (`self = resolveSelfInvocation()`, `registry driver.ts`/`adapter.ts:332`), re-run with the argument `acp-mcp-bridge`. It is an stdio MCP server named `t3-code`.
- Session endpoint and bearer come from `McpProviderSessions.read(threadId)` (`adapter.ts:1671-1674`, and `resolveMcpContext` callers). `McpProviderSession` is in `packages/provider-core/src/server/McpProviderSessions.ts`, not read.
- Transport selection (`AcpSessionRuntime.ts:2135-2149`):
  ```ts
  if (mcpCapabilities?.acp === true && acpServers.length > 0) return acpServers;
  if (mcpCapabilities?.http === true && httpServers.length > 0) return httpServers;
  return activationOptions?.mcpServers ?? options.mcpServers ?? [];
  ```
  So: ACP-tunnelled MCP if the agent advertises `mcpCapabilities.acp`; else HTTP if advertised and `mcpOverHttp`; else the stdio `servers` entry.
- **For Claude: `mcpOverHttp` is not set**, so HTTP is never injected. Claude gets the stdio `t3-code` server unless its initialize advertises `mcpCapabilities.acp` (the replay fixture's initialize does not advertise `mcpCapabilities`, so Claude receives the stdio entry). `httpServers` is `[]`.
- `mcpOverAcpBridge.ts` (242 lines) carries MCP JSON-RPC over ACP (`_mcp/...` tunnelling). Limits: `MAX_CONNECTIONS = 16`, `MAX_MESSAGE_BYTES = 8 MiB` (lines 7-8). The connection tracks `sessionId` and `protocolVersion`.
- `runtimeModel.ts:860` comment: "goose, qwen, and claude-acp may identify MCP calls only in metadata" — Claude MCP tool calls are recognised from metadata, not from a dedicated field.
- The app's own tools (orchestrator MCP, etc.) are exposed through this same `t3-code` server; `mcpSessions.read(turnInput.threadId)` decides `hasT3Mcp` and `hasDeviceTools` for the instructions text (`adapter.ts:7702-7707`).

---

## 10. Usage, cost, tokens, and error handling

### 10.1 Usage capture (ACP path)

- The only usage signal Claude sends over ACP that is wired is `usage_update` (`runtimeModel.ts:1568-1580`): `used` tokens, `size` (context window), and optional `cost` (`amount` and `currency`, currency trimmed to 32 chars).
- The adapter stores it as `ThreadTokenUsageSnapshot` (`contextUsage`) in `contextUsageBySessionId` and on the provider thread (`adapter.ts:4885-4931`). This feeds the composer's context meter (`providers-acp.md:101-104`).
- Per-turn token accounting from the `session/prompt` response `usage` field is used only when `flavor.reportsPerTurnPromptUsage === true` (`adapter.ts:8186-8190`, mapper at 239-276, `acpPerTurnPromptUsage`). **Not set for Claude**: only `glm-acp-agent` sets it (`registry adapter.ts:226-227`). So for Claude the turn's per-turn token usage is not captured on this path.
- `ClaudeTurnTokenUsage.ts` and `claudeUsageLimits.ts` are **not** used on the ACP path (see section 0).

### 10.2 Error handling

- Prompt RPC errors: `acpRegistryPromptFailure` (`registry adapter.ts:117-131`):
  ```ts
  export function acpRegistryPromptFailure(agentId: string, cause: unknown) {
    return makeProviderFailure({
      cause,
      ...(isAcpRequestError(cause)
        ? { message: cause.errorMessage, code: String(cause.code),
            class: agentId === "mistral-vibe" && cause.code === MISTRAL_VIBE_RATE_LIMITED ? ("usage_limit" as const) : ("provider_error" as const) }
        : { class: "provider_error" as const }),
    });
  }
  ```
  For Claude this is always `provider_error` with the agent's message and code. **There is no usage-limit classification for Claude on the ACP path.** Only Mistral Vibe code `-31001` maps to `usage_limit`.
- Idle error: `stopReason: "error"` from `state_update` becomes `idleErrorFailure(work.error)` (`adapter.ts:4804`, 8203-8216).
- Auth required: `flavor.requiresSignIn` is not set for Claude. Auth-required errors trigger `authenticateAfterRequired` (see 3.4).
- Process crash: transport failure finalizes the turn `failed` with class `transport_error` and the stderr tail (see 8.3).
- Retry: the only `Effect.retry` on this path is `Effect.retry({ times: 1 })` for the replacement runtime on rollback (`adapter.ts:9096`). **No automatic prompt retry and no backoff exists for Claude on this path.** Provider retry notices come only through the `reportProviderRetry` extension. The only registered one is Mistral Vibe's `_session/retrying` (`registry adapter.ts:96-115`). Claude does not register it.
- `Effect.timeoutOption` on prompt does not exist. A stuck prompt is bounded only by the stop-confirm 10 s (8.2), the session-load 90 s (4.2), and the registry 30 s and 20 min timers (section 2).

---

## 11. Tests

Claude-specific:
- `apps/server/src/orchestration-v2/Adapters/AcpRegistryAdapterV2.test.ts`:
  - `claudeFrames` replay (lines 125-200): Claude Agent `claude-agent-acp 0.90.0` on ACP v2 (initialize, session/new, state_update running/idle, usage_update).
  - Permission and question flows for the Claude replay (around 1548-1628).
  - Launch environment (`CLAUDE_AGENT_ACP_EXPERIMENTAL_V2`) for `agentId: "claude-acp"`, local `/opt/bin/claude-agent-acp`, and override `"0"` (lines 1018-1043).
  - Subagent and background-task cases (grep found `claude-session` fixtures, not read).
- `claudeAcp.ts` has **no dedicated unit test** (see section 0).
- `packages/provider-acp-registry/src/server/AcpRegistrySupport.test.ts`, `auth.test.ts`, `authenticationState.test.ts`, `driver.test.ts`, `AcpRegistryRuntimeCoordinator.test.ts`: exist, not read.

Generic ACP (apply to Claude):
- `packages/provider-acp/src/server/`: `clientPolicy.test.ts`, `clientTerminals.test.ts`, `mcpOverAcpBridge.test.ts`, `nativeLogging.test.ts`, `runtimeModel.test.ts`, `sessionConfig.test.ts`, `stderr.test.ts`. `AcpSessionRuntime.processTree.test.ts` (process-tree termination). Not read.
- `packages/effect-acp/src/`: `agent.test.ts`, `client.test.ts`, `errors.test.ts`, `protocol.test.ts`, `schema.test.ts`, `_internal/stdio.test.ts`. Fixture: `test/fixtures/acp-mock-peer.ts`. Not read.
- `apps/server/src/orchestration-v2/Adapters/AcpAdapterV2.test.ts`, `AntigravityAdapterV2.test.ts`: generic adapter tests. Not read.
- Mock agent: `apps/server/scripts/acp-mock-agent.ts`. Not read.
- Replay recorders: `apps/server/scripts/record-*-acp-replay-fixture.ts` (kiro, grok, fx). None for Claude (`claude` replay frames are inline in the test).

---

## 12. Things a port must not miss (verified)

1. **Two Claude integrations.** The ACP path (this report) is not the native Agent SDK path (`ClaudeAdapterV2.ts`). Pick one.
2. **ACP v2 is hard-coded** (`AcpSessionRuntime.ts:2087`, `protocolVersion: 2`). Claude's env default is `CLAUDE_AGENT_ACP_EXPERIMENTAL_V2=1` (`registry adapter.ts:204-209`). Setting `0` changes the agent to v1 and the client's turn logic then uses the RPC response path, not `state_update` idle.
3. **Turn completion is the agent's `state_update` idle, not the `session/prompt` response** (`adapter.ts:8202`). Porting only the RPC response will leave turns open or finalise them too early.
4. **`_t3_unknown` wrapper.** Claude's `subagent_update` and `session_message` arrive as `session/update` with unknown `sessionUpdate`. The client wraps them (`effect-acp/src/client.ts:500, 520`) as `{ sessionUpdate: "_t3_unknown", originalSessionUpdate, raw }`. `claudeAcp.ts` decodes `update.raw` (lines 84-86, 104-106). A port must replicate that wrap or the subagent events are lost.
5. **Child-session lineage.** Updates whose `sessionId` is not the root are routed to a child, never flattened into the root turn (`AcpSessionRuntime.ts:1766-1773`; `adapter.ts` `followNativeChild`).
6. **fs and terminal are advertised as false.** Claude reads and writes files and runs commands itself. T3 only shows what it reports. `providers-acp.md:121-123` confirms: "For other agents, T3 Code shows the command, output, and exit status the agent reports but does not run or stop their terminals."
7. **Permission options are matched by kind, not by position.** `allow_once`, `allow_always`, `reject_once`. Auto-approve prefers `allow_once` (`adapter.ts:1286-1293`). `acpForSession` falls back to `allow_once` when `allow_always` is absent (1278-1281). Downgrade to `accept` if the offered options lack it (6502-6508).
8. **Reads never ask** (`clientPolicy.ts:82-98`). Only `edit`, `delete`, `move`, and `execute` kinds gate in approval-required mode.
9. **No approval timeout.** Pending approvals wait until answered or torn down (`adapter.ts:6487`).
10. **Cancel does not kill or wait for Claude** (`interruptPromptOnCancel` false path, `AcpSessionRuntime.ts:2373-2375`). The `wait-for-prompt` 15 s and kill fallback are not used.
11. **Stop = `session/cancel`, then `session/close` with a 10 s confirm** for session-wide Stop (`adapter.ts:1631, 8645-8655`).
12. **No process-tree kill for Claude.** `ownDetachedProcessGroup` is unset, so only `child.kill` plus AgentScope stop apply. Claude's own grandchildren may outlive T3 (verify on each OS).
13. **Shell-resolved spawn for registry, shell:false for local.** `AcpSessionRuntime.ts:1374-1380`. On Windows, registry spawns go through shell resolution; local `.cmd` is rejected (`AcpRegistrySupport.ts:1702-1711`).
14. **Install lock and receipt semantics.** Stale lock is 5 min; receipts under `package-installs/` are validated against the on-disk `package.json` version (`AcpRegistrySupport.ts:1117-1176`, 1138-1155).
15. **Exact-version enforcement is in the registry entry, not in the app.** The app has no version pin for `claude-agent-acp`; it runs whatever the live registry index lists as latest (`AcpRegistrySupport.ts:51, 86-105`). A port that wants reproducible installs needs its own pin.
16. **`session/resume` has no fallback to a new session** (`AcpSessionRuntime.ts:2242-2246`). Resume failure surfaces as an error.
17. **No usage-limit or retry classification for Claude over ACP.** Only Mistral Vibe is mapped (`registry adapter.ts:117-131`, `96-115`). `claudeUsageLimits.ts` is native-only.
18. **Per-turn token usage is not captured for Claude over ACP** (`reportsPerTurnPromptUsage` unset). Only `usage_update` (context used/size and optional cost) is wired.
19. **Instructions are re-sent in the text block only when state changes**, and on compaction (`adapter.ts:7703-7734, 8160-8181`). Slash commands get no runtime-instructions block (`adapter.ts:7811`).
20. **Image attachments require `promptCapabilities.image`**, otherwise the turn fails before `session/prompt` (`adapter.ts:7738-7746`).
21. **Auth types other than `agent` are rejected** (`AcpSessionRuntime.ts:2183-2192`). Browser or terminal sign-in is handled by the registry coordinator and the probe, not by the turn runtime.
22. **Probe and session-list defaults.** Probe timeout 60 s (`probe.ts:46`). Probes use `authenticateOnAuthRequired: false` (`probe.ts:353,486`) so auth-required is reported immediately, not after an auth attempt.

---

## 13. Coverage (honest accounting)

Read in full (file-level):
- `docs/user/providers-acp.md`
- `packages/provider-acp-registry/src/settings.ts`
- `packages/provider-acp-registry/src/server/claudeAcp.ts`
- `packages/provider-acp-registry/src/server/adapter.ts`
- `packages/provider-acp-registry/src/server/AcpRegistrySupport.ts`
- `packages/provider-acp/src/server/clientPolicy.ts`
- `packages/effect-acp/src/_internal/stdio.ts`

Read by range:
- `packages/provider-acp/src/server/AcpSessionRuntime.ts`: 1-230, 1285-1484, 1596-2947 (2730-2947 included). **Not read:** 230-1284, 1484-1596, 2410-2540, 2723-2730.
- `packages/provider-acp/src/server/runtimeModel.ts`: 1363-1620.
- `packages/provider-acp/src/server/adapter.ts`: 141-330, 520-530, 812-880, 1272-1406, 1900-1915, 2280-2420, 3560-3600 (excerpt), 4790-4830, 4860-4960, 5250-5300, 5380-5560, 6360-6560, 7690-7840, 8120-8270, 8640-8660, 9080-9100. **About 15-20% of the file.**
- `packages/provider-acp-registry/src/server/driver.ts`: 246-260, 379-470.
- `packages/provider-acp/src/server/mcpOverAcpBridge.ts`: header only.
- `apps/server/src/orchestration-v2/Adapters/AcpRegistryAdapterV2.test.ts`: 120-200, 1018-1050, plus grep.

Grep only (not read): `packages/effect-acp/src/client.ts`, `protocol.ts`, `compat.ts`, `errors.ts`, `rpc.ts`, `agent.ts`, `_generated/*`; `packages/provider-acp/src/server/clientTerminals.ts`, `sessionConfig.ts`, `stderr.ts`, `nativeLogging.ts`; `packages/provider-acp-registry/src/server/probe.ts`, `auth.ts`, `authenticationState.ts`, `AcpRegistryRuntimeCoordinator.ts`; `packages/shared/src/AgentScope.ts`; `packages/provider-core/src/server/runtimeInstructions.ts`, `orchestrationInstructions.ts`; `apps/server/scripts/acp-mock-agent.ts`.

**Gaps a second pass should close first:** the remainder of `adapter.ts` (especially `makeAcpAdapterV2` setup at 1658-1900, the prompt and turn start around 7840-8120, and the `pendingRuntimeRequests` answer path around 5496-5520), `AcpSessionRuntime.ts` 230-1284 (`layer`/`make` setup and posix process ownership), `effect-acp/src/client.ts` (the `_t3_unknown` wrap and the request dispatch), and `probe.ts` and `AcpRegistryRuntimeCoordinator.ts` (the sign-in and config publish path).
