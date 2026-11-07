# deepseek client gaps: built, and filed for later

Follows [deepseek-client-gap-analysis.md](deepseek-client-gap-analysis.md). The first section is what was built from it. The second is the gaps still open, each with what it needs and a rough size, so each can be picked up on its own.

## Built

| Gap | What exists now | Where |
|---|---|---|
| Slash-command registry | `/` menu with `/plan`, `/permission`, `/goal`, `/model`, `/stop`; skills in the same picker; the registry takes a context object, so it has no React in it | `lib/slashCommands.ts`, `lib/rankByName.ts`, `modules/ai/components/SlashMenu.tsx`, wiring in `Composer.tsx` |
| Fuzzy name ranker | ordered-subsequence ranking, with exact, prefix, substring and scattered letters in that order | `lib/rankByName.ts` |
| Credential masking | keys, tokens, PEM blocks and `PASSWORD=…` values are hidden in sent bubbles; copy and the model's text keep the original | `lib/secretMask.ts`, `MessageBubble.tsx` |
| Question drafts | selections survive a reload or a thread switch, per request id | `lib/questionDraft.ts`, `AskQuestionDialog.tsx` |
| Safer approval focus | Deny holds the focus, so a stray Enter cannot approve a tool call. The store already clears the request on the first answer, which gives the single-answer lock | `PermissionDialog.tsx` |
| Exit-code capsule | a finished shell row shows `exit N`, `timed out` or `killed`. A non-zero exit is no longer drawn as the tool failing | `modules/ai/lib/shellOutcome.ts`, `ToolCallCard.tsx` |
| Subagent tree | children under their parent, the agent's name, a plain-words stop reason, and a two-press stop | `modules/ai/lib/subagentTree.ts`, `SubagentPanel.tsx`; backend `name`, `parent`, `stop_reason` in `cmd_coding_list_subagents` |
| Writing to a running subagent | the transcript has an input; the message reaches the child at its next step | `cmd_coding_message_subagent`, `subagents::message_from_user` |
| Jobs list | background commands with status, a two-press stop, and the last output when they end | `agent/tools/background.rs` (`cmd_jobs_list`, `cmd_job_stop`, `cmd_jobs_clear`), `JobsPanel.tsx`, `jobsStore.ts` |
| Staged-save limits card | shell timeout and subagent count, edited as a draft: Save, a "changed from" marker, Reset, and a reason for an invalid value. The backend reads them on each use | `settings/lib/limits.ts`, `LimitsCard.tsx`, `agent/tools/limits.rs` |

## Filed: not built

Sizes: S is under a day, M a few days, L about a week or more.

### Layout and panels
| Gap | What it needs | Size |
|---|---|---|
| Right-sidebar tab system (`ui-sidebar-right`) | Decide where it docks relative to the project rail and `RAIL_RESERVE`. Add a tab-type registry (terminal, browser, files, preview) and a per-thread tab set that survives switching threads. Everything below hangs off this | L |
| Interactive terminal (`ui-sidebar-terminal`) | xterm.js on the frontend. A PTY backend (`portable-pty`) with a retained-process list so tabs survive a reload. Needs the sidebar tabs first | L |
| In-app browser tab (`ui-sidebar-browser`) | A sandboxed Tauri webview per tab, with loopback allowed. The existing Browser panel is a different mechanism, so decide whether to reuse it | M |
| File tree per pane (`ui-sidebar-files`) | A tree component fed by `cmd_list_dir_tree`, with expansion and scroll kept per pane. Needs the sidebar tabs | M |
| Preview renderer registry | `FilePreview.tsx` hard-codes four kinds. It's a small refactor, but only worth doing together with the sidebar, when previews gain a second home | S |
| Panel geometry service (`ui-layout`) | Replace the fixed `RAIL_RESERVE` padding with panels that report their width. Pairs with the sidebar | M |
| Thread row actions and a pending dot (`ui-workspace`) | Fork needs a backend operation that copies a thread's events. Archive and reorder need a field on the thread record. The dot reuses existing busy/approval state | M |

### Chat and composer
| Gap | What it needs | Size |
|---|---|---|
| Open in app (`ui-open-in-app`) | Backend enumeration of installed editors and apps on Windows, macOS and Linux, then a submenu on `FileLink` | M |
| Context-menu registry (`ui-context-menu`) | A target-typed registry with a resolve timeout, then move the hand-wired right-click in `FileLink.tsx` onto it | M |
| Message feedback (`ui-message-feedback`) | A thumbs pair plus a dialog. Needs a place to store the feedback and a decision about where it goes | S–M |
| Session mentions (`ui-reference`) | `@thread` candidates and an atomic reference token. The backend already has multi-thread `read_thread` | S |
| Uploads with progress and cancel (`file-upload`) | Today files are inlined at send, so there's nothing to upload. Only worth it if attachments move to a staged-receipt model | M |
| Drag from the transcript into the composer (`ui-file-drag`) | A drag source on image blocks and a drop token in `ChatComposer` | S |
| Tab to drill into folders in the `@` menu (`ui-input-trigger`) | Keyboard handling in `MentionMenu` and `Composer`; the ranked file index already returns directories | S |
| Always-allow scope on approvals | Needs a stored allow rule on the backend, keyed by tool and path pattern | M |
| Permission presets (`ui-permission-presets`) | Read only, Workspace write and Full need real sandbox levels behind them, not labels. Today's modes are ask, auto and plan | M–L |

### Agents and runs
| Gap | What it needs | Size |
|---|---|---|
| Schedule manager (`ui-schedule`) | A standalone page for the routines now in `RoutineDialog`: revision-checked saves, delivery history and run-now | M |
| Workflow-run tree (`ui-workflow-run`) | Durable run and phase events from `workflowRunner.ts`, then a tree rebuilt from them | M |
| Team task board (`ui-agent-team`) | A shared task list beside the subagent tree, and a backend list for the team to write to | M |
| Session presets (`ui-agent-preset`) | Named bundles of tools, profile and prompt, plus a read-only viewer | M |
| Goal activation state (`ui-goal`) | Show live activation apart from the stored goal. The backend has the phase; the UI needs an "idle / working / waiting" line | S |
| Streaming output inside the jobs list | `run_streaming` pushes lines to a channel keyed by tool, not by call. Key them by call id first | S–M |

### Settings and platform
| Gap | What it needs | Size |
|---|---|---|
| i18n (`locale`) | Choose a library. Extract the strings from every component into typed namespaces. Persist the choice | L |
| Shared settings client (`ui-settings`) | One client with schema validation and atomic multi-field save, replacing the direct `cmd_get_settings` / `cmd_save_settings` calls in each section. `LimitsCard` is the first user of the staged pattern | M |
| Agent-loop parallelism setting (`ui-settings-agent-loop`) | The backend has no knob for parallel tool calls. Add one, then a third row in `LimitsCard` | S |
| Subagent depth and width limits | Depth is a fixed limit in the backend; width (`maxActiveSubagents`) is done. Add a `maxSubagentDepth` setting the same way | S |
| Plugin market and inventory | Only relevant if mods get an index and versions. `ModsSection` shows local mods | L |
| Trajectory at scale (`ui-trajectory`) | Virtual rows, a search index and a drag filter on the timeline. Do this when real runs grow past a few hundred steps | M |
| Plugin loader, HMR, slots, dockkit, connection | A different architecture from ours. Skip unless Vivianne opens to third-party plugins | — |
