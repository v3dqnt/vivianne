# Remove agent teams UI: panels, roster, hires, charter — implementations and 24/7 agents stay

**Decisions (user, Oct 30 2026):**

- **Backend is untouched.** Every Rust file, command, event and tool implementation survives: `team/{mod,master,run,runs,plans}.rs`, the hire/prompting/delegation tools in `exec.rs`, `cmd_bots_*`, `cmd_*team_*` / `cmd_*agent_run*` commands, `team:changed` / `team:run` events, notch run hooks, `projects.rs` legacy modes. No diff in `packages/gui/src-tauri`.
- **UI is what dies:** the roster panel (`TreeTeam`), the specialist strip, hire requests UI, charter/onboarding dialogs, the specialists/charter sections in the tools drawer, tree project creation, and the teams branches of Workspace/Home/Sidebar.
- **24/7 general agents untouched** — same stores, same components, same flows as today.
- Existing tree projects on disk are fine: they still open and the **lead chat still works** (prompting the lead, hiring and delegation all happen through the agent itself now — tool-driven, no human-facing panels).
- Run cards stay (`TeamRunCard` + `teamRunStore` + the `MessageBubble`/`ToolCallCard`/`toolGroups` branches) — they are chat message renderers, not panels.
- Nothing is deleted from disk; orphaned data (`mode: 'tree'` projects, `.viv/agents`, charter, runs) simply has no UI anymore.

## Phase 0 — Preconditions

- The `mode` union and `modeOf` in `projectsStore` are **not** touched: tree projects must keep resolving so the browser can open them and the chat can render. Only creation of new ones stops.
- Baseline: `npx tsc -b` green in `packages/gui-next`. No cargo/gate needed since no backend edits.
- `LEAD_CHANNEL = '__lead'` lives in `TreeTeam.tsx` but survives the roster's death (chat channel conventions). Move it before deleting that file.

## Phase 1 — Move the one shared constant, then delete the panels

Move `LEAD_CHANNEL` from `modules/team/components/TreeTeam.tsx` into `modules/team/store/teamStore.ts` (the stores file) and update the two importers that survive (`TeamRoom.tsx`, and any later use). Then delete:

| File (paths relative to `packages/gui-next/src/`) | Notes |
|---|---|
| `modules/team/components/TreeTeam.tsx` | Constant already moved |
| `modules/team/components/TeamStrip.tsx` | Specialist strip |
| `modules/team/components/HireRequests.tsx` | Hiring UI |
| `modules/team/components/TeamConversations.tsx` | Only orphaned helper for TreeTeam (`SECTION_LABEL_SX`) — check no survivor imports it |
| `modules/sessions/store/masterStore.ts` | Charter/lead UI state — backend master logic untouched |
| `modules/sessions/components/CharterDialog.tsx` | Charter editing UI |
| `modules/sessions/components/OnboardingDialog.tsx` | Teams onboarding UI |

**Do not delete:** `TeamRunCard.tsx`, `store/teamRunStore.ts`, the run-card branches in `MessageBubble`/`ToolCallCard`/`toolGroups`. `ProjectTools.tsx`, `TeamRoom.tsx`, `AgentMessenger`, `BotChat`, `BotSteps`, `WidgetLab`, `ContactTab`, `GeneralAgents`, and the whole avatar stack all survive (24/7 or chat-render surface).

## Phase 2 — Surgical edits (files that survive, teams references go)

Paths relative to `packages/gui-next/src/`:

- `modules/workspace/Workspace.tsx` — remove `TeamStrip`, `HireRequests` and the `teammate`/`TeamRoom` editor branch; the lead chat keeps working through the ordinary `ChatView`. `modeOf` stays. The left rail stays threads-only (that layout work already shrank tree projects' rail to nothing).
- `modules/sessions/components/NewProjectDialog.tsx` — remove the tree option with its specialist-count + charter step, leaving threads and plain projects. The dialog still uses `create()`'s mode param, just no longer produces tree mode.
- `modules/sessions/components/Sidebar.tsx` — remove `TreeTeam` preview + `masterStore` import; 24/7 leave-agent logic stays.
- `modules/home/HomeView.tsx` — remove the teams components/branches (`TeamActivity` and the tree tab); the "24/7 agents" tab stays.
- `modules/team/components/ProjectTools.tsx` — remove the **Specialists** and **Charter** sections and the `useMasterStore` import; the files/topics/commands sections and the `CARD`/`LABEL` exports stay. The row that opens the Git view stays.
- Everything else rides untouched.

**Regression surface to re-check:** AgentsView must behave identically (it shares `TeamRoom`, `AgentMessenger`, `BotChat`, `NewAgentDialog`). If any edit in those files changes more than a team-only branch, restore it and re-do surgically — do not refactor 24/7 surfaces in the same pass.

## Not touched (short list)

- All of `packages/gui/src-tauri` (commands, events, tools, scheduler, projects.rs, notch.rs, lib.rs registrations).
- `teamStore`, `botsStore`, `generalAgentsStore`, `faceStore`, `workflowRunner`.
- `run cards`: `TeamRunCard`, `teamRunStore`, branches in `MessageBubble`, `ToolCallCard`, `toolGroups`.
- `NotchFace`, `ThemesSection` team-face references.
- guides page `packages/vivianne-guides/projects/team.md` — it documents chat-driven team usage, which still exists.

## Verification

1. `npx tsc -b` in `packages/gui-next` green.
2. Grep `src` for the deleted names: `TreeTeam|TeamStrip|HireRequests|TeamConversations|masterStore|CharterDialog|OnboardingDialog` → zero hits (excluding the run-card cluster that stays).
3. In the dev server: open an existing tree project → it renders as a normal chat project; lead chat works; no teams panels anywhere. Home renders unchanged for 24/7 and threads flows.
4. New-project dialog: no team option; a threads and a normal project still create correctly.
