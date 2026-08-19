# VIVIANNE — Complete Engineering Handbook & Architecture Reference

**Scope:** `packages/gui` + `packages/gui-remix` (frontend + Rust/Tauri backend) — the full agentic coding harness.
**Purpose:** A complete, self-contained contextual reference for anyone working on, fixing, or extending Vivianne. Every module, every subsystem, every data flow, every file, every command, every event, every tool, every config key — and the known problems that stand between the current state and a best-in-class harness.
**Generated:** 2026-08-01 · **Version audited:** 1.5.0

> **How to read this document.** §1–§3 give the identity and the map. §4–§9 are the deep dives (frontend, backend, agent loop, tools, AI layer, memory). §10 is the full data/persistence atlas (where every byte lives). §11–§13 are the complete inventories: commands, events, tools, providers, config keys. §14–§16 are the problems, the roadmap, and the glossary. The file is written so that each section is independently usable — cross-references point at the section where a concept is fully explained.

---

## 1. What This Project Is

Vivianne is a **desktop AI coding agent** — a cross-platform (Windows/macOS/Linux) native application that lets a user chat with a large language model (LLM) and have that model *do things*: read files, edit code, run shell commands, browse the web, search code, spawn subagents, run scheduled automations, and talk by voice. It is built as a **Tauri 2 desktop app**: a Rust binary hosts the backend and opens a system webview that runs a **Next.js/React frontend**. The two sides communicate over Tauri's IPC bridge (`invoke` for commands, `listen`/`emit` for events, `Channel` for high-throughput streams).

The product name in the binary is "Vivianne (Remix)"; the user-facing codename for the rewrite effort is "supercharged monster of an agentic harness." This repo contains two GUI attempts (`gui`, `gui-remix`) plus a `trailer` package (a Remotion video trailer) and a deleted `packages/agent` TS module that has been **rewritten in Rust** inside `gui-remix/src-tauri`.

---

## 2. Repository Map

```
project root/
├── packages/gui/            # LEGACY frontend+backend (older copy)
├── packages/gui-remix/      # ACTIVE frontend+backend (the real codebase)
├── packages/trailer/        # Remotion video trailer (marketing, unrelated to runtime)
├── *.md                     # root analysis docs (jcode-*, openscience-*, this file)
├── scripts/                 # release/CI tooling for the monorepo
└── package.json             # root workspace manifest
```

### 2.1 `packages/gui-remix/` — the one that matters

```
packages/gui-remix/
├── package.json             # npm manifest (Next 16, React 19, Astryx, ...)
├── next.config.ts           # output: "export" (static SPA build)
├── tauri.conf.json          # Tauri window config, CSP, updater
├── tsconfig.json
├── AGENTS.md / CLAUDE.md    # AI-assistant instructions (Astryx + Next 16 caveats)
├── SKILL.md                 # generic coding-standards skill (vestigial)
├── Comparison.md            # parity table vs nanobot/grok-build-viv/hermes-agent-fresh  ⚠ STALE
├── todo.txt                 # duplicate of Comparison.md                            ⚠ STALE
├── docs/
│   ├── VIV-DESIGN.md             # the design system spec
│   ├── voice-mode-latency-analysis.md  # voice latency budget + roadmap (unimplemented)
│   └── session-2026-07-20-astryx-migration.md
├── src/
│   ├── app/                     # Next App Router (single page + notch route)
│   │   ├── layout.tsx           # fonts + ThemeProvider/AstryxThemeProvider/AgentProvider
│   │   ├── page.tsx             # THE shell: title bar, dock layout, modals, browser overlay
│   │   ├── globals.css          # design tokens, @theme, glass, orb glow
│   │   └── notch/               # the voice overlay window's page
│   ├── components/              # 35+ components (see §4)
│   ├── lib/                     # client utilities (modelRouter, themes, voiceEngine, ...)
│   └── (no hooks/ dir content)
└── src-tauri/
    ├── Cargo.toml               # Rust deps (see §3.2)
    ├── tauri.conf.json          # windows, CSP, updater, bundling
    ├── build.rs
    ├── capabilities/default.json
    └── src/
        ├── lib.rs               # Tauri entry: plugins, ~120 commands, setup, shortcut
        ├── main.rs              # calls app_lib::run()
        ├── agent/               # CORE: loop, tools, state, memory, usage, harness/
        ├── ai/                  # providers, auth, discovery, embeddings
        ├── coding_agent/        # grok-parity: subagents, plan mode, permissions, sandbox
        ├── orchestrator/        # multi-instance supervisor + IPC
        ├── mcp/                 # Model Context Protocol client
        ├── lsp/                 # Language Server Protocol client
        ├── voice/               # whisper.cpp STT bindings
        ├── camoufox.rs          # real-browser web preview + search
        ├── terminal.rs          # portable-pty PTY sessions
        ├── automation.rs        # cron scheduler
        ├── docker_installer.rs  # on-demand Docker Desktop install
        ├── nul                  # ⚠ STRAY FILE: Windows `> nul` redirect artifact — delete
        └── todo.txt             # ⚠ stale parity doc
```

### 2.2 `packages/gui/` — the legacy twin

A near-identical copy of `gui-remix` with drift: it has `lib/persist.ts`, `lib/localModels.ts`, uses `@tabler/icons-react` + `daisyui`, and runs on port 3000 (vs 3001). The voice-mode doc confirms they are "essentially identical." **Every change has to be made in both, or they diverge.** Consolidation is problem #1 (see §14).

---

## 3. Technology Stack

### 3.1 Frontend

| Technology | Version | Role |
|---|---|---|
| Next.js | 16.2.10 | App Router shell, `output: "export"` → pure static SPA served by the webview |
| React | 19.2.4 | UI framework |
| TypeScript | ^5 | |
| zustand | 4.5.7 | State store (one giant `AgentProvider` store) |
| motion | 12.42.2 | Animation (framer-motion successor) |
| Tailwind CSS | ^4 | CSS-first config via `@theme` in globals.css |
| Astryx (`@astryxdesign/core`) | 0.1.7 | 150-component design system (Dialog, List, Layout, Badge…) |
| Radix UI + shadcn/ui-style wrappers | 1.6.2 | `components/ui/*` primitives |
| @xterm/xterm + addon-fit | 6.0.0 | Terminal rendering |
| streamdown + @streamdown/* | 2.5.0 | Streaming markdown/block rendering |
| shiki | 3.23.0 | Syntax highlighting |
| react-resizable-panels | 4.12.2 | Dock panel layout |
| cmdk | 1.1.1 | Command palette |
| kokoro-js | 1.2.1 | In-browser TTS (ONNX) |
| @tauri-apps/api + plugins | 2.11.x | IPC, dialog, updater, process |
| @hugeicons/react | 1.1.9 | Icon set |
| diff / @types/diff | 8.0.4 | Diff computation |
| use-stick-to-bottom | 1.1.6 | Chat auto-scroll |
| ansi-to-react, react-jsx-parser, embla-carousel-react, media-chrome, @rive-app/react-webgl2, @xyflow/react, tokenlens, `ai` (Vercel SDK) | — | ⚠ appear unused / vestigial in the bundle — audit |

### 3.2 Backend (Rust)

| Crate | Version | Purpose |
|---|---|---|
| tauri | 2.11.3 | App framework |
| tokio (full) | 1.x | Async runtime |
| reqwest | 0.13.3 | HTTP (providers, web_fetch, discovery, downloads) |
| whisper-rs | 0.16 | Native whisper.cpp STT (log_backend feature) |
| rusqlite (bundled) | 0.32 | SQLite **with FTS5 compiled in** — memory store |
| keyring | 3.x | OS keychain (windows-native / apple-native / sync-secret-service) |
| tract-onnx | 0.21 | Local ONNX inference for embeddings |
| tokenizers | 0.21 | HF WordPiece tokenizer |
| portable-pty | 0.9.0 | Real PTY terminals |
| xcap | 0.9 | Screen capture (computer tool) |
| enigo | 0.6 | Keyboard/mouse control (computer tool) |
| tiktoken-rs | 0.11 | Token estimation |
| cron | 0.12 | Automation schedules |
| rand, sha2, hmac, regex, base64, uuid, chrono, serde, serde_json, serde_yaml, toml, futures, async-stream, async-trait, dirs, url, which, image, lazy_static, ndarray | — | Utilities |

---

## 4. Frontend Deep Dive (`gui-remix/src`)

### 4.1 App shell — `src/app/page.tsx`

A single `Home()` component (no routing) that composes:

- **`TitleBar`** — custom frameless OS title bar (the Tauri window has `decorations: false`).
- **Dock layout** — `react-resizable-panels` with a horizontal main `PanelGroup` and nested vertical groups. Panels dock to left/right/top/bottom zones via a `DockStack` (panels sharing a zone stack vertically). The zones are driven by `layout` state from the store (`sidebarPos`, `workspacePos`, `diffPos`, `previewPos`, `terminalPos`, `webPreviewPos`).
- **`panelLayoutKey`** — a derived string key that forces a full remount of the `PanelGroup` whenever the *set* of visible panels changes, working around `react-resizable-panels` not recomputing sibling sizes for conditionally-inserted panels.
- **`NotchManager`** — polls settings every 2s; creates/hides the `notch` webview window based on `voice.enabled`.
- **Settings modal** (`currentView === "settings"`) — Astryx Dialog.
- **Agents modal** (`currentView === "agents"`) — motion overlay containing `AgentsView`.
- **`ExtensionUIHandler`** — overlay for extension-driven UI requests.
- **Browser warning overlay** — when `__TAURI_INTERNALS__` is absent (plain webview-less browser), shows "Running in Web Browser" and blocks usage.

### 4.2 The store — `src/components/AgentProvider.tsx` ⭐ THE GOD COMPONENT

~2,000+ lines. One zustand store (`createStore` + React context) exposing ~100 fields/actions. It is simultaneously:

1. **The IPC bridge** — every backend command has a wrapped `useCallback` here (`invoke("cmd_...")`).
2. **The event reducer** — `handleBackendEvent(payload)` switches over ~20 event types from the `agent_event` channel and mutates UI state (messages, retry status, fallback banner, permission dialogs, question dialogs, confirmation dialogs, web preview, tool cards).
3. **The session/memory/MCP/skills/usage/workspace client.**
4. **The layout store** (panel positions).

Key state:
- `messages: AgentMessage[]` — the rendered transcript; `mapBackendMessage` normalizes backend `AgentMessage` (tagged union) → UI shape; a guard prevents a blank backend reload from clobbering already-streamed text.
- `state: AgentState` — model, thinkingLevel, personaMode, autoModelEnabled, sessionName, steeringMode, followUpMode, autoCompactionEnabled, autoRetryEnabled, counts.
- `availableModels`, `authenticatedProviders` — from `cmd_list_available_models` / `auth_is_authenticated` per provider.
- `permissionRequests` / `questionRequests` / `confirmationRequests` — approval dialogs fed by events.
- `subagents` — polled every 3s via `cmd_coding_list_subagents` (drives the sidebar badge).
- `memoryStatus`, `mcpServers`/`mcpTools`, `sessions`, `sessionStats`, `sessionCost`, `compactionConfig`, `workspaceFolders`/`workspaceAccessMode`, `queuedMessages`, `webPreview*`, `layout`.

Key flows:
- **`sendMessage`** — if busy, routes to **steering queue** (UI shows `queued: true`); else appends user message, sets busy, runs auto-model pick (`chooseBestModel`), invokes `cmd_agent_prompt`.
- **`steerAgent`** — invoke `cmd_agent_steer` (interrupt-and-pivot).
- **`handleBackendEvent`** — the switch. It defensively reads payload fields by **both** snake_case and camelCase (`getField`), a smell that the wire contract has drifted (see §14-S18).
- **Streaming channel** — registers a Tauri `Channel` for `streaming_bash`/`streaming_powershell` live output (chosen over `emit` per Tauri docs, since the event system "is not designed for low latency or high throughput").

### 4.3 `src/components/ChatInterface.tsx` — the chat surface

- **Composer**: auto-growing textarea, image attachments (base64), `@mention` menu for **skills + workspace files** (`UnifiedMentionMenu`; file tree flattened client-side from `listDirTree`), Enter-to-send / Shift+Enter newline.
- **Send/Steer/Abort buttons**: while generating, a violet **Steer** button and a red **Abort** button appear beside Send.
- **Quick controls row**: ModelSelectorTrigger, Thinking Level dropdown (off/minimal/low/medium/high), Persona dropdown (agent/ask/debug/explore), **AUTO/PERM** mode toggle, WorkspacePanel inline control, attach-image, context-usage (info) button.
- **Activity feed**: `groupActivityRows` clusters consecutive tool rows into collapsible `ActivityCluster`s; `ActivityRow` (memoized) renders user/assistant/tool cards. Assistant rows show collapsible `Reasoning` (thinking stream), pulsing-dots empty-stream state, `MessageResponse` markdown, and a `Sources` footer (from `attachSources` in `lib/sources.ts` — parses `web_search`/`web_fetch` results into source chips).
- **Revert checkpoint**: hovering a user message shows "Revert here" → confirmation dialog → `cmd_agent_revert_to_index`.
- **Floating widgets**: `TodoPanel` (parses `todo_write` tool cards), `QueuePanel` (visible pending steering/follow-up messages with remove buttons), retry/fallback banners.
- **Welcome screen**: logo, tagline, 4 suggestion cards, or an "Add an API key" CTA when no provider is authenticated.

### 4.4 `src/components/SettingsPage.tsx` — 13 sections

| Section id | Contents |
|---|---|
| api-keys | `ApiKeyManager` — paste key per provider, env-var hints, OAuth |
| memory | Memory provider (SQLite/in-memory), auto-compaction on/off, reserve tokens, LLM-written summary toggle, keep-last-N-images, project memory enable/purge/preview, AI extraction toggle, **memory reranker** model picker |
| engine | contextStrategy/recallWindow/stateCompression (⚠ inert), auto-retry toggle, **fallback model** picker, camoufox URL, Docker Desktop install |
| execution | Steering mode (HITL vs Autonomous), follow-up mode, bypass (auto-approve) mode, thinking level card |
| usage | `UsageSettings` — daily real token/cost from `cmd_get_usage_history` |
| skills | skill directory paths, discovery list, upload `.md` → `cmd_save_skill_file` |
| tools | enable/disable per tool → `disabledTools` in settings |
| mcp | add stdio/HTTP servers with env/headers, list tools, disconnect |
| automations | create/edit/delete/run-now cron automations with presets |
| voice | full STT/TTS tuning (see §8) |
| themes | light/dark/system + custom accent themes from `lib/themes.ts` |
| notifications | test notification, completion-notification toggle |
| about | version, updater |

### 4.5 Other frontend components

- **`Sidebar.tsx`** — session list (grouped by date), new chat, models, usage, settings entry, subagent-count badge. `RenameSessionDialog` inline.
- **`AgentsView.tsx` / `SubagentsPanel.tsx`** — subagent registry live view (status dots, elapsed, tool calls, prompt/result/error panels, kill button). ⚠ Shows `coding_agent` subagents, **not** orchestrator instances.
- **`WorkspaceFilePanel.tsx`** — gitignore-aware dir tree (`cmd_list_dir_tree`), file click → preview.
- **`FilePreviewPanel.tsx`** — read-only file renderer with Shiki highlighting.
- **`DiffPanel.tsx` / `DiffView.tsx`** — renders edit tool diffs from tool args/results (`diff` library).
- **`WebPreviewPanel.tsx`** — live screenshot of the camoufox tab with address bar / back / forward / reload.
- **`TerminalPanel.tsx`** — xterm.js backed by real PTYs (`cmd_terminal_spawn` etc.), shell picker.
- **`GlassDock.tsx` / `GlassPanel.tsx`** — frosted-glass chrome (`backdrop-blur-2xl`, `bg-white/40` light mode, solid elevated in dark).
- **`ai-elements/`** — `message`, `conversation`, `reasoning`, `persona` (animated orb for voice), `model-selector`, `context` (context-usage popover), `file-tree`.
- **`ToolCards/`** — per-tool card renderers.
- **`PromptRail.tsx`** — cluster map allowing jump-to-message.
- **`ActivityCluster.tsx`, `ImageLightbox.tsx`, `TodoPanel.tsx`, `QueuePanel.tsx`, `SkillMentionMenu.tsx`, `UnifiedMentionMenu.tsx`, `CopyButton.tsx`, `UpdateChecker.tsx`, `UsageSettings.tsx`, `ProviderIcons.tsx`, `ExtensionUIHandler.tsx`, `ThemeToggle.tsx`, `ThemeProvider.tsx`, `AstryxThemeProvider.tsx`.**

### 4.6 Design system — `globals.css` + `docs/VIV-DESIGN.md`

- **Palette**: monochrome "ink & canvas" with sky/ocean accent — `--ink` `#0c3b6d` (light) / `#ededed` (dark), `--canvas` white / `#0a0a0a`, hairline borders, surface tints. Full token table in `:root`/`.dark`.
- **Fonts**: Sora variable (sans) + JetBrains Mono, via `next/font/local` and `next/font/google`.
- **Motion**: spring cubic-bezier `[0.32, 0.72, 0, 1]` for the composer; `[0.16, 1, 0.3, 1]` for rows.
- **`@theme`** maps tokens → Tailwind v4 color utilities; shadcn semantic tokens remapped onto the palette.
- **Body background**: animated 22s sky/ocean gradient drift (light mode only); `prefers-reduced-motion` respected.
- **Glass docks**: `.glass-dock-host` makes Astryx surfaces transparent (light mode) so frost shows through.
- **Liquid orb**: `@property --lg-angle` conic rainbow halo behind the mic glyph while voice is active.
- **`lib/themes.ts`**: 8 extra accent themes injected as `data-theme` + CSS vars (light & dark variants each).
- **`lib/astryxTheme.ts`**: maps the palette onto Astryx tokens (accent = ink, pill buttons).
- **`VIV-DESIGN.md`**: full spec — double-bezel cards (28px outer / 18px inner), pill buttons with diagonal hover chip, 1280px content cap, uppercase eyebrow text, airy 128–160px vertical padding.

### 4.7 `lib/` utilities

- **`modelRouter.ts`** — client-side heuristic model picker: `tierOf()` (flagship/fast by name patterns) × `estimateComplexity()` (length, code fences, keywords) → score with context-window/cost tiebreakers. Explicitly documented as not an LLM-judged router.
- **`mentions.ts`** — `@skill`/`@file` tail-token detection + completion.
- **`sources.ts`** — parses `web_search` output ("N. Title / URL") and `web_fetch` args into per-message `sources[]`.
- **`utils.ts`** — `cn()`, Shiki language map, relative time, date grouping.
- **`voiceEngine.ts`** — whisper bridge (pre-emphasis + peak normalization) and Kokoro TTS (see §8).

---

## 5. Backend Entry — `src-tauri/src/lib.rs`

### 5.1 Plugins

`shell`, `store`, `dialog`, `updater`, `process`, `notification`, `global-shortcut` (Ctrl+Shift+V → broadcast `voice-wake` to all windows), `log` (debug builds only).

### 5.2 Setup sequence

1. **Auth state**: if OS keychain available → `AuthState::with_keychain_store()`, including one-time migration of legacy plaintext `credentials.json`. Else file-backed `credentials.json` with a loud warning.
2. **`AppState`** (primary agent, steering/follow-up queues, abort/steer flags) — managed.
3. **`CodingAgentState`** (single session) — managed.
4. **Question emitter** installed → `coding_ask_user_question` events to frontend.
5. **Orchestrator `Supervisor`** + `instances.json` storage — managed.
6. **`AutomationStore`** + `spawn_scheduler` (30s tick).
7. **`TerminalRegistry`** (PTY sessions).
8. **MCP reconnect** — async spawn, doesn't block startup.
9. **Camoufox prewarm** — background install check.
10. **Global shortcut** Ctrl+Shift+V registered.

### 5.3 Command surface (full list, grouped)

**Auth:** `auth_read_credential`, `auth_store_api_key`, `auth_store_oauth`, `auth_delete_credential`, `auth_is_authenticated`, `auth_resolve_api_key`, `cmd_list_available_models`

**Agent core:** `cmd_agent_prompt`, `cmd_agent_continue`, `cmd_agent_reset`, `cmd_agent_abort`, `cmd_agent_steer`, `cmd_agent_steering_queue`, `cmd_agent_follow_up_queue`, `cmd_agent_clear_queues`, `cmd_agent_get_queues`, `cmd_agent_remove_queued`, `cmd_agent_messages`, `cmd_agent_set_model`, `cmd_agent_set_thinking_level`, `cmd_agent_set_persona_mode`, `cmd_agent_set_compaction_config`, `cmd_agent_get_compaction_config`, `cmd_compact_now`, `cmd_agent_get_workspace`, `cmd_agent_set_workspace`, `cmd_create_workspace_folder`, `cmd_agent_set_steering_mode`, `cmd_agent_set_follow_up_mode`, `cmd_debug_agent_status`, `cmd_agent_session_stats`, `cmd_agent_revert_to_index`

**Models/settings/sessions:** `cmd_get_models_config`, `cmd_save_models_config`, `cmd_get_settings`, `cmd_save_settings`, `cmd_get_sessions`, `cmd_get_active_session_id`, `cmd_delete_session`, `cmd_load_session`, `cmd_save_current_session`, `cmd_rename_session`, `cmd_fork_session`, `cmd_clone_session`, `cmd_session_cost`, `cmd_get_usage_history`, `cmd_write_text_file`, `cmd_list_dir_tree`, `cmd_read_file_content`, `cmd_get_project_dir`, `cmd_set_project_dir`

**Permissions/UI:** `cmd_agent_respond_permission`, `cmd_agent_answer_question`, `cmd_agent_respond_confirmation`, `cmd_test_notification`

**Memory:** `cmd_memory_status`, `cmd_memory_enable`, `cmd_memory_disable`, `cmd_memory_configure`, `cmd_memory_configure_extraction`, `cmd_memory_purge`, `cmd_memory_remember`, `cmd_memory_forget`, `cmd_memory_recall`

**MCP:** `cmd_mcp_add_server`, `cmd_mcp_remove_server`, `cmd_mcp_list_servers`, `cmd_mcp_list_tools`, `cmd_mcp_authorize`

**Web preview:** `cmd_web_preview_navigate`, `cmd_web_preview_back`, `cmd_web_preview_forward`, `cmd_web_preview_refresh`

**Docker:** `cmd_check_docker_status`, `cmd_install_docker`

**Terminal:** `cmd_terminal_list_shells`, `cmd_terminal_spawn`, `cmd_terminal_write`, `cmd_terminal_resize`, `cmd_terminal_close`

**LSP:** `cmd_lsp_diagnostics`

**Orchestrator:** `cmd_orch_spawn`, `cmd_orch_list`, `cmd_orch_status`, `cmd_orch_stop`, `cmd_orch_rpc`, `cmd_orch_rpc_stream`

**Coding agent:** `cmd_coding_session_create`, `cmd_coding_prompt`, `cmd_coding_get_state`, `cmd_coding_abort`, `cmd_coding_steer`, `cmd_coding_follow_up`, `cmd_coding_clear_queues`, `cmd_coding_answer_question`, `cmd_coding_list_subagents`, `cmd_coding_kill_subagent`, `cmd_coding_plan_mode`, `cmd_coding_set_plan_mode`, `cmd_coding_permission_rules`, `cmd_coding_set_permission_rules`, `cmd_coding_register_stream_channel`, `cmd_capture_screen`

**Voice:** `cmd_whisper_preload`, `cmd_whisper_transcribe`

**Skills:** `cmd_list_skills`, `cmd_save_skill_file`

**Automation:** `cmd_automation_list`, `cmd_automation_create`, `cmd_automation_update`, `cmd_automation_set_enabled`, `cmd_automation_delete`, `cmd_automation_run_now`

---

## 6. The Agent Module — `src-tauri/src/agent/`

This is the **heart of the harness**. It is a Rust rewrite of the deleted TS `packages/agent/src/`, using an actor model with tokio channels.

### 6.1 `types.rs` — the vocabulary

- **`MessageContent`** tagged union: Text / Image / Thinking / ToolCall / ToolResult.
- **`AgentMessage`** tagged union (`type` field): User / Assistant / ToolResult / Custom. ToolResult carries persisted `arguments` (so diffs survive reloads). `Custom` **converts to empty content blocks** for the LLM — a documented trap (compaction must insert `User`, never `Custom`).
- **`ThinkingLevel`**: Off / Minimal / Low / Medium / High / XHigh.
- **`PersonaMode`**: Agent (default, unrestricted) / Ask (read-only Q&A) / Debug (read + run, no write) / Explore (read-only navigation). Each mode adds a system-prompt section **and** restricts tools.
- **`QueueMode`**: All (drain everything) / OneAtATime.
- **`ToolExecutionMode`**: Sequential / Parallel (per-tool override).
- **`AgentTool` trait**: name/label/description/parameters_schema/prepare_arguments/async execute (with `on_update` partial-result callback)/execution_mode. Hook context types: `BeforeToolCallContext`, `AfterToolCallContext`, `ShouldStopAfterTurnContext`, `PrepareNextTurnContext` — a hook surface exists but is not yet exposed to third-party extensions.
- **`ModelRef`**: id/name/api/provider/base_url/reasoning/max_tokens/context_window.
- **`AgentContext`**: system_prompt + messages + `delegate_depth` (delegation capped at 1 level).

### 6.2 `state.rs` — thread-safe state

`AgentState` wraps an `Arc<RwLock<AgentStateInner>>` with: system_prompt, model, thinking_level, persona_mode, messages, compaction config (enabled / reserve_tokens 16384 / keep_recent_tokens 20000 / llm_summary_enabled / image_keep_last_n 4), workspace_folders (first = primary, drives relative path resolution), `WorkspaceAccessMode` (Full/ReadOnly — ReadOnly restricts the same tool set as persona mode).

### 6.3 `events.rs` — the event bus

Tokio `mpsc::UnboundedSender`/`Receiver`. Full event set (snake_case `type` field):

| Event | Meaning |
|---|---|
| `AgentStart` / `AgentEnd{messages}` | run lifecycle |
| `TurnStart` / `TurnEnd{message, tool_results}` | turn lifecycle |
| `MessageStart` / `MessageUpdate{message, chunk}` / `MessageEnd{message}` | message lifecycle + streaming deltas |
| `ToolExecutionStart{tool_call_id, tool_name, args}` / `ToolExecutionUpdate{partial_result}` / `ToolExecutionEnd{result, is_error}` | tool progress |
| `PermissionAsked{request_id, tool_call_id, tool_name, args, reason}` / `PermissionResolved{approved}` | gated-tool approval |
| `QuestionAsked{questions}` / `QuestionResolved` | `question` tool MCQs |
| `ConfirmationAsked{message, danger_level}` / `ConfirmationResolved` | model-initiated yes/no |
| `WebPreviewRequested{url, screenshot_base64, error}` | open the preview panel |
| `RetryStatus{attempt, max_attempts, delay_ms, reason}` | transient retry (buffered — never pollutes transcript) |
| `ModelFallback{from, to, reason}` | switched to fallback model (persists until agent_end) |

### 6.4 `loop_core.rs` — THE LOOP ⭐

`run_agent_loop_with_prompts` → `run_agent_loop_continue`. Structure mirrors the old TS `agent-loop.ts` two-level loop:

- **Constants**: `MAX_STEPS = 100` (hard ceiling; last step gets a "wrap up now" system-reminder nudge, past it a synthetic `stop_reason:"length"` message), `DOOM_LOOP_NUDGE_THRESHOLD = 3`, `DOOM_LOOP_STOP_THRESHOLD = 5`, `TODO_NUDGE_THRESHOLD = 8`, `MAX_REQUEST_BYTES = 4_000_000`.
- **Per iteration**:
  1. Check abort flag → synthetic "Stopped by user." message.
  2. Check steer flag → drain steering queue and pivot immediately.
  3. Step ceiling enforcement (nudge/stop).
  4. Inject pending steering/follow-up messages.
  5. `maybe_compact_context` preflight (token-budget check, cheap unless over).
  6. Resolve API key: exact provider → sibling providers (nvidia↔nvidia-nim) → env var.
  7. `prune_aging_tool_results` + `evict_old_images` (context shaping).
  8. Convert `AgentMessage`s → `ai::Context`; fetch tool definitions (exclude `delegate` when `delegate_depth >= 1`).
  9. Byte-size safety net: >4MB → evict images, then drop oldest half of history.
  10. `call_provider_with_fallback` (see below) → AssistantMessage.
  11. Extract tool calls; doom-loop signature tracking (name+args, key-order-independent); todo nudges.
  12. `execute_tool_calls` (parallel/sequential) → tool results into context+state.
  13. Post-tool compaction check.
  14. Emit `TurnEnd`.
- **Outer loop**: only drains the **follow-up queue** once the inner loop would otherwise stop.
- **After the run**: turn-close memory auto-capture (`build_turn_memory_summary` → `auto_capture`), maybe-trigger L1 extraction, completion notification, `AgentEnd`.

**Reliability stack** (all in loop_core):
- `call_provider_with_retry` — up to 4 attempts, backoff 1/2/4/8s (cap 16s) + ±20% jitter, transient-only classifier (`is_retryable_assistant_error`), buffered as `RetryStatus`.
- `call_provider_with_fallback` — after retries exhausted, one attempt on user-configured `fallbackModel` (never on user-abort); `ModelFallback` event.
- `maybe_compact_context` — LLM-authored structured summary (Goal/Constraints/Progress/Decisions/Next-Steps) or deterministic Mermaid tool-call diagram; `force: true` path for the manual Compact button.
- `resolve_api_key` — documented `.await`-only (never `block_on` on the runtime — panics).
- Streaming uses a **Tauri event round-trip**: `call_provider_stream_and_await` listens for `ai-stream-token` events tagged by `stream_id`, feeds `StreamTokenEvent`s through an mpsc, and collects the final message via oneshot. (This is the one place the frontend's `Channel`-based streaming for bash differs — bash uses `Channel`, LLM tokens use `emit`.)

### 6.5 `tool_exec.rs` — the tool registry + safety gates

- **`GATED_TOOLS`** (require approval when bypass mode off): bash, powershell, write, edit, apply_patch, computer, web_fetch, web_search, generate_image, streaming_bash, streaming_powershell, connect_mcp_server, create_automation, sandbox_execute. Pure reads (read, ls, grep, find, glob, sleep, todo_write) are never gated.
- **`PLAN_MODE_GATED_TOOLS`**: write, edit, bash, powershell, apply_patch, notebook_edit, streaming_bash, streaming_powershell — refused outright in plan mode, checked *before* the approval wait.
- Pending-permission / pending-question / pending-confirmation registries (oneshot senders) resolved by the frontend-facing commands.
- Tool execution dispatches over the registered `AgentTool`s, supports `on_update` partial-result streaming, and applies plan-mode/permission/workspace-readonly/persona-mode merged restrictions (`disabled_tools`).

### 6.6 `agent.rs` — the `Agent` facade

High-level struct: `prompt()` / `continue_run()` / `steer()` / `follow_up()` / `clear_queues()` / `reset()`, holding `AgentState` + queues + `AgentHarness`. The steering/follow-up queues are `pub(crate)` Arcs **cloned out** at construction so steer commands never block on the agent mutex (held for the whole run).

### 6.7 `commands.rs` — the Tauri surface (see §5.3)

Notable details:
- `cmd_agent_prompt` rebuilds the system prompt each turn from a **stripped base** (splits off previously-injected `<vivianne_directive>`, `<vivianne_mode>`, `<vivianne_skills>`, `<vivianne_mentioned_skills>`, `<vivianne_workspace>`, `<project_instructions>`, `<project_memory>`) then re-appends fresh blocks — preventing injection stacking. Image attachments get a "N image(s) attached — look at them" prime text (models otherwise refuse vision).
- Session persistence: `cmd_save_current_session` writes `~/.viv/agent/sessions/<id>.json` (plain JSON — see §14-S4).
- `cmd_agent_revert_to_index` truncates in-memory + persisted session.
- Workspace changes mid-conversation inject a visible `User` message so the model notices.

### 6.8 `harness/` — vestigial second harness ⚠

`env.rs` (LocalFileSystem/LocalShell), `fs.rs`, `shell.rs`, `session.rs`, `prompt_templates.rs`, `message_helpers.rs`, `utils.rs`, `compaction.rs`, `skills/`. Used only by the orchestrator instances. The main agent path bypasses it. **Duplication to reconcile.**

### 6.9 Supporting modules

- **`usage_store.rs`** — real provider-reported Usage per completed turn; daily aggregates + per-`source_tag` breakdown (`main:<session>`, `delegate:<id>`, `task:<id>`), persisted to `usage_by_source.json`. `cmd_session_cost` sums any tag ending in the session id (includes subagent rollup).
- **`directive.rs`** — `assemble_capability_blocks` (shared with subagents): the always-on capability directive, mode framing, skills catalog, mentioned skills, workspace block, project instructions.
- **`project_instructions.rs`** — loads AGENTS.md/CLAUDE.md-style project context files.
- **`notifications.rs`** — desktop "turn complete" notification.
- **`paths.rs`** — canonical dirs: `~/.viv/agent/`, `~/.viv/skills/`, `~/.viv/camoufox/`.
- **`proxy.rs`** — server-routed LLM proxy stream (legacy path).

---

## 7. The AI Layer — `src-tauri/src/ai/`

### 7.1 `provider_registry.rs` — routing truth ⭐

One table: provider id → (`default_base_url`, `ApiKind`). `ApiKind` determines which stream function runs (the *wire format*), provider identity only matters for key lookup/billing.

| ApiKind | Providers |
|---|---|
| `AnthropicMessages` | anthropic |
| `OpenAiChat` | groq, xai, deepseek, together, fireworks, openrouter, cerebras, nvidia, nvidia-nim, **ssm-moe** (local, no default URL, OpenAI shape) |
| `OpenAiResponses` | openai (direct only) |
| `GoogleGenerative` | google |
| `MistralChat` | mistral |
| `Bedrock` | bedrock (no default URL — region-specific) |
| `AzureOpenAiResponses` | azure-openai-responses (no default URL — deployment-specific) |

Historical bug fixed by this registry: `"azure-openai"`/`"azureOpenai"` arms never matched the real id `"azure-openai-responses"`, so Azure models fell into the OpenAI default and 404'd at `api.openai.com`.

### 7.2 `model_discovery.rs` — live catalogs

For every provider with a resolvable credential, calls the real `/models` endpoint (OpenAI-style for most; Anthropic and Google get dedicated fetchers with `display_name`/`inputTokenLimit`). Capabilities (vision/image/audio) are **pattern-matched** (`classify_capabilities`), not authoritative. Providers that fail are silently skipped. Azure/Bedrock/HuggingFace are intentionally not auto-discovered.

### 7.3 `auth.rs` + `keychain_store.rs` — credentials

- `CredentialStore` trait with serialized per-provider read-modify-write. Implementations: `InMemoryCredentialStore`, `FileBackedCredentialStore` (0600 on Unix), `KeychainCredentialStore` (OS keychain via `keyring`).
- `resolve_provider_auth`: override → stored → ambient env. OAuth refresh via double-checked locking (⚠ refresh-inside-lock limitation, see §14-S14).
- `ApiKeyResolver` (EnvApiKeyAuth), `OAuthHandler`, `AuthContext` (injectable env/file probes).
- Tauri commands: read/store/delete/is_authenticated/resolve + `cmd_list_available_models`.

### 7.4 `api/` — the 12 wire implementations

`anthropic.rs` (Messages API with `thinking` budget), `openai_completions.rs` (Chat Completions + streaming + tool calls), `openai_responses.rs` (Responses API), `openai_shared.rs` / `openai_prompt_cache.rs` (⚠ caching helpers exist but no end-to-end cache-key strategy), `transform_messages.rs` (AgentMessage→provider payload), `google.rs` / `google_shared.rs`, `mistral.rs`, `azure_openai.rs`, `bedrock.rs`, `cloudflare.rs`, `github_copilot_headers.rs` (⚠ Copilot-flavored auth header — appears unintegrated), `simple_options.rs`.

### 7.5 `embedding.rs` + `local_onnx.rs` — local embeddings ⭐

- Local path: `all-MiniLM-L6-v2` ONNX via `tract-onnx`, downloaded once to `~/.vivianne/models/onnx-minilm/`, ~3–5ms CPU inference, 384-dim, offline, no key. Mean-pooling + L2 norm. `OnceLock` engine, LRU (128-entry, FNV-1a keyed).
- Remote fallback: OpenAI `text-embedding-3-small`.
- `embedding_model()` switches by availability; memory rows are tagged with their model so cross-model cosine never happens.

---

## 8. Voice Mode — full pipeline

**Files:** `src/components/VoiceMode.tsx` (orchestrator), `src/lib/voiceEngine.ts` (bridge), `src-tauri/src/voice/mod.rs` (whisper.cpp), plus the `notch` window and global shortcut.

**Flow:**
1. **Passive listening**: mic → `ScriptProcessorNode` (deprecated; default 2048 frames ≈128ms @16kHz) → RMS energy VAD (default threshold 0.006) → ring buffer keeps last 3s pre-roll → every `wakeCheckInterval` (default 600ms) while voice is detected, the whole buffer is transcribed by whisper and checked against wake words (`vivianne,viv,vivian`).
2. **Wake → session**: `agentReset()` (fresh thread), `playWakeCue()` (80ms sine blip — the latency doc's "no greeting" fix is **already partially implemented**), arm command capture.
3. **Command capture**: buffer all chunks; finalize after `silenceTailMs` (default 500ms) of silence or `maxUtteranceMs` (15s).
4. **Transcription**: `voiceEngine.transcribe` applies **pre-emphasis** (`y[n]=x[n]-0.95·x[n-1]`) + **peak normalization** to boost whisper accuracy, then `cmd_whisper_transcribe` → native whisper.cpp on `spawn_blocking`.
5. **Agent**: `voicePrompt(command)` wraps the command with voice-behavior rules (brief conversational reply, screenshot only when needed, tools allowed) → same `sendMessage` path as chat.
6. **TTS**: sentence-boundary streaming — speak each completed sentence via **Kokoro-82M** (in-browser ONNX, ~330MB, `kokoro-js`) or browser SpeechSynthesis fallback; prefix-dedup prevents re-speaking on reload.
7. **Linger** 3.5s (120s safety timeout) → back to passive. Notch window hides during passive listening, resizes 96↔240px while replying.

**Latency budget (from the repo's own analysis doc):** wake ~1.4–2s, greet ~0.6–2.5s, silence tail ~0.9s, transcribe ~0.25–1.1s, agent 2–25s, TTS ~0.25–1.7s/sentence → **6–12s simple command, 8–25s+ with tools**. The doc's full optimization roadmap (keyword spotting, AudioWorklet, Rust-side TTS, raw binary IPC, adaptive silence tail, TTS caching/hybrid) is **unimplemented** — several sliders in Settings (energy threshold, chunk size, pre-emphasis) already exist to tune the current path.

---

## 9. The Coding-Agent Layer (grok-parity) — `src-tauri/src/coding_agent/`

Ports the model-facing surface of grok-build onto Vivianne's in-process `Tool` trait. **Overlaps with `agent/`** — both define tools/sessions/compaction; the UI drives the `agent/` path via `cmd_agent_*`, while `coding_agent/` provides the subagent/permission/plan surface. This is the biggest internal duplication to reconcile (see §14-S3).

- **`orchestration.rs`** — `SubagentRegistry` (process-wide, in-memory), `task`/`task_output`/`wait_tasks`/`kill_task` tools; `ask_user_question` (2–4 MCQs, GUI emitter → oneshot responder); plan mode (`PlanModeState` global atomic flag + plan text; mutating tools gated); subagent runner hook (`install_subagent_runner`) with a stub fallback.
- **`permissions.rs`** — per-tool `Rule` manager, exposed to UI via `cmd_coding_permission_rules`/`set`.
- **`streaming.rs`** — `StreamBatch` channel for streaming_bash/powershell (Tauri `Channel`).
- **`sandbox.rs`** — env **allowlist** (strip `*_KEY/_TOKEN/_SECRET/_PASSWORD/_CREDENTIAL*` suffix vars) + **destructive-command denylist** (`rm -rf /`, `format C:`, `diskpart`, `mkfs`, fork bomb, `dd of=/dev/sd*`). Explicitly documented as "a static string/regex guard, not a real jail."
- **`skills.rs`** — markdown frontmatter loader (nested `SKILL.md` convention, skips `references/scripts/assets/.git/node_modules`), `requires_provider` dependency gating (resolved in `cmd_list_skills`), lightweight catalog vs full body on mention, `@mention` overrides `disable_model_invocation`.
- **`apply_patch.rs`** — unified diff application (string-exact context matching, conflict-aware).
- **`compaction.rs`** — token estimation, cut-point finding, LLM/Mermaid summary generation (used by `loop_core`).
- **`bash_executor.rs` / `exec.rs`** — command execution with env filtering.
- **`computer.rs`** — `xcap` screenshots (+ `enigo` input) for the `computer` tool; `cmd_capture_screen`.
- **`tools_enh.rs`** — enriched read/list, `IgnoreSet` (gitignore), code_search.
- **`context_shaping.rs`** — `prune_aging_tool_results`, `evict_old_images` (used by the main loop).
- **`opencode.rs`** — OpenCode-style shortcuts/UX.
- **`session.rs` / `session_manager.rs`** — a parallel session abstraction (⚠ duplicates `agent` sessions).
- **`repo_tools.rs`, `memory_tools.rs`, `tools_ext.rs`** — `glob`, `powershell`, `notebook_edit`, `web_fetch` (HTML→text), `web_search` (camoufox first, DuckDuckGo Lite fallback), `sleep`, `diagnostics` (LSP), `todo_write`.
- **`sandbox.rs`/`trust.rs`/`settings.rs`/`messages.rs`/`system_prompt.rs`** — trust-state, settings manager, message builders, `build_system_prompt` ("You are Vivianne, an expert coding assistant…").

---

## 10. Orchestrator / MCP / LSP / Camoufox / Terminal / Automation

### 10.1 `orchestrator/` — multi-instance supervisor

`Supervisor` manages live `InstanceRecord`s, each backed by its **own independent `Agent`** (`new_with_cwd_and_model`) — parallel agents with separate histories, models, and cwds. `InstanceStorage` persists to `instances.json`. `ipc_protocol.rs` defines `OrchestratorRequest`/`Response` (Spawn/List/Status/Stop/Rpc/RpcStream) and `RpcCommand` (Prompt/GetState/NewSession/…). `handle_rpc` runs **real full turns** and returns the final answer. ⚠ `open_rpc_stream` ignores its session-event callbacks (stub), and **no UI surfaces the orchestrator** — the Agents view only shows `coding_agent` subagents.

### 10.2 `mcp/` — Model Context Protocol client (from scratch)

JSON-RPC 2.0 over stdio (newline-delimited) or HTTP; OAuth 2.1 + PKCE for remote servers. `McpConnection::connect` → `initialize` → `list_tools`; tools namespaced `mcp__<server>__<tool>`; configs persisted to `~/.viv/agent/mcp_servers.json` (⚠ plaintext incl. stdio env secrets — documented accepted limitation) and reconnected at startup; `connect_mcp_server` tool is gated.

### 10.3 `lsp/` — Language Server Protocol client

Auto-detects language servers on PATH per extension (typescript-language-server, pyright, rust-analyzer, gopls, clangd, …), spawns lazily, aggregates `textDocument/publishDiagnostics` into `<diagnostics>` blocks surfaced after edits (the `diagnostics` tool + `cmd_lsp_diagnostics`).

### 10.4 `camoufox.rs` — real browser

Manages a persistent `~/.viv/camoufox/` install of `@askjo/camofox-browser` (Camoufox — a Firefox fork with fingerprint spoofing), launched as a detached process, base URL `http://localhost:9377`. Used for (a) **web preview** — a real separate browser navigating + screenshot relay, which bypasses X-Frame-Options/CSP that would block an iframe, and (b) **web_search** via the `@google_search` macro with accessibility-snapshot extraction. `ensure_running` handles install → launch → 2-min readiness wait; `prewarm` runs at startup; falls back to DuckDuckGo Lite scraping when unhealthy. Single shared tab (`CURRENT_TAB`).

### 10.5 `terminal.rs` — real PTYs

`portable-pty` sessions per tab; shell picker (PowerShell / Git Bash / cmd on Windows; bash/zsh elsewhere); output pumped to `terminal-output` events, exit watcher emits `terminal-exit`; frontend renders with xterm.js. In-process registry, no persistence (a shell doesn't survive restart anyway).

### 10.6 `automation.rs` — cron scheduler

"Run this prompt through the agent on this cron schedule." 5- or 6-field cron (local-timezone math — a documented UTC-offset bug fix), 30s poll loop, emits `automation_fired`; **the frontend** listens and forwards the prompt to `cmd_agent_prompt` only when idle (the scheduler deliberately never grabs the agent mutex). Fire-time imprecision ≤30s is accepted.

### 10.7 `docker_installer.rs`

On-demand Docker Desktop install (fresh download from docker.com at install time, not bundled): Windows via `Start-Process -Verb RunAs --quiet --accept-license`, macOS via `open` (drag-to-install). Gated behind explicit UI confirmation. Status probe: `docker --version` + `docker info`.

---

## 11. The Memory System — full pipeline ⭐ (the differentiator)

**Storage** (per workspace root): `<root>/.vivianne/memory/`
- `state.json` — enabled, autoConsolidate, extractionEnabled, timestamps.
- `entries.jsonl` — legacy append-only store (kept as migration source + safety net).
- `memory.db` — **SQLite** with `memories` table (+ confidence, access_count, last_accessed_at, embedding, embedding_model, scene_name), **FTS5 virtual table** with triggers, and `memory_edges` table. Graceful schema migrations via `PRAGMA table_info` + ALTER. One-time JSONL→SQLite migration.
- `persona.md` — L3 output.

**Write path**: `remember` (explicit, redacted) / `auto_capture` (turn-close, deterministic summary, deduped, secret-redacted) → append + insert + background embed.

**Retrieval path** (`recall`):
1. Hybrid candidates: FTS5 BM25 (`"t1" AND "t2"` AND-joined, `bm25()` ASC) ‖ brute-force cosine over vector rows (same `embedding_model` only).
2. **RRF merge** (`rrf.rs`, k=60).
3. **Confidence decay** × category half-life (corrections 365d, preference/persona 90d, episodic/entity 60d, instruction 180d, auto/fact 45d) × logarithmic access boost.
4. **Dynamic two-threshold gate** (`dynamic_gate_select`): rel floor 0.90, drop ratio 0.95, abs floor 0.25 — score-relative, not fixed top-K.
5. Optional **LLM listwise reranker** (`memory_rerank.rs`): single call ranks candidates, can return `[]` (empty-gold); **topic-stability gate** (cosine < 0.30 → force rerank; min cadence 3 turns) amortizes to ~1 call / 1.4 turns.
6. **BFS graph cascade** (`memory_graph.rs`): typed edges (HasTag 0.8, InCluster 0.6, RelatesTo w, Supersedes 0.9, Contradicts 0.3, DerivedFrom 0.7), decaying cumulative weights, max depth 3, floor 0.3.
7. Access touch (feeds the boost).

**Maintenance** (`memory.rs`): L1 batch extraction (EXTRACTION_BATCH_SIZE=5, LLM distills `auto` notes into persona/episodic/instruction facts + scene tags), L3 persona synthesis (PERSONA_BATCH_SIZE=10 → persona.md), scene-tag edge inference, background embedding, `context()` injects `<user_persona>` + `<project_memory>` (with "Recent topics" from L2 scene summary), capped at 8KB.

**Memory tools**: `remember`, `forget`, `recall` (memory_tools.rs), plus the `memory_status/enable/disable/configure/configure_extraction/purge` commands. The reranker model is user-selectable in Settings → Memory (a `sidecarModel` in settings.json).

---

## 12. The Complete Tool Inventory

**Base tools** (`tool_exec.rs` + `tools_ext.rs` + `repo_tools.rs` + `tools_enh.rs`): read, write, edit, apply_patch, bash, powershell, streaming_bash, streaming_powershell, ls, grep, find, glob, code_search, web_fetch, web_search, sleep, todo_write, notebook_edit, diagnostics, generate_image, computer, delegate, sandbox_execute, connect_mcp_server, create_automation, delete_automation, set_automation_enabled.

**Grok-parity tools** (`orchestration.rs` + `opencode.rs`): task, task_output, wait_tasks, kill_task, ask_user_question, enter_plan_mode, exit_plan_mode.

**MCP tools**: `mcp__<server>__<tool>` for every connected server.

**Gating matrix** (three orthogonal restriction sources merged in tool_exec):
1. `disabledTools` (user Settings → Tools).
2. Persona mode restricted lists.
3. Workspace ReadOnly mode.
Plus: `GATED_TOOLS` approval (bypass mode off), `PLAN_MODE_GATED_TOOLS` hard refusal.

---

## 13. Configuration & Data Atlas (where everything lives)

| Path | Contents |
|---|---|
| `~/.viv/agent/settings.json` | settings: bypassMode, lastModel, fallbackModel, sidecarModel, autoRetryEnabled, disabledTools, skills[], camoufoxUrl, compaction{}, engine{}, voice{}, memoryProvider, notifications… |
| `~/.viv/agent/models.json` | user model config (providers map) |
| `~/.viv/agent/sessions/*.json` | per-session transcripts (plain JSON) ⚠ not a DB |
| `~/.viv/agent/automations.json` | cron automations |
| `~/.viv/agent/mcp_servers.json` | MCP configs (incl. stdio env secrets) ⚠ plaintext |
| `~/.viv/agent/usage_by_source.json` | real token/cost history by day+source |
| `~/.viv/skills/` | user skills (SKILL.md / *.md with frontmatter) |
| `~/.viv/camoufox/` | persistent camoufox-browser install |
| `<workspace>/.vivianne/memory/` | state.json, entries.jsonl, memory.db, persona.md |
| `~/.vivianne/models/onnx-minilm/` | local ONNX embedding model |
| `app_data_dir/credentials.json` | ⚠ plaintext key fallback (only when keychain unavailable) |
| OS keychain | API keys (Windows CredMan / macOS Keychain / libsecret) |

**Environment variables** (`env_api_keys.rs`): per-provider `*_API_KEY` conventions; used as last-resort key source and by discovery.

---

## 14. Shortcomings & Risks (the "fix Vivianne" list)

### 14.1 Structural
- **S1 — Two duplicate GUI packages** (`gui` + `gui-remix`). Every change must be applied twice; drift already exists (deps, lib files, port). Consolidate to one.
- **S2 — AgentProvider monolith** (~2,000+ lines, one zustand store, `any`-heavy with file-wide eslint disables). Split into domain stores.
- **S3 — Internal duplication/dead code**: `agent/harness/` (unused by main loop), `coding_agent` sessions/compaction vs `agent` equivalents, `test_loop.rs` ad-hoc, `proxy.rs`, `github_copilot_headers.rs` unintegrated. `Comparison.md`/`todo.txt` are **stale** — they list fallback-model/compaction/memory/plan-mode/doom-loop as missing, but all are implemented.
- **S4 — Sessions are JSON files**, no durable DB: no WAL, no atomic rename, no crash safety, no checkpoint/resume.
- **S5 — Zero frontend tests**; no full-loop integration/e2e.

### 14.2 Harness gaps (per the parity targets)
- **S6 — No checkpointing / resume.**
- **S7 — No end-to-end prompt-cache management** (`openai_prompt_cache.rs` exists but no cache-key strategy; Anthropic `cache_control` not wired; `SessionStats.cacheRead/cacheWrite` are hardcoded 0).
- **S8 — Single credential per provider** — no pool rotation on 429 (only retry + one fallback model).
- **S9 — No stale-request timeout detector** — hung streams wait for provider timeouts.
- **S10 — Sandbox is heuristic, not a jail** — env allowlist + denylist regex only; no OS-level jail on Windows; `sandbox_execute` requires Docker Desktop.
- **S11 — Orchestration half-built** — RPC stream stub, no orchestrator UI, delegation depth capped at 1, subagent tool events not streamed to UI.

### 14.3 Operational / bugs
- **S12 — Dev port mismatch**: `package.json` runs `next dev --port 3001`; `tauri.conf.json` `devUrl` says `http://localhost:3000`. Dev mode likely fails to load — verify & align.
- **S13 — Stray `nul` file** in `src-tauri/src/` (Windows `> nul` artifact) — delete; harden gitignore.
- **S14 — OAuth refresh gap** (`resolve_stored_oauth` can't refresh inside the locked `modify` callback — handler not `'static`).
- **S15 — Memory root is `current_dir`** — memory is per-process-cwd, not per-workspace-folder; implicit and undocumented.
- **S16 — MCP secrets in plaintext** (documented accepted).
- **S17 — Voice latency** (see §8 roadmap — largely unimplemented).
- **S18 — Wire-contract drift** — `handleBackendEvent` reads both snake_case and camelCase defensively; sessions round-trip multiple result shapes ("backend and forwarded payloads have evolved independently"). Needs one typed IPC schema.
- **S19 — Vestigial/unused frontend deps** — `@rive-app/react-webgl2`, `@xyflow/react`, `react-jsx-parser`, `media-chrome`, `ansi-to-react`, `embla-carousel-react`, `tokenlens`, `ai` (Vercel SDK), etc.
- **S20 — Inert Settings knobs** — Engine section (contextStrategy/recallWindow/stateCompression) explicitly "may not yet change runtime behavior."
- **S21 — No mock IPC for browser testing** — UI is unusable without Tauri (`__TAURI_INTERNALS__` guard), hampering web-based dev/tests.

---

## 15. Recommendation Roadmap (to "Supercharged Monster")

**Phase 0 — Consolidate & stabilize (1 wk):** merge `gui` into `gui-remix`; fix S12/S13; kill S3 dead code; refresh stale docs; add frontend vitest + mock IPC.
**Phase 1 — Durable state (1–2 wk):** SQLite SessionDB (reuse bundled rusqlite pattern from memory store), WAL + atomic writes, checkpoints + resume, credential pools (S8), stale-request detector (S9).
**Phase 2 — Context economics (1–2 wk):** prompt-cache management end-to-end (S7) incl. real cacheRead/cacheWrite in SessionStats; wire or remove inert Engine knobs (S20).
**Phase 3 — Trust & safety (2–4 wk):** real OS sandboxing (Windows Job Objects/restricted tokens, seatbelt, bubblewrap); MCP secrets vault (S16); fix OAuth refresh (S14).
**Phase 4 — Orchestration UI (2–3 wk):** surface the orchestrator supervisor; stream subagent events live; lift delegation cap with proper subagent tree + cost rollup (S11).
**Phase 5 — Voice (1–2 wk):** implement the repo's own latency roadmap (keyword spotting, AudioWorklet, Rust TTS, raw binary IPC, adaptive silence).
**Phase 6 — Differentiators (ongoing):** public hook/callback API for extensions; LLM-judged router opt-in; deterministic replay harness for CI.

---

## 16. Glossary

- **Agent loop** — the inner while-loop that alternates LLM call → tool calls → LLM call until no tools remain.
- **Steering vs follow-up** — steering interrupts a running turn immediately; follow-ups run at the natural turn boundary.
- **Doom loop** — the model repeating the same tool call with identical args; detected by signature tracking and nudged/stopped.
- **Compaction** — folding old context into a summary when near the token ceiling (LLM or Mermaid).
- **Context shaping** — per-turn pruning (aging tool results, evicting old images) independent of full compaction.
- **RRF (Reciprocal Rank Fusion)** — merging ranked lists by `Σ 1/(k+rank+1)`.
- **BM25 / FTS5** — SQLite full-text search scoring (lexical).
- **Embedding / cosine** — vector similarity (semantic).
- **Hybrid memory search** — lexical + semantic fused, gated by confidence decay and a dynamic score-relative threshold.
- **L1/L3 extraction** — LLM distillation of raw turn notes into typed facts (L1) and persona synthesis (L3).
- **Reranker + topic-stability gate** — LLM listwise relevance judge, skipped when the query topic hasn't shifted.
- **Grok-parity** — feature parity with xAI's grok coding agent (task tool, plan mode, ask_user_question).
- **Bypass/PERM mode** — auto-approve tool calls vs. per-call approval dialogs.
- **Plan mode** — a global flag that hard-refuses mutating tools until exited.
- **Astryx** — the 150-component design system the UI is built on.
- **Notch** — the small always-on-top voice window.
- **Camoufox** — fingerprint-spoofing Firefox fork used as the "real browser" for preview/search.
- **PTY** — pseudo-terminal: a real interactive shell session.
- **MCP** — Model Context Protocol: standard way to connect external tools/servers.
- **LSP** — Language Server Protocol: editor-grade language diagnostics.
- **Tauri Channel** — high-throughput IPC for streaming data (vs. events for discrete messages).

---

## 17. File-by-File Index (gui-remix)

| Path | Role | Verdict |
|---|---|---|
| `src/app/page.tsx` | Shell, dock layout, modals, notch manager, browser overlay | Solid; heavy inline panel logic |
| `src/app/layout.tsx` | Fonts + provider chain | Fine |
| `src/app/globals.css` | Tokens, @theme, glass, orb, sky gradient | Excellent |
| `src/components/AgentProvider.tsx` | God store + IPC + event reducer | Split (S2) |
| `src/components/ChatInterface.tsx` | Composer + activity feed + mentions | Strong |
| `src/components/SettingsPage.tsx` | 13 sections | Strong; remove inert knobs |
| `src/components/VoiceMode.tsx` | Voice pipeline | Roadmap exists (S17) |
| `src/components/SubagentsPanel.tsx` / `AgentsView.tsx` | Subagent live view | Good; wire orchestrator (S11) |
| `src/components/GlassDock.tsx` / `GlassPanel.tsx` | Frosted chrome | Nice |
| `src/components/ai-elements/*` | Message/Conversation/Reasoning/Persona/ModelSelector/Context | Polished |
| `src/lib/modelRouter.ts` | Auto model pick | Heuristic; upgrade path noted |
| `src/lib/voiceEngine.ts` | Whisper/Kokoro bridge | See S17 |
| `src/lib/themes.ts` | 8 custom accent themes | Nice |
| `src-tauri/src/lib.rs` | Entry + ~120 commands + setup | Clean |
| `src-tauri/src/agent/loop_core.rs` | The loop | ⭐ Core asset |
| `src-tauri/src/agent/tool_exec.rs` | Tools + gating | ⭐ Core asset |
| `src-tauri/src/agent/memory*.rs` (+ store/graph/rerank/rrf) | Memory pipeline | ⭐ Differentiator |
| `src-tauri/src/ai/*` | Provider layer + registry + discovery + embeddings | Strong |
| `src-tauri/src/coding_agent/*` | Grok parity + sandbox + skills | Strong; dedupe with agent/ |
| `src-tauri/src/orchestrator/*` | Multi-instance supervisor | Half-built UI-side (S11) |
| `src-tauri/src/mcp/*` | MCP client | Good; secrets gap (S16) |
| `src-tauri/src/lsp/*` | LSP client | Good |
| `src-tauri/src/voice/mod.rs` | whisper.cpp | Good |
| `src-tauri/src/camoufox.rs` | Real browser | Clever |
| `src-tauri/src/terminal.rs` | PTYs | Good |
| `src-tauri/src/automation.rs` | Cron | Good |
| `src-tauri/src/docker_installer.rs` | Docker bootstrap | Convenient |
| `docs/VIV-DESIGN.md` | Design spec | Great |
| `docs/voice-mode-latency-analysis.md` | Latency roadmap | Implement it |
| `Comparison.md` / `todo.txt` | Parity tracking | ⚠ Stale — refresh or delete |
| `src-tauri/src/nul` | Stray artifact | Delete (S13) |

---

## 18. Bottom Line

Vivianne already has the hardest 80% of a monster harness: a real agent loop with anti-runaway guards, steering, subagents, permissions, plan mode, and a genuinely advanced hybrid memory system — all in fast, safe Rust behind a polished glass UI. The path forward is **not a rewrite**; it is: (1) kill the duplication, (2) make state durable, (3) cut context costs with prompt caching, (4) close the trust gap with real sandboxing, (5) finish the orchestration story, and (6) execute the voice roadmap that's already written in the repo. Sections §14–§15 are the checklist.
