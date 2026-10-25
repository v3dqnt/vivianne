# Drag a session out into its own window

Goal: drag a project or thread out of the sidebar and it becomes a standalone window, like a browser tab. Several windows work in parallel and nothing running is interrupted.

## What blocks it today (verified in code)
The agent's conversation is a single *primary* thread. Opening another project re-points it (`openScope`, `syncWorkspaceToProject`). Non-primary threads exist in Rust (`AgentRegistry::resolve(Some(id))`, as Viv Instant uses) but most of what a conversation needs is still primary-only. `agentStore.ts` says so itself:

| Needs a thread id | Today |
|---|---|
| Abort | `cmd_agent_abort` has no thread; secondary `abort()` is a no-op |
| Steer / follow-up queue | primary-only; secondary ignores sends while busy |
| Model, thinking level, workspace folders, access mode | set on the primary state |
| Persistence and "project's one conversation" | bound to the primary via `openScope` |
| Plan, long task, compaction, revert, subagent panel | gated by `isPrimary` |
| Permission / question dialogs | emitted globally; need a thread filter per window |
| Provider streaming | answered only by the `main` window; that must stay true |

So a pop-out cannot be a view of the same thread (events carry only a thread id, and the primary is shared). It needs its own thread id per project.

## Plan
1. **Backend thread scoping (about 2 days).** Add `thread_id` to abort, steer, queues, set-model, set-workspace, plan and long-task commands; resolve through `AgentRegistry`. Primary behaviour stays the default (`None` means primary), so nothing existing changes.
2. **Thread-bound conversations (about 1 day).** A project's conversation is addressed by `proj:<id>` instead of "whatever the primary has open"; the sidebar opens the primary onto it as now, a window opens its own.
3. **Window shell (about 1 day).** `WebviewWindow` labelled `session-<id>` loading the app with `?thread=proj:<id>`; the window's `useAgentStore` is `createAgentStore(thatId)`. `main.tsx` already keeps the provider dispatcher in `main` only. Drag from a sidebar row creates it (drop outside the window, or a "Open in new window" menu entry as the reliable fallback).
4. **Parallel safety.** Per-project workspace lock (two windows on one project share a folder: warn, never auto-merge), a cap on concurrent turns, and per-window permission prompts.

Out of scope for the first cut: dragging a window back into the main window, and multiplayer projects.

## Risk
Step 1 touches the hot path of every conversation, so it needs the existing agent tests green and a manual check that the primary chat is unchanged before the window shell is added.
