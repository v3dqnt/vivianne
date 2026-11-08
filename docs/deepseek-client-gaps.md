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
| Context-menu registry | target-typed registry, providers raced against a 1 s timeout, grouped items; thread and file providers registered at startup | `lib/contextMenu.ts`, `components/ContextMenu.tsx`, `modules/sessions/contextMenus.ts` |
| Open in app | the editors found on PATH (VS Code, Cursor, Windsurf, Zed, Sublime, Notepad++), default app, show in folder; opens at a line where the editor allows it | `agent/system/open_apps.rs`, file right-click menu |
| Fork and archive threads | right-click a thread: fork into a new thread, archive (hidden behind "Show archived"), delete. Archive is kept per device. Fork uses the existing session-copy command and has not been run in the app | `daysStore.forkThread`, `lib/archivedThreads.ts`, `HomeView.tsx` |
| Terminal reads that don't consume | offset-addressed buffer, a per-session cursor, a `from` argument, a note when the head was dropped, and the next offset in every result | `cli_tools/output_buffer.rs`, `terminal.rs` |
| Tree kill for MCP, LSP and checker children | own process group, killed with their tree on drop, shutdown or deadline | `runtime/streaming.rs` (`own_group`, `TreeKillOnDrop`) |
| Rail tabs (Overview, Files, Preview, Terminal) | the project rail is one card with tabs; the working tabs widen it smoothly and the chat eases over to match. Files is a tree with a filter and per-file-type icons (Material Icon Theme); Preview shows one file; Terminal is a real shell (xterm over the existing PTY backend) that keeps its scrollback while another tab shows | `modules/rail/*`, `components/FileTypeIcon.tsx`, `lib/fileIcons.ts` |
| Sliding tabs | one shared segmented control (Home and the rail) with a chip that slides between tabs | `components/SegmentedTabs.tsx` |
| `/goal` in the composer | Goal left the rail; `/goal <objective>`, `pause`, `resume`, `done`, `clear`, and a quiet goal line above the composer | `modules/ai/components/GoalLine.tsx`, `lib/slashCommands.ts` |
| Thinking-states line and streaming words | the reasoning label swaps through states with a shimmer; streamed words fade in (only the last ~400 characters animate). `Markdown` now keeps one stable component map, so streamed elements are no longer rebuilt on every token | `components/ThinkingStates.tsx`, `modules/ai/lib/remarkStreamWords.ts`, `Markdown.tsx` |
| ACP server (`app acp`) | Agent Client Protocol over stdio: initialize, session/new, prompt, cancel, list, resume, close, set_config_option, permission requests; refuses client MCP servers and audio/embedded content | `cli_tools/acp.rs` |
| Scripted agents (`run_script`) | `agent()`, `phase()`, `log()` with live progress in the script card, a shared 8-at-once agent limit, a 200-agent cap, and a 30-minute ceiling | `agent/tools/ptc.rs` |
| File safety | one writer per file, edits refused when the file changed since it was read, atomic writes, streamed reads of files over 10 MB with a per-line cap | `agent/tools/files/guard.rs` |
| MCP routing and resilience | calls route by known server names (not by splitting on `__`), a failed re-list keeps a server's last tools, `tools/list_changed` re-reads, non-text results are named, server stderr is logged | `mcp/mod.rs`, `mcp/transport.rs` |
| Broken agent profiles reported | a misspelled `allowed_tools` entry or invalid frontmatter is named in `list_subagent_profiles` instead of the profile silently losing tools | `agent/profiles/subagent.rs` |
| Staged-save limits card | shell timeout and subagent count, edited as a draft: Save, a "changed from" marker, Reset, and a reason for an invalid value. The backend reads them on each use | `settings/lib/limits.ts`, `LimitsCard.tsx`, `agent/tools/limits.rs` |

## Filed: not built

Sizes: S is under a day, M a few days, L about a week or more.

### Layout and panels
| Gap | What it needs | Size |
|---|---|---|
| In-app browser tab (`ui-sidebar-browser`) | A sandboxed webview as a fifth rail tab, loopback allowed. The existing Browser panel is a different mechanism, so decide whether to reuse it | M |
| Terminal that survives a reload | The shell is kept per folder while the app runs and rejoined on return. After a window reload the screen starts empty (the process is still there); replaying its scrollback needs the offset buffer exposed to the GUI | S |
| In-app browser tab (`ui-sidebar-browser`) | A sandboxed Tauri webview per tab, with loopback allowed. The existing Browser panel is a different mechanism, so decide whether to reuse it | M |
| Panel geometry service (`ui-layout`) | Replace the fixed `RAIL_RESERVE` padding with panels that report their width. Pairs with the sidebar | M |

### Chat and composer
| Gap | What it needs | Size |
|---|---|---|
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

## Process layer (from the `subprocess` audit)

Done for `bash` and `powershell`: a timeout keeps the output so far, the drain after the loop falls under the same deadline, the whole process tree is killed on timeout or stop, each stream has its own budget, and credential names are withheld. Credentials are now also withheld from MCP, language-server and checker children, with an MCP server's own configured variables applied on top.

| Gap | What it needs | Size |
|---|---|---|
| Subagent harness children still get the full environment | Deliberately left: a harness such as Codex or Claude Code may rely on a provider key from the parent. Decide which keys each harness is allowed, then scrub the rest | S |
| Tree containment for the docker path and the subagent harnesses | MCP, LSP and checkers now kill their tree; the docker path and harness children still kill only the direct child | S |
| One spawn seam | A `ProcessRuntime` that the roughly 90 `Command::new` sites go through, so containment, env scrub and deadlines are decided once | L |
| Shared output collector | Byte-oriented, with one spill file that holds the whole stream, offset readers, and a "head was dropped" flag | M |
| PTY readiness | Foreground process and idle/busy state, so the model stops guessing with fixed sleeps | M |
| Process lifecycle tests | Spawn and collect, tree kill, drain cannot escape the deadline, terminal resize. `streaming.rs` has the shell cases; nothing covers the other call sites | M |

## MCP (from the `mcp` audit)

| Gap | What it needs | Size |
|---|---|---|
| Reconnect with backoff | A stdio child that exits or an HTTP server that fails stays dead until re-added. One retry budget per outage, doubling delay, reset after stability, and a typed status | M |
| Resources | `resources/list`, `resources/read` and templates as shared tools with an explicit `server` argument; binary payloads described, not pasted | M |
| Server instructions | Read `initialize.instructions`, bound it, attribute it, add it to the prompt | S |
| Secrets out of the config file | `mcp_servers.json` stores `env` in plaintext. Store references, resolve at runtime, redact resolved values from errors | M |
| Probe and restart | Test a server config without registering it; restart one | S–M |
| Image content passed to the model | Images are now named instead of dropped; admitting them needs a modality check and an attachment path | M |

## Not verified

- **ACP:** exercised end to end against the real binary for initialize, session/new, list, an invalid-cwd error and close. A real `session/prompt` was not run: the only configured model was rate-limited. Permission requests, cancel and resume are covered by tests against a fake host only.
- **`agent()` in `run_script`:** covered by tests against a stubbed `task` tool. Not run with real subagents.
- **Thread fork:** uses the existing session-copy command; not run in the app.
- **Streaming words, thinking states:** watched in the component gallery (Motion tab) and sampled for opacity; not watched on a real streamed reply, for the same rate-limit reason.
- **MCP changes:** unit-tested (name routing, non-text content); not run against a live MCP server.
- **File guard on shell writes:** `bash` and `powershell` can still change a file behind the guard's back; only `write`, `edit` and `apply_patch` are checked.
