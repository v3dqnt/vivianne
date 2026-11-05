# Vivianne: current features and planned updates

Last updated 2026-11-05. Reflects the `ui-polish` worktree, where most recent work is **uncommitted**. "Built" means the code exists; anything not yet run in the live app is marked.

Vivianne is a native desktop AI agent (Tauri 2, Rust, React). You talk to it in a chat, and it works on your projects, your computer and your browser, with any model. Windows is the primary platform; macOS and Linux build, but computer control is Windows-only.

## Current features

### Chat and threads
- **Top tabs:** open threads sit in the title bar. `+` or Ctrl+Shift+N starts a blank thread, and tabs come back after a restart.
- **Projects:** a blank thread can be filed under a registered or new project from the composer's project pill. Home lists threads by project.
- **Summary titles:** a model names each thread after its first message.
- **Project banners:** preset, uploaded picture or gradient, shown on Home.
- **Project rail:** the collapsible right card, with workspace, version control and a context breakdown.
- **Composer:** one card holding the text and controls: attach, project, mode, context ring, thinking level, model, dictation, send/stop/steer.
- **Failed turns:** shown in the chat as a yellow "Stopped here" card.
- **Long tasks:** rate limits are waited out, failing models fall back, dropped connections retry, and tasks resume after a restart.
- **Progress:** checkpoints, folded tool groups, and a run summary with time, tokens and cost.
- **Plan mode:** research and a plan you can comment on before anything changes.

### Agents
- **24/7 agents:** always-on assistants with apps, browser, screen, web, documents, memory and a file shell, but no coding tools. They have schedules and inline approval cards, and stop works mid-work.
- **Coding agents over ACP:** agents such as Claude Code are installed once into `~/.viv/agents/<id>`, listed under Settings > Providers, and have a Test/debug view. See the open issues below.
- **Older team and company projects:** still open and chat.
- **Memory:** standing instructions and facts, recalled when relevant.
- **Self-written tools:** when no tool fits, the agent writes, tests and keeps one.

### Tools and computer use
- **Computer control (DIP):** Windows UI Automation with pixel fallback, plus a notch that shows activity and a Stop button.
- **Your own browser:** through the extension, on any Chromium browser.
- **Tool search and `run_native`:** Rust/C++ scripts for fast multi-file changes, with usage guides in `vivianne-guides/tools`. Built, not yet exercised live.
- **Image generation:** local (stable-diffusion.cpp) and cloud models, picked directly or through a tool. Built, not yet exercised live.
- **Files:** preview for code, images, video, audio, 3D, Markdown, HTML, CSV, PDF and editable Office documents.
- **Also present:** MCP, skills, mods, multiplayer pairing, Git view with review, and Nebius serverless jobs (`nebius_job`).

### Models
- **Providers:** the major cloud providers and any OpenAI-compatible endpoint, your ChatGPT plan through sign-in, and local models through llama.cpp.
- **Routing:** cost-aware `viv-auto` (beta), with usage tracking by model.

### Design
- Solar icons, with a design-scale guard (`scripts/design-scale.mjs`) and a palette contrast test.

### Disabled
- **Viv Instant:** turned off 2026-10-25 as unstable. Keep it out of pitches and demos.

## Planned updates

### Threads as agents (`docs/threads-worktrees-plan.md`)
Next is B1, the riskiest step.
| Phase | What it adds |
|---|---|
| B1 | Thread ids and thread-scoped backend commands |
| B3 | A git worktree per thread, on its first turn, with terminal and merge/delete (the module exists) |
| B4 | Threads message each other with receipts, plus `spawn_thread` and lineage |
| B5, B5b | Coding agents in worktrees, and thread tools through an MCP bridge |
| B6 | Convert old day and conversation sessions to threads, with a backup first |
| B7 | Thread event log with cursors, before the Android client |
| B8, B9 | Forks, side chats, handoffs and thread lifecycle |

### ACP provider layer (`docs/acp-provider-layer-plan.md`)
P0, the managed install, is done.
| Phase | What it adds |
|---|---|
| P1 | Keep what the agent's `initialize` reports |
| P2 | Process-tree teardown |
| P3 | Visible sign-in |
| P4 | Sessions that survive a restart |
| P5 | Flavors |
| P6 | Models and effort from the agent |
| P7 | Vivianne's tools inside the agent (same work as B5b) |
| P8 | A proper registry |
| P9 | A mock agent for tests |
| P10 | Richer events |

### Other planned work
- **Queue system:** rebuild it.
- **Compaction:** fix auto-compaction. Both a beta watcher model and the model's own self-compaction are wanted; the prototype awaits a verdict.
- **Agent products:** 24/7 agents (lively, memory, apps, design) and the coding agent (pure reasoning, fast long-horizon) get developed as two products.
- **Agents refactor:** merge the second chat runtime into one base agent (`docs/agent-merge-plan.md`).
- **Session pop-out windows:** blocked on the thread-scoped backend (`docs/session-windows-plan.md`).
- **Devlab 24/7:** setup, Linux build and remote connection (`docs/devlab-24x7.md`).
- **`viv` CLI:** benchmark runner and release console are missing.

## Open issues
- Claude Code sign-in had expired at last test, so a real ACP thread turn is still unverified. Codex and Gemini installs are untested.
- The new tab flow hasn't had a real message sent through it.
- `Sidebar.tsx`, `ThreadsSidebar` and the `ProjectRail` component are mostly dead code awaiting cleanup.
- Solar icons (CC BY 4.0) still need visible attribution.
- `README.md` is out of date: it mentions the composer spend limit (removed from the UI) and creating team/company projects.
