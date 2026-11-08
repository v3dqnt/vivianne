# How you work with Vivianne: threads, trees, bots

One page that says what each way of working is for, what it can and can't do, and how we get there from today's code.

## Today, and why it confuses

A project picks one of three modes when it is made (`projects::PROJECT_MODES`):

| Mode | What it really is |
|---|---|
| Threads | Separate chats. Only one runs at a time: the app has one "primary" conversation that is re-pointed when you switch. |
| Company | The project chat *is* a master agent with a charter. It proposes hires (`hire_agent`); its specialists run as short-lived subagents (`task`) with no conversation of their own. |
| Agent team | You build bots; each has its own conversation, memory, routines and rules, one team room, DMs, and `team_run` to work a task together. |

The General tab has its own bots (the same bot code, in a folder of its own, `bots::general_root`).

Company and Agent team overlap: both are "several agents on one project", one with a boss and throwaway workers, the other with persistent workers and no boss. Users have to choose before they know what they want, and features land in one and not the other. Underneath, bots are a second copy of the chat runtime (`docs/agent-merge-plan.md`, phase 4).

## The model

**Two ways to work on a project, and bots of your own outside projects.**

| | Threads | Tree | General bots |
|---|---|---|---|
| For | Separate pieces of work you run yourself | A team that works on a project together, with a lead | Personal assistants not tied to a project |
| You talk to | Each thread | Circles and DMs | DMs and circles |
| Agents are | The base agent, once per thread | Bots with roles in an org chart | Bots |
| Who assigns work | You | The lead (or you, when there is no lead) | You |
| Agents talk to each other | Threads ask each other | Bots message, circles, the lead assigns | Bots message, circles |

### Words (one name each, used everywhere in the app)

- **Thread**: one conversation with Vivianne in a project.
- **Bot**: a persistent agent with a name, role, instructions, model, tools, rules, memory, face, routines, and optionally its own email and phone.
- **Lead**: a bot promoted to run the team or a branch of it. It can propose hires into its branch and assign work to the bots in it.
- **Sub-lead**: a lead whose branch sits under another lead.
- **Specialist**: any bot that isn't a lead.
- **Circle**: a group chat between you and chosen bots. Every tree project and the General tab have an **Everyone** circle.
- **DM**: you and one bot.

## Threads

**Use cases**
- Quick questions, one-off fixes, explanations.
- Independent work side by side: one thread refactors the API while another writes the docs and a third investigates a failing test.
- A science or maths task you want to drive step by step yourself.

**Scope**
- A project has any number of threads, and **several run at once** (today only one can).
- **Threads can ask each other.** A thread can send a message to another thread in the project and get its reply ("ask the API thread which endpoint names it settled on"). Both threads show the exchange as a note. You can also send a message to another thread from the composer with `@thread-name`.
- Not in scope: hierarchy, hiring, assignment. When you want those, use a tree.

**Rules for thread talk**
- `list_threads` shows each thread's name, status and a one-line summary; `ask_thread{thread, message}` delivers the message as a turn in that thread (queued if it is busy) and returns its reply.
- A thread that was asked can answer but not ask back in the same exchange (no loops), and a turn can ask at most 3 times.
- Asking runs under the asked thread's own permissions; approvals show in that thread.

## Tree

Today's Company and Agent team, merged.

**Use cases**
- A feature across a codebase: a lead plans it and assigns backend, frontend and test specialists, who work in parallel, each in the paths it owns.
- A research project: a lead with a literature reviewer, a data analyst and a writer; the analyst's branch has its own sub-lead with two modellers.
- A maths problem: a lead with a prover and a checker who verifies every step.
- A team you run yourself: no lead, and you hand out the work in circles and DMs.

**Scope**
- Every agent is a bot, with its own conversation (its DM), memory, routines and rules. No more throwaway specialists.
- **An org chart.** Each bot reports to another bot or to you. Promote a bot to make it a lead; a lead can have sub-leads, each running its own branch, to any depth.
- **No lead means you are the lead.** You hire, assign and approve; bots still message each other and work in circles.
- **Hiring.**
  - You can hire anywhere: pick the role and where it sits (by default, under the top lead if there is one). The lead it reports to is told ("Dee joined your branch as data analyst") and folds it into the plan and the charter's roster.
  - A lead proposes hires into its own branch. Each proposal is a card in the circle with Approve and Decline (a project setting lets leads hire directly).
- **Assigning.** A lead assigns work to the bots in its branch (directly, or through a sub-lead for that sub-lead's branch). Each assignment runs in the assignee's own conversation, so its DM shows the work; assignees work in parallel and report back to the lead (today's `team_run` engine, now driving bots). Specialists can message any teammate but can't assign.
- **Promote and demote.** Demoting or removing a lead moves its reports up to its own manager.
- **Circles.** The project opens on the Everyone circle. Make more circles for parts of the team ("Frontend crew"). A message goes to whoever it @mentions, `@everyone`, or else to whoever the router picks, as the team room does today.
- **Charter.** The project's intent stays in `.viv/charter.md`, edited by you and the top lead.
- File ownership (`owns`), the gate's guardrails and bot rules work as now.

**Privileges**

| | You | Top lead | Sub-lead | Specialist |
|---|---|---|---|---|
| Hire | anywhere | proposes, into the tree | proposes, into its branch | no |
| Assign work | anyone | anyone in the tree | its branch | no |
| Promote / demote | yes | no | no | no |
| Message teammates, post in circles | yes | yes | yes | yes |
| Edit the charter | yes | yes | no | no |

## General bots

Grok-style personal bots, outside any project.

**Use cases**
- A news digest every morning, an inbox triager with its own email address, a fitness coach, a travel planner.
- A "Morning briefing" circle where the news bot and the calendar bot post together.
- Bots helping each other: the trip planner asks the finance bot what the budget is.

**Scope**
- Create, edit and delete bots: name, role, instructions, model, tools, rules, memory, face, routines, email and phone (as now).
- DMs, an Everyone circle and circles you make.
- **Bots talk to each other**: a bot can message another General bot and get its reply, and bots in a circle hand work to each other with @mentions.
- Not in scope: hierarchy and hiring. General bots are peers.

## What changes in the code

**One foundation, used by everything above**
- **Threads run in parallel.** Every conversation command takes a thread id (abort, steer, queue, model, workspace, plan, long task, compaction, revert), and permission prompts carry their thread. This is step 1 of `docs/session-windows-plan.md`, and pop-out windows reuse it.
- **A bot is a profile plus a thread** (agent merge phase 4). A bot's conversation is a registry thread, `bot:<root>:<id>`; its DM is the normal chat view bound to that thread. `BotChat`, `BotSteps`, `LiveRunCard`, the `dm.jsonl` mirror and the bot's own trimming are deleted. `AgentProfile` gains the bot's place in the tree (lead or specialist, its branch), and the gate allows `hire_agent` and assignment only to leads, within their branch.

**Data**
- `projects.mode`: `threads` or `tree`. `company` and `team` are read as `tree` (migrated on load).
- `.viv/agents/<id>.md`: `reports_to` (exists), plus `lead: true` for a promoted bot.
- `.viv/circles/<id>.json` (name, members) and `.viv/circles/<id>.jsonl` (messages). The Everyone circle is today's `room.jsonl`.
- General bots keep their folder; it gets circles the same way.

**Migration** (on first open, with a note to the user)
- Company project: the master becomes a bot promoted to top lead (same name, charter and face); the project conversation's history becomes its DM; its specialists become bots reporting to it.
- Agent team project: a tree with no lead; the team room becomes the Everyone circle; DMs and routines stay.
- Threads projects and General bots: unchanged.

**Removed**
- The "Agent team" and "Company" cards in New project: two cards, Threads and Tree.
- The master as "the project conversation" and specialists as subagents.
- Everything bots duplicated from the chat runtime.

## Order of work

Each step ships on its own and keeps the app working.

1. **Parallel threads.** Thread-scoped commands and permission prompts. Check: two threads in one project run at once; stop, steer, revert and compaction act only on their own thread; the primary chat behaves as before.
2. **Bots as threads.** Migrate `thread.json` into registry threads; the DM becomes the chat view; delete the bot runtime copy. Check: an agent's DM has everything the chat has (plans, checkpoints, cards, model picker); routines and check-ins still post.
3. **Circles.** The room becomes the Everyone circle; create, rename and edit circles; routing per circle. In tree projects and General.
4. **Tree.** The org chart (promote, demote, sub-leads), hiring both ways, assignment within a branch, privileges in the gate, the Org chart panel (today's `CompanyTree`), the sidebar with Circles and Team. Migration of Company and Agent team projects; New project shows two cards.
5. **Thread talk.** `list_threads`, `ask_thread`, `@thread` in the composer, notes in both threads, loop and rate limits.
6. **General bots talk.** Bot-to-bot messages between General bots, and circles in General.
7. **Words and help.** The names above everywhere in the UI; one short explainer on each New project card and in each empty state.

**Tests each step:** full `cargo test`, the frontend type check, the gate's decision table (new rows for lead and specialist privileges), and a manual run in the app of what the step changed.

## Sidebar, after

```
Threads project            Tree project               General
  Threads                    Circles                    Bots
    Refactor API  ●            # Everyone                 Newsy
    Write docs                 # Frontend crew            Coach
    + New thread               + New circle               + New bot
                             Team                       Circles
                               ★ Ada · lead               # Morning briefing
                                 Ben · backend            + New circle
                                 ★ Cy · data lead
                                   Dee · analyst
                               + Hire
```
