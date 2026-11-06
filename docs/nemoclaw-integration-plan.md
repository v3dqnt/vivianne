# NemoClaw: what to take, and how

Source: `NVIDIA/NemoClaw` (Apache-2.0), read at commit `5fa2335`. This plan uses its design, not its code, and the extraction came from a read-only research pass: spot-check anything you build on.

NemoClaw runs an agent (OpenClaw, Hermes, LangChain Deep Agents) inside an NVIDIA OpenShell sandbox on Docker, Linux or WSL2. Vivianne is a native desktop app with the agent in-process. So the useful things are the *patterns*, applied natively. OpenShell itself is an optional execution backend, not our state store.

## 1. Where Vivianne stands against each component

| NemoClaw component | What it does | Vivianne today | Gap |
|---|---|---|---|
| 24x7 server | Supervised gateway with respawn loop, PID and identity file, `/readyz`, health check, separate OS user the agent cannot kill | Tray app. Routines and check-ins run only while the app is open | No headless runtime, no supervision, no health surface |
| Inference | One local route (`inference.local`); a gateway holds the real keys, the agent sees placeholders; per-sandbox provider attachment; fail-closed route-conflict checks | Keys in OS keychain, read by Rust and by the webview's TS provider; routing tiers per task | Keys cross into the webview; no single choke point for accounting, fallback and policy |
| Guided onboarding | Staged preflight (ports, Docker, GPU, CA), validated credentials, `--resume` from a persisted session, express mode | First-run card (Nebius edition), provider settings | No preflight, no resumable multi-step setup, no secret-boundary check |
| Managed inference | Declarative catalog: models, serving presets per hardware class, recipes with digest-pinned images, automatic selection by readiness requirements | Hardware probe, fit rule, llama.cpp install, GGUF header reader, tuning knobs | Knowledge is in code, not a catalog; no auto-selected recipe per machine |
| Network policy | Deny-by-default egress per sandbox: YAML policy, tiers (restricted, balanced, open, personal), ~25 presets, runtime approve/deny, SSRF guard | Per-tool approvals and per-agent rules; no network policy | Web, MCP and shell egress are not governed by destination |
| Managed integrations | Channels (Telegram, Discord, Slack, Teams…), MCP, skills as declarative manifests; credentials registered as endpointless profiles; failed add rolls back | Connections (MCP and connectors), email and phone tools, skills hub, vault | No channel manifests, no placeholder-credential injection, no add/remove/status verbs |
| OpenShell state persistence | Registry, per-sandbox state roots, `backup-all`, backup before destroy, rebuild keeps state, `gc`, corruption diagnostics | `~/.viv` files and sqlite, checkpoints | No backup/restore, no rebuild-preserving upgrade, no gc |
| Lifecycle CLI and aliases | `nemoclaw <name> status/logs/recover/policy/channels/…`, `update`, `backup-all`, `gc`, `uninstall`; per-agent aliases (`nemohermes`, `dcode`…) | None (a GUI app; `acu` is a separate coreutils tool) | No CLI, no per-agent aliases |

## 2. Target architecture (one idea, not eight features)

Four pieces that make each other cheaper:

1. **`vivd`, a headless runtime** (same Rust crate, a `--daemon` mode). Owns bots, routines, workflows, automations and the inference gateway. The GUI becomes a client of it over a local, bearer-authenticated pipe.
2. **A local inference gateway inside `vivd`.** OpenAI-compatible on loopback with a per-launch key. It holds provider keys, applies the cost router and difficulty tiers, does fallback, records usage by model, and enforces policy. The webview and every subprocess talk to it, not to providers.
3. **An egress policy engine** in front of every network-touching tool, with the vault's secrets injected here.
4. **A `viv` CLI**, the same binary, talking to `vivd`.

Why this is better than a port: the gateway turns four separate gaps (key isolation, accounting, routing, policy) into one choke point we already half-have (the router, `usage_by_model`, the vault). It also fixes a real weakness: provider keys currently reach the webview.

## 3. Component by component

### 3.1 24x7 server → `vivd`
- Extract the agent loop's host-independent parts behind a `Host` trait (events out, approvals in), so the same core runs under the GUI or under `vivd`.
- Supervision: Windows Task Scheduler "at logon" task or a user service, `launchd` agent, `systemd --user` unit; a respawn loop with backoff inside, and a PID plus start-identity file so a stale PID is never trusted (NemoClaw's fix for #4952).
- Health: `GET /readyz` on the control pipe; `doctor` reads it. Approvals while the GUI is closed: queue them, notify through the tray and channels, never auto-approve.
- Separation: run tool execution as a restricted child (job object on Windows, a low-integrity token) so the agent cannot kill the supervisor. NemoClaw uses a separate Unix user; this is the Windows equivalent.
- Honest limit: without OpenShell/Landlock this is process hygiene, not a kernel sandbox.

### 3.2 Inference → local gateway
- `127.0.0.1:<port>/v1`, key rotated per launch, stored for local clients only.
- Provider attachment per agent (id, immutable identity); a request that fails natively does not silently fall back to another provider unless the agent's policy allows it. Fail closed on a route conflict.
- The existing difficulty router becomes a gateway stage, so subagents, teammates, MCP tools and external clients all get tiering.
- Streams through unchanged; `include_usage` forced, so costs are always recorded.
- Migration: move the TS provider's HTTP calls behind the gateway behind a flag, measure, then remove the keys from the webview path.

### 3.3 Guided onboarding → a resumable setup engine
- One `doctor` engine that returns structured checks (id, status, fix). Onboarding, Settings → Reliability, `viv doctor` and the installer all use it.
- Stages persisted to `onboard-session.json`; `--resume` after interruption; Ctrl-C leaves a clean state.
- Add the **secret-boundary check**: before saying "ready", confirm no provider key is reachable from the webview or a sandboxed child.
- Express mode for known hardware classes (DGX Spark, GB10, Blackwell desktop), driven by the catalog below.

### 3.4 Managed inference → a serving catalog
- Declarative files (schema-validated, versioned): `models/`, `presets/` (hardware class → candidate recipes with readiness requirements and priority), `recipes/` (runtime build, artifact digest, our tuning flags: `--n-cpu-moe`, KV type, speculative, parallel).
- Automatic selection: probe hardware, evaluate requirements, highest priority wins. Our existing fit rule and tuning knobs become the evaluator and the recipe's execution section.
- Seed with Nemotron 3 Nano and Ultra, NVFP4 for Blackwell, and our measured defaults. Record qualification results (tokens/s on the machine) next to the recipe.
- Pin artifacts by digest; verify before start.

### 3.5 Network policy → egress engine
- Policy as YAML in NemoClaw's shape (`network_policies`, `endpoints`, `rules`, `binaries`) so presets are interchangeable, with the four tiers and presets for our real destinations (Nebius, NVIDIA, Tavily, GitHub, npm, PyPI, Hugging Face).
- Enforcement points we control: `web_fetch`, `web_search`, MCP HTTP, the browser tool's navigations, the inference gateway. Shell and spawned processes get `HTTP(S)_PROXY` to the same engine (best effort, stated as such).
- Runtime approve/deny reuses the existing permission dialog; decisions are per session or persisted as a rule.
- SSRF guard: block link-local and metadata addresses, private ranges unless trusted, re-check after DNS resolution.
- For hard isolation, run risky tools in the existing Docker `sandbox_execute` with `--network none` or the proxy only.

### 3.6 Managed integrations → manifests plus injection
- Channel manifests (Telegram, Discord, Slack, Teams first): declare fields, secrets, policy preset, and capabilities; a compiler turns a manifest into config; add/remove/status verbs; a failed add rolls back and reports leftovers.
- **Credential injection at the egress proxy.** Tools and agents use `{{vault:github-token}}`. The engine swaps in the real value only for the destination the entry is bound to. This extends the vault from browser fills to API calls and shell commands, and is the part NemoClaw does with gateway placeholders. It must refuse injection into a response body and never log the value.
- Skills hub installs go through the same manifest pipeline (scan, pin, policy preset, status).

### 3.7 State persistence → registry, backups, rebuild
- One registry of agents and workspaces with per-agent state roots under `~/.viv`.
- `viv backup-all` (versioned archives, strict mode before any upgrade), `viv restore` (NemoClaw has no standalone restore: we should), automatic backup before destroy and before schema migrations.
- `viv gc` (orphaned state, old backups, stale downloads) with `--dry-run`.
- Diagnose a corrupt registry instead of failing with a stack trace.
- OpenShell as an optional execution backend where Docker or WSL2 exists, not a dependency: the agent core is the same, only the tool runner changes.

### 3.8 Lifecycle CLI and aliases → `viv`
- Global: `onboard [--resume|--fresh]`, `list`, `use`, `launch`, `status`, `doctor`, `update [--check]`, `backup-all`, `restore`, `gc`, `uninstall [--keep-data]`, `completion`.
- Agent-scoped: `viv <agent> status|logs [--follow]|recover|restart|policy|channels|skill|inference|rebuild|export`.
- Aliases generated per agent at creation (`viv-<agent>` shim), mirroring `nemohermes`/`dcode`; a name-first rule, with a clear message when the name is missing.
- ACP stdio adapter (`viv-acp`) so editors that speak the Agent Client Protocol can drive Vivianne.

## 4. Order of work

| Phase | Deliver | Why first | Size |
|---|---|---|---|
| A | `doctor` engine, `viv status/doctor/logs`, backup/restore/gc | Small, visible, and everything else needs a health surface | about 3 days |
| B | Egress policy for `web_*`, MCP, browser; tiers and presets; SSRF guard | Biggest trust gain; fits the existing approval flow | about 4 days |
| C | `vivd` headless runtime with supervision and `/readyz` | Makes "always-on" true | about 5 days |
| D | Inference gateway; move keys out of the webview; router becomes a stage | Needs C's runtime; biggest architectural payoff | about 5 days |
| E | Serving catalog and auto-selected recipes | Builds on the tuning work already done | about 3 days |
| F | Channel manifests, credential injection, ACP | Needs B and D | about 6 days |

For the hackathon (deadline Oct 30), A and B are the realistic scope and answer the Personal AI track's "secure" requirement directly. C is feasible if D is deferred. D to F are post-submission.

## 5. What we should not copy
- A Docker and WSL2 install requirement. Vivianne stays a native app; OpenShell is optional.
- The hosted `curl | bash` installer pattern for a desktop app. We keep the signed installer and updater.
- Process separation claims we cannot enforce on Windows without a sandbox. Say "process hygiene" until a real sandbox backs it.

## 6. Open questions
- Does `vivd` run as a per-user background process or a Windows service? User-level avoids elevation and fits a personal assistant.
- Gateway key delivery to the webview: a one-time bootstrap through Tauri IPC, then loopback only.
- Which channels matter for the demo: Telegram first (simplest token flow).
