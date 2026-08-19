# Changelog

All notable changes to Vivianne are documented here.

## [1.5.0] — 2026-07-20

### Added

- **AI Elements feature set** — ported from the AI Elements toolkit:
  - **Sources** — assistant replies now show a collapsible list of the web pages a `web_search`/`web_fetch` call actually drew from.
  - **Plan** — a `present_plan` tool for the agent to lay out a numbered step-by-step plan before acting.
  - **Sandbox** — a `sandbox_execute` tool that runs untrusted code in a real, network-isolated Docker container (`--network none`, memory/CPU/PID limits, read-only mount) rather than in-process.
  - **Snippet** — a `present_snippet` tool for showing a syntax-highlighted code snippet with a description, independent of a file edit.
  - **Web Preview** — a dockable panel showing a live screenshot of a URL the agent navigated to, backed by a real browser process (see camoufox-browser below) instead of a sandboxed iframe, so it also renders sites that block embedding (e.g. Google) and can open local `file://` HTML/PDF files directly.
  - **Queue** — a floating panel showing messages queued to steer the agent mid-run or follow up after it finishes, with per-item removal and an inline "queue a follow-up" input.
  - **Checkpoints** — hover any of your own messages to revert the conversation back to that point, removing everything after it (with a confirmation dialog showing exactly how many messages will be removed).
  - **Confirmations** — a `confirm_action` tool for the agent to ask a plain yes/no question before a risky action, reusing the existing question/answer request pattern.
- **camoufox-browser integration** — web preview, web search, and web_fetch now route through [camoufox-browser](https://github.com/jo-inc/camofox-browser), a real (fingerprint-spoofed) Firefox process, instead of an iframe or plain HTML scrape. Auto-launches (`npx @askjo/camofox-browser`) the first time it's needed if it isn't already running.
- **One-click Docker install** — Settings → Engine → Sandbox now shows Docker Desktop's status and, if it isn't installed, offers to download and install it (fetched fresh from docker.com at install time, not bundled — keeps the installer small and always gets Docker's current signed build). Requires the user's explicit confirmation in-app, then Windows' own admin-elevation prompt.
- **OpenAI Responses API** — direct OpenAI calls (`provider: "openai"`) now speak `/v1/responses` instead of the legacy Chat Completions endpoint, with true incremental streaming, reasoning summaries, and correct function-call/function-call-output formatting. Every OpenAI-*compatible* third-party host (OpenRouter, Groq, xAI, DeepSeek, Together, Fireworks, Cerebras, NVIDIA NIM, local servers) stays on Chat Completions, since none of them implement the Responses endpoint.
- **Refreshed welcome screen** — replaced the placeholder `viv --model claude-sonnet` CLI command box (a leftover from before Vivianne was a GUI app) with the Vivianne logo and four clickable quick-start suggestions (explore the codebase, find/fix a bug, add a feature, refactor) that pre-fill the composer.

### Fixed

- **MCP connections failing with 406 / SSE parse errors** — the Streamable-HTTP transport wasn't sending the `Accept: application/json, text/event-stream` header the 2025-03-26 MCP spec requires, and couldn't parse SSE-formatted response bodies. Also added `Mcp-Session-Id` header tracking.
- **Automations firing at the wrong time** — the cron scheduler was computing `next_run` against UTC directly, silently reinterpreting every cron field as UTC instead of the user's local timezone (an 11:00 automation was firing at 16:30 for an IST user). Fixed by converting to local time for the schedule math, then back to UTC for storage.
- **Stale model list after adding/removing an API key** — the available-models list only ever refreshed once at app start; adding or deleting a key now refreshes it immediately.
- **camoufox-browser tab creation returning HTTP 400** (`"userId and sessionKey required"`) — the `POST /tabs` request was missing the (undocumented-as-required) `sessionKey` field.

### Changed

- Removed the dead `ExtensionUIRequest`/`sendUiResponse` scaffolding (never actually wired to any backend command) in favor of the working `confirm_action` tool.
- Diff panel now does real syntax highlighting (via Shiki) instead of plain monospace text.
