# deepseek-harness `packages/client` vs Vivianne `gui-next`

Surveyed 2026-11-07. The reference is about 75 packages under `deepseek-harness/packages/client/`; ours is `packages/gui-next/src`.

**How this was done.** Four Haiku agents surveyed the packages in groups. They read each package's README and file list, and only some of the source. A few claims were spot-checked against our code by grep. Treat each verdict as a pointer to go and look, not as proof.

## Architecture in one paragraph

deepseek's client is a **plugin host**.
- A framework-free boot kernel (`web`) loads per-package bundles declared in `dsh.client` manifests (`modules`).
- Features fill typed **slots** (`ui-slots`) and register into registries for tools, context menus, sidebar tab types and preview renderers.
- Stores are React-free (`store`), and traffic runs over a reconnecting `connection` layer.

Vivianne is **one Tauri React app**.
- `App.tsx` statically imports `modules/*`.
- Every backend call goes through `ipc/index.ts`.
- About 47 zustand stores hold state, and features import each other directly.

Theirs is built to be extended by third parties. Ours is faster to change and has no extension seams. Most "behind" rows below come from that difference, not from missing polish.

## Where Vivianne is ahead

| Area | Vivianne | Why it's ahead |
|---|---|---|
| Themes | `palettes.ts` (12 palettes), `ThemeBuilder.tsx`, contrast checks | deepseek has light/dark/system and font sizes only |
| Scientific/rich renderers | `modules/ai/components/sci/`, `Model3DBlock.tsx`, Office viewers (`modules/office`) | no equivalent seen |
| Plan tracking | `modules/plan` (versions, per-step state, approve/cancel) | deepseek shows a plan chip and cards, with no step tracking |
| Change review | `ChangeSummary.tsx`, `ReviewPanel.tsx` with undo/redo | deepseek has review without undo |
| Attachments | `lib/attachments.ts` detects text vs binary and tells the model to use its read tool | not seen in deepseek |
| Shortcut engine | `lib/shortcuts.ts` (recording, clash check, overrides) | parity on the engine; the grouped settings list is good |
| Deliverables | `present` tool card in `ToolCallCard.tsx` (built this week) | parity; the survey's "behind" predates it |

## Where Vivianne is behind (grouped, highest value first)

### Chat and composer
- **Slash-command registry** (`ui-commands`, `ui-input-trigger/src/core/{detect,menu}.ts`). We have no `/` menu; `@` mentions only, with no Tab-to-drill into folders. A registry covering `/model`, `/permission`, `/goal` and skills unblocks several other rows.
- **Credential masking in sent bubbles** (`ui-secret-mask/src/client/credential-{scanner,formats}.ts`). Display only: copy and the model input keep the raw text.
- **Question card in the composer seat** (`ui-user-questions/src/client/{draft-store.ts,QuestionComposer.tsx}`). Drafts survive reload, there's a timer, and defaults are recommended. Ours is a modal, `AskQuestionDialog.tsx`.
- **Approval lock** (`ui-approval/src/client/{ApprovalPanel,ApprovalCard}.tsx`). A withdrawn or repeated request can't be answered twice, and there's an Always-allow scope. Ours: `PermissionDialog.tsx` modal with autofocus on Allow, plus `team/ApprovalWidget.tsx`.
- **Permission presets** (`ui-permission-presets`): Read only / Workspace write / Full, a separate default for new sessions, and a risk acknowledgement before Full. Ours: Ask/Auto/Plan in `ModePill.tsx`.
- **Message feedback** (`ui-message-feedback`): thumbs pair plus a dialog. None in ours.
- **Session mentions** (`ui-reference`): `@session` as an atomic reference. Ours does files only.
- **Uploads with progress and cancel** (`file-upload`, `ui-attachment`). Ours inlines content at send.
- **Exit-code capsule on failed command rows** (`ui-tool`). This is small and pairs with the shell work.

### Layout and side panels
- **Right-sidebar tab system** (`ui-sidebar-right/src/client/shell/{SidebarRight.tsx,tab-registry.ts,tab-domain.ts}`). One docked surface per session, with tab types that plugins can add. `Workspace.tsx` states there are no side panels; the project rail is fixed.
- **Terminal in the sidebar** (`ui-sidebar-terminal/src/client/{terminal.tsx,TerminalRecovery.tsx}`). It uses xterm.js, runs in the session cwd, and recovers processes after a reload. We have no interactive terminal; `shellTaskStore.ts` only replays tool runs.
- **In-app browser tab** (`ui-sidebar-browser`), for loopback dev servers in a sandboxed tab. Our links open the system browser.
- **File tree per pane** (`ui-sidebar-files`), which keeps expansion and scroll across tabs. We have none.
- **Preview renderer registry** (`ui-sidebar-documentpreview/src/client/document/{registry,unviewable}.ts`). Our `FilePreview.tsx` hard-codes `renderedKind`; replace it with a registry and a plain-text fallback.
- **Open in app** (`ui-open-in-app/src/client/{file-applications.ts,OpenInAppAction.tsx}`). The submenu lists installed apps. Ours, `FileLink.tsx`, only reveals the file in Explorer.
- **Context-menu registry** (`ui-context-menu/src/client/{registry,types}.ts`), keyed by target type with a 1 s resolve timeout. Ours wires right-click by hand.
- **Thread row actions** (`ui-workspace`): fork, archive, reorder, and a pending-interaction dot. Not found in `ThreadsSidebar.tsx`.
- **Panel geometry service** (`ui-layout`). Ours uses a fixed `RAIL_RESERVE` padding.

### Agents and runs
- **Continuable subagents** (`ui-subagent/src/client/{SubagentHeaderLineage.tsx,sidebar-chat/index.tsx}`). A nested catalog, follow-up prompts to a child, and stop. Ours: `SubagentPanel.tsx` is a flat list, polls every 2 s, and is read-only plus kill. The backend tree, `message_parent` and interrupt now exist, so this is UI work only.
- **Jobs list** (`ui-jobs/src/client/JobListAction.tsx`): streamed output per job and a two-press kill. Pairs with the Background button and background tool runs.
- **Schedule manager** (`ui-schedule/src/client/{editor-store.ts,TaskManagerPage.tsx,DeliveryHistory.tsx}`): saves are checked against the revision, delivery history is kept, there's run-now, and RRULE/cron. Ours: `RoutineDialog.tsx` per agent.
- **Workflow-run tree** (`ui-workflow-run/src/client/{workflow-definition.ts,WorkflowRunPanel.tsx}`): run, then phase, then member, rebuilt from events. Ours runs steps one after another with no tree view.
- **Team task board** (`ui-agent-team`), beside the subagent tree.
- **Session presets** (`ui-agent-preset`), named presets with a read-only config viewer.
- **Goal activation state** (`ui-goal`). Our controls in `ThreadGoal.tsx` match theirs. The gap is showing live activation separately from the stored goal.

### Settings and platform
- **Localisation** (`locale/src/locales/*`): typed dictionaries per namespace, 8 languages. We have no i18n.
- **Staged-save settings cards** (`ui-settings-shell/src/client/{shell-card-controller.ts,ShellCard.tsx}`). Edits are held until Save, an "overridden" marker shows, and you can reset to default. Use the same card for shell limits, agent-loop parallelism and subagent depth/width (`ui-settings-subagent/src/client/SubagentLimitsFields.tsx`).
- **Shared settings client** (`ui-settings`). Each of our sections calls IPC directly.
- **Fuzzy ranker** (`ui-primitives/src/rank-by-name.ts`): an ordered-subsequence match with prefix hits first and stable ties, for the `/` and `@` menus.
- **Plugin market** (`ui-plugin-market`): install is disabled with a reason when no version is compatible, and asks for confirmation. Plugin inventory (`ui-settings-plugin-inventory`) shows failures.
- **Trajectory at scale** (`ui-trajectory/src/client/{trajectory-virtual-rows,trajectory-search-index,timeline}.ts`): virtual rows, search, and a drag filter on the timeline. Ours, `modules/trajectory`, is fine for short runs.
- Lower priority: the `modules`/`hmr`/`slots` plugin loader, `resources` (URL-addressed live values), `product-analytics` (event allowlist), `connection` (only matters if a remote client ever exists).

## Recommended order

1. **Slash registry plus fuzzy ranker.** It's small, and other work depends on it: `/model`, `/permission`, `/goal` and skills all hang off it.
2. **Approval lock and composer-seat question card.** These fix real double-answer and lost-draft risks.
3. **Right sidebar with tab registry.** Once it exists, the terminal, browser, file tree and preview each become one tab type rather than four layout changes.
4. **Jobs list and continuable subagents.** The backend already exists; this puts it in front of the user.
5. **Staged-save settings cards** for shell, loop and subagent limits.
6. **Credential masking and the exit-code capsule.** Both are small and quick to do.
7. **i18n**, only if non-English users are a goal.

Skip unless we open Vivianne to third-party plugins: the plugin loader, HMR, slots, the dockkit split engine, and `connection`.
