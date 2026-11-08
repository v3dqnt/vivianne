# Threads as agents: sidebar and banner, summary titles, thread ids, thread-to-thread messages, a worktree per thread

Status: APPROVED 2026-11-04, in progress (see Progress). Track A is done; Track B is built in the same worktree because Track A no longer touches Rust files, so a second worktree would only add a merge.
Builds on: `docs/session-windows-plan.md` (thread scoping, its steps 1-2 are Phase B1 here), `docs/teams-removal-plan.md` (teams UI already removed), `docs/agent-merge-plan.md` (one base agent), `docs/design` scale work on branch `ui-polish`.

## What the code does today (verified)

| Area | Today | Where |
|---|---|---|
| Thread title | First 30 characters of the first user message, copied literally (`text.chars().take(30)`) | `runtime/commands.rs` `save_current_session_impl` |
| Thread identity | One live *primary* thread (id `''`) that is re-pointed when a session opens; sessions are files `session_<ms>.json`. Extra live threads exist only for pop-outs and Viv Instant (`AgentRegistry::resolve(Some(id))`) | `runtime/commands.rs` `AgentRegistry`; `ai/store/activeThread.ts` |
| Thread scope | A thread belongs to a project by `workspace_folders[0]` in its `SessionInfo` | `threadsOf` in `ThreadsSidebar.tsx` |
| Worktrees | `cmd_git_checkout(path, branch, pr, worktree=true)` makes `<repo>-<branch>` beside the repo and reuses an existing one (`worktree_of`) | `runtime/commands.rs` |
| Messaging between agents | `ask_agent` (24/7 agents by @id), `message_agent`/`team_post` (members of one team run). Nothing addresses a thread | `tools/schemas.rs` |
| Project picture | `Project.icon`, a `data:image/...` URL under 256 KB in `projects.json` (`cmd_set_project_icon`) | `agent/sessions/projects.rs` |
| ACP agents | A thread whose model is `harness/<id>` runs in an ACP child process; `cwd` is the primary workspace folder; `mcpServers: []` | `agent/harness.rs` |

## Two tracks, one contract

- **Track A (UI)** lives in worktree `viv-main-gui-exp-ui-polish`, branch `ui-polish`. It needs no backend change except a banner field (A2.1).
- **Track B (backend)** is risk-first and goes in its own worktree `viv-main-gui-exp-threads`, branch `threads-core`, cut from the same snapshot so the two never fight over files. Merge both into `agent-merge` at the end.

The contract between them (so A can be built against a stub before B lands): a thread row gets `{ id, title, titleSource: 'auto'|'user', branch?: string, worktree?: string, status: 'idle'|'running'|'waiting', unread: number }`.

## Phases

### Phase A1: Sidebar cleanup, new-chat icon, contrast (UI) — Size M
Verified when: before/after screenshots of the thread screen with real data show the sidebar rows, header and empty state on the design scale and readable (≥4.5:1 for every text level), and `npm test` + `tsc` are green.
Files: `modules/sessions/components/ThreadsSidebar.tsx`, `modules/workspace/Workspace.tsx`, `modules/team/components/ProjectTools.tsx`.

#### Step A1.1: Sidebar structure and the icon-only new chat (Size S)
- Acceptance: the "New chat" row becomes one icon button (square-and-pencil, tooltip "New chat", `aria-label`, Ctrl/Cmd+N) in the header; search is an icon that expands inline; the Threads list starts directly under the banner (A2); one row style (28px, 13px, active = selected tint + accent bar), the change count and status dot share one right-aligned slot.
- Verification: `npx tsc -p tsconfig.app.json --noEmit`; `npm test` (scale + palettes guards); CDP screenshot of the thread screen.
- Blocked by: none.

#### Step A1.2: Empty, loading and hover/focus states (Size S)
- Acceptance: empty state says how to start; skeleton matches row shape; every row has a visible keyboard focus ring; delete confirmation reads the same as elsewhere.
- Verification: same commands; a screenshot of each state.
- Blocked by: A1.1.

### Phase A2: Discord-style project banner (UI + one backend field) — Size M
Verified when: each project shows a banner header at the top of its threads sidebar with the project name over it and a dropdown (rename, change banner, change icon/colour, open folder, project settings, delete); projects with no banner show a gradient built from the project colour; General has its own default.

#### Step A2.1: Banner field, storage and commands (Size S) — Blocked by: none
- Acceptance: `Project.banner` (a file name under `~/.viv/project-banners/`, or `preset:<id>` for a built-in one); `cmd_set_project_banner(id, Option<data URL>)` validates `data:image/`, caps size, writes the file, and returns the stored reference; deleting a project removes its banner; old `projects.json` loads unchanged (`#[serde(default)]`).
- Verification: `cargo test --no-default-features --lib projects` (new tests: set/clear/oversize/old-file-loads).
- Test seam: `projects.rs` functions over a temp dir.

#### Step A2.2: The header component and its menu (Size M) — Blocked by: A2.1, A1.1
- Acceptance: 112px header, cover-fit image, 55% bottom scrim so the name stays ≥4.5:1 over any image, name 16px/600, verified-style tick omitted (not our concept), chevron opens the menu; keyboard operable; collapsed-sidebar form is the project initial on the banner colour.
- Verification: tsc + tests; screenshots over a light image, a dark image and no image (contrast measured on the rendered title area).

#### Step A2.3: Choosing and resizing a banner (Size S) — Blocked by: A2.1
- Acceptance: file picker → client-side resize to 960×320 JPEG (≤200 KB) via a pure `fitBanner(w,h,max)` function; drag-drop on the header; remove restores the gradient; a picker with 8 built-in presets drawn in code (SVG/CSS gradients and patterns, so no bitmap assets to license), each tinted by the project colour.
- Verification: `npm test` for `fitBanner`, `bannerGradient` and `presetBanner(id, color)` (pure, in `modules/sessions/lib/banner.ts`).

### Phase B1: Thread identity and thread-scoped backend — Size L, split into B1a-c; HIGHEST RISK, do first
Verified when: two sessions can run turns at the same time in the app; opening a session no longer re-points a shared primary; the primary chat behaves exactly as before for a single session (existing tests green).

#### Step B1.1: Thread id = session id; registry resolves by it (Size M) — Blocked by: none
- Acceptance: `AgentRegistry::resolve(Some(session_id))` returns a persistent `AppState` for that session, created on first use and loaded from its file; the empty id still means the primary; `thread.rs` (already extracted) gets `open(session_id)`; ids are `session_<ms>` and never reused.
- Verification: `cargo test --no-default-features --lib runtime::thread` + new registry tests (two ids → two states; same id → same state).

#### Step B1.2: Thread-scope abort, steer, queues, model, workspace, plan, long task (Size M, in 2-3 sub-steps by command group) — Blocked by: B1.1
- Acceptance: each command takes `thread_id` and resolves through the registry; `None` stays primary. Follows `docs/session-windows-plan.md` step 1.
- Verification: `cargo test --no-default-features --lib`; manual: start a long turn in thread A, switch to B, send a message in B, stop A.

#### Step B1.3: Frontend placement per session (Size M) — Blocked by: B1.1
- Acceptance: `agentStoreFor(sessionId)` is the store for that session; `daysStore` and `TeamRoom` stop writing foreign `setState`s (architecture review candidate 2); the sidebar can show running/unread per thread.
- Verification: tsc, `npm test`, CDP check of two threads streaming.

### Phase B2: Summary titles — Size S
Verified when: a new thread's title is a ≤6-word summary of what was asked (not the first 30 characters), set once after the first reply; a title the user typed is never overwritten.

#### Step B2.1: Pure title logic + fallback (Size S) — Blocked by: none
- Acceptance: `title.rs`: `fallback_title(text)` (cleans markdown/markers, first sentence, ≤48 chars cut at a word, no mid-word cuts); `title_prompt(first_ask, first_reply)`; `parse_title(model_output)` (strips quotes, trailing period, `Title:` prefix; rejects empty/too long).
- Verification: `cargo test --no-default-features --lib title` (table tests: markers, URLs, code fences, very long first lines, non-English).

#### Step B2.2: Generate alongside the first reply (Size S) — Blocked by: B2.1
- Acceptance: when a thread's first user message is sent, a concurrent `complete_background` call using Vivianne's current chat model (also for coding-agent threads; if none is set up, the fallback) writes the title from the first message, sets the name and emits `session:renamed`; it never delays or blocks the reply; ≤8s, then the fallback stays; `SessionInfo.title_source` (`auto`|`user`) is set to `user` by `cmd_rename_session`, which blocks regeneration. Cost accepted: one extra full-price call per new thread.
- Also: a "Regenerate title" entry in the thread's menu asks for a new summary from the thread so far; it replaces an `auto` title and, because you asked, a `user` one too.
- Verification: unit test with a stub completer; manual: new thread → the title appears about when the first reply starts.

### Phase B3: A worktree per thread — Size M
Verified when: a new thread in a git project has its own branch and folder; the agent's workspace is that folder; two threads editing the same file don't collide; deleting a thread cleans up safely.

#### Step B3.1: `thread_worktree` module (Size M) — Blocked by: B1.1
- Acceptance: `ensure(project_root, session_id, title) -> {path, branch}` using `git worktree add -b <branch> <path> <base>`; folder `<repo>/.viv/worktrees/<thread-id>`, branch `viv/<slug>-<4 chars>` cut from the project's current branch; idempotent (second call returns the same); non-git folder or General → `None`, thread uses the folder as today; reuses `git_in`/`worktree_of`; path added to `.git/info/exclude`, never to the user's `.gitignore`.
- Verification: `cargo test --no-default-features --lib thread_worktree` against a temp repo (create, reuse, dirty base, branch name collision, non-git folder).

#### Step B3.2: Provision on the first turn, as a visible phase (Size M) — Blocked by: B3.1, B1.2
- Acceptance:
  - **Phase:** the first turn of a thread in a git project starts in `preparing`: stages fetch (only if the strategy asks), checkout, setup script, then the agent starts. The thread row and the chat show the stage.
  - **Strategies:** `root` (no worktree; also every non-git project), `existing_worktree { path }` (attach; refused if another thread is bound to it), `worktree { baseRef }` (new, the default).
  - **Setup script:** `.viv/setup.sh` (`.viv/setup.ps1` on Windows) runs once inside a new worktree, output kept, a non-zero exit fails the preparation.
  - **Failure:** one failure code with its reason shown and a Retry that reuses the worktree already bound to the thread.
  - **After it:** `set_workspace([worktree.workspace], full)` is applied, `SessionInfo.worktree/branch/base_branch` are saved, and the Git tab and sidebar follow the thread's root.
- Verification: temp-repo tests for each strategy, the one-thread-per-worktree refusal, a failing setup script, and retry reusing the worktree; live: new thread, edit a file, the main folder is unchanged and the worktree has the change.

#### Step B3.4: The thread's terminal opens in its worktree (Size S) — Blocked by: B3.2
- Acceptance: a terminal opened from a thread starts in that thread's workspace folder and is listed under that thread; its history is kept per thread (5,000 lines or 8 MiB).
- Verification: open a terminal in a worktree thread; `pwd` is the worktree.

#### Step B3.3: Finish, merge and delete (Size S) — Blocked by: B3.2
- Acceptance: delete = remove the worktree if clean; if it has uncommitted or unmerged work, keep the folder and branch and say so; "Merge into <base>" and "Open PR" actions in the Git tab (a button; the agent can also do it with `bash`, approved as usual).
- Verification: temp-repo tests for clean/dirty/unmerged delete.

### Phase B4: Thread ids callable from other threads, and thread messaging — Size L, split B4.1-4
Verified when: thread A can list threads, message thread B by id, and B receives it as a visibly attributed message and acts on it; loops are impossible.

#### Step B4.0: Lineage on the thread row (Size S) — Blocked by: B1.1
- Acceptance: `parent_thread_id`, `relationship` (`fork` | `subagent` | `side`), `root_thread_id`, `created_by` (`user` | `agent` | `system`), `creation_source` on the thread's session file (serde defaults, so old files load); the sidebar lists only threads with no parent; `cmd_thread_family(id)` returns a thread with its forks and children.
- Verification: `cargo test --no-default-features --lib sessions`; a listing test: a thread with two children shows once.

#### Step B4.1: Receipts, outbox and delivery rules (Size M) — Blocked by: B1.2
- Acceptance:
  - **Store:** `~/.viv/threads.db` (rusqlite, WAL), tables `thread_commands(command_id PK, from_thread, to_thread, hop, status, accepted_at, result)` and `thread_outbox(effect_id PK, command_id, to_thread, kind, payload_json, status pending|running|succeeded|failed|cancelled, attempt_count, available_at, lease_owner, lease_expires_at, last_error)`.
  - **Receipts:** a message carries a command id; sending the same id again returns the first result and runs nothing.
  - **Outbox:** a worker claims due rows into a lease, runs the delivery, and on failure retries with `min(30s, 100ms · 2^(attempt-1))` up to 5 attempts, then marks it `failed` with the error. At startup, rows whose lease expired are claimed again, so a message is not lost when the app closes. Deliveries to one target are serialised.
  - **Delivery by target state:** live and busy → its follow-up queue at its next step; live and idle → starts a turn; not loaded → loaded headlessly and run (as bots are).
  - **Transcript:** each message is stored in the target as a `User` message tagged `<from_thread id= title= hop=>`.
  - **Limits:** hop limit 5 (the sender is told when a chain stops); no cap on concurrent threads by default, with a `maxConcurrentThreads` setting (0 = unlimited); a thread can never message itself.
- Verification: `cargo test --no-default-features --lib thread_outbox`: the same command id twice runs once; a crash between claim and finish is recovered by the lease sweep; backoff timings; the 5-attempt ceiling; the three delivery cases with a fake runner; the 5-hop stop; self-message refusal.

#### Step B4.2: Tools `list_threads`, `read_thread`, `message_thread`, `ask_thread`, `interrupt_thread`, `pending_requests` (Size S) — Blocked by: B4.1
- Acceptance: schemas + effects (`Talk`, so no approval to send; what the receiver does is approved on its own side); `ask_thread` returns "sent" and the answer arrives later as a message (same shape as `ask_agent`); `read_thread` returns a bounded, paged view (the history page limits in B7); `interrupt_thread` stops another thread's turn; `pending_requests` lists and reads a thread's waiting permission or question requests and **cannot approve or answer a permission request** (only the user can); unknown or own id is a clear error. Tools declare reads / writes threads / starts threads, and a read-only session is refused the writers.
- Verification: `cargo test --no-default-features --lib tools` (schema/effect drift tests exist), plus capability group entry.

#### Step B4.4: `spawn_thread`: a child thread for a piece of work (Size M) — Blocked by: B4.0, B4.1, B3.2
- Acceptance: a tool `spawn_thread { title, prompt, workspace?: root | worktree, wake?: always | settled_only }` creates a child thread (its own worktree by default) and sends it the prompt with `senderThreadId` set to the caller; the child's final answer comes back to the caller as a message, waking it per `wake` (default `settled_only`: only when the parent is idle); a parent with awaited children does not end its turn early. Provider-native subagents from a coding agent stay read-only.
- Verification: unit tests with a fake runner: wake policy both ways, the answer returns once (receipt), the parent waits for an awaited child.

#### Step B4.3: The message card in the transcript and the sidebar's unread dot (Size S) — Blocked by: B4.1, B1.3
- Acceptance: a received message renders as a quiet card ("From <thread title>") instead of a user bubble; the sidebar row shows an unread dot.
- Verification: screenshot; component test for the tag parser.

### Phase B5: ACP threads in worktrees — Size S
- Acceptance: for a thread on `harness/<id>`, the agent's `cwd` is the thread's worktree; a message from another thread becomes a `session/prompt` on its ACP session.
- Verification: the existing `cmd_harness_debug` run against a worktree cwd; one real Claude Code turn editing a file inside the worktree.
- Blocked by: B3.2, B4.1.

### Phase B5b: The thread tools for coding agents, through an MCP bridge — Size M, split B5b.1-3
Verified when: a real Claude Code thread can call `list_threads` and `message_thread` and the target thread receives the message with the right sender.
Fog (researched at pickup, so no steps are guessed): which MCP transports each ACP agent accepts (every agent must take stdio; `initialize` reports `mcpCapabilities.http`/`sse` for the rest). Working assumption: a stdio server, with Vivianne's own binary started as `app --mcp-threads --port <p> --token <t>`, proxying to a loopback endpoint in the running app.

#### Step B5b.1: Loopback thread API with a per-session token (Size S) — Blocked by: B4.1
- Acceptance: a localhost-only endpoint (random port, bound to 127.0.0.1) that accepts `list_threads` / `message_thread` / `ask_thread` for exactly one thread, identified by an unguessable token minted per ACP session and revoked when it ends; requests go through the same functions and hop counter as the in-app tools.
- Verification: `cargo test --no-default-features --lib` (wrong token refused, revoked token refused, hop limit applies, other interfaces refused).

#### Step B5b.2: The stdio MCP server mode (Size S) — Blocked by: B5b.1
- Acceptance: `--mcp-threads` speaks MCP over stdio, exposes the three tools with the same schemas, and forwards to B5b.1.
- Verification: a test client sends `initialize`, `tools/list`, `tools/call` and reads the replies.

#### Step B5b.3: Hand it to the agent in `session/new` (Size S) — Blocked by: B5b.2, B5
- Acceptance: `mcpServers` in `session/new` lists the bridge with its token for agents that accept it; the debug panel's Test shows whether the agent listed the tools.
- Verification: live with Claude Code: ask it to list threads, then to message another thread.

### Phase B6: Convert old days and conversations into threads — Size M, split B6.1-2
Decision: convert (the user's call). Made safe by a backup, because it rewrites history files.
Verified when: every old day and project conversation is a thread in the list with a real title and keeps its transcript byte for byte; the originals are in a dated backup.

#### Step B6.1: Backup, then the idempotent conversion (Size M) — Blocked by: B2.1, B1.1
- Acceptance: before anything is written, `~/.viv/sessions` is copied to `~/.viv/sessions-backup-<date>/` (abort if that copy fails); then each day file and each project conversation gets `kind: "thread"` metadata and, where its name is empty or a literal copy, a title from `fallback_title` (no model calls in bulk); ids and messages are not changed; the stitched project conversation becomes one thread titled from its project; running the conversion twice changes nothing; a failed file is skipped and reported, never half-written (atomic writes already exist).
- Verification: `cargo test --no-default-features --lib` against a temp copy of real-shaped data (a day, a conversation, a plain session, a corrupt file, a second run); manual: the real set, diffed against the backup (`messages` identical).

#### Step B6.2: Stop creating the old scopes (Size S) — Blocked by: B6.1
- Acceptance: `open_day` and `open_conversation` are no longer called for new chats; the commands remain for opening what exists; General's list and each project's list show the converted threads.
- Verification: tsc, `npm test`; the Home and thread screenshots with the converted data.

### Phase B7: A thread event log with cursors — Size L, split when picked up; AFTER B6 and before the Android client
Verified when: a 20 MB thread autosaves in milliseconds, and a client that disconnected resumes from its last sequence without re-reading the transcript; a client whose cursor is older than the retained log gets a snapshot, then the live tail.
Fog (researched at pickup): moving existing session JSON into the log, keeping JSON as an export, and which events become projections (runs/attempts/nodes) versus staying in the transcript.
- Page policy from the report: a page is at most 10 user turns, 75 items, 500 turn items and 1 MiB encoded; long finished runs collapse into one item with a cursor that loads them on demand; the bounded snapshot keeps pending approvals and the latest todo list outside the window.
- Shape, from the report: append-only events per thread with a monotonic sequence; a projection of the transcript updated in the same transaction; subscribe(`afterSequence`) returns replay or snapshot then live; the boundary of pruned history is published before deleting; ids for events are allocated, not derived.
- Depends on B1, B4.1 (it shares `threads.db`).

### Phase B8: Forks, side chats and handoffs — Size L, AFTER B4 and the ACP layer's P4; split when picked up
Verified when: you can fork a thread at any turn, open a side chat beside a running thread without disturbing it, and move a thread between Vivianne's agent and a coding agent without losing the thread's context.
- Fork: a new thread with the source's transcript up to a chosen turn; the same agent session when it supports forking (`session/fork`), else a token-budgeted summary of the transcript.
- Side chat: shares the parent's worktree, so it starts in approval-required mode; cannot start another side chat; its context is a digest of at most 8,000 characters plus a `read_thread` tool pointing at the parent; it is deleted with the parent.
- Switch: changing a thread's agent or model sends the new one a delta summary since it last saw the thread (or the full summary), delivered as injected history if the agent supports it, else inline in its next prompt, once.
- Fog (researched at pickup): the budget per agent and which agents support `session/fork`.

### Phase B9: Thread lifecycle — Size M, later
- Archive and pin first; then auto-archive after a merged PR or N idle days.
- **Usage-limit recovery**, as T3 does it: a turn that failed with class `usage_limit` and a future reset time arms a recovery on the thread (`{ runId, resetAt, autoResume, snooze }`); at the reset time, if the thread has no pending permission or question, it is sent "Continue where you left off." as a server-made message; overdue arming stops after 24 h; subagents of a settled parent are skipped; an "auto-resume" setting and a banner on the thread with the reset time. Detection comes from P10.
- Thread status shown in the sidebar: idle, preparing, running, waiting (a permission or question is pending).
- Out of scope until asked: snooze, tags, guest sharing.

## Decision log (proposed; becomes final after the gates)

| Decision | Chosen option | Why |
|---|---|---|
| Thread id | the session id (`session_<ms>`) | already unique, already the file name, no second id to keep in sync |
| Title | cheap-model summary after the first reply, fallback = cleaned first sentence, pinned by a user rename | gets a summary without delaying the reply; never loses a user's own name |
| Worktree creation | lazily, on the thread's first turn, only in a git repo | a thread that only asks a question never makes a branch |
| Messaging | `message_thread` (fire and forget) and `ask_thread` (answer arrives later) | same shape as `ask_agent`, which users already know |
| Banner storage | file under `~/.viv/project-banners/`, referenced from `projects.json`; plus 8 built-in presets drawn in code | `projects.json` is rewritten on every project change; 200 KB images do not belong in it; presets cost no assets |
| Worktree place and time | `<repo>/.viv/worktrees/<thread-id>`, branch `viv/<slug>-<4 chars>` from the project's current branch, created on the thread's first turn, git repos only | a question-only thread never makes a branch; hidden through `.git/info/exclude` so the user's `.gitignore` is untouched |
| Idle thread receives a message | wakes up and works, same approvals | what makes threads agents; approvals still guard every action |
| Title model | Vivianne's current chat model, started when the first message is sent | user's call; one extra full-price call per new thread, never blocks the reply |
| Concurrency and chains | unlimited threads (setting `maxConcurrentThreads`, 0 = unlimited), 5 hops | user's call; the hop limit is the only brake on message loops |
| Agent access to thread tools | yes, through an MCP bridge (Phase B5b) | user's call; per-session token, loopback only |
| Old days and conversations | converted into threads, backup first | user's call; rewrites history files so the backup is mandatory |
| Thread messages: exactly once, not lost on quit | command receipts and an outbox in `threads.db` (B4.1) | what T3's orchestrator does for every command; `rusqlite` is already bundled |
| Subagents | `spawn_thread` makes real child threads; `task` stays as is; agent-native subagents stay read-only | T3's split: only app-owned children can receive messages |
| Full event-sourced thread store | deferred to B7 | fixes whole-file rewrites and gives the Android client cursor resume, but is a migration of every session |

## What the T3 orchestration report changes (2026-11-04)

T3's orchestrator turns every command into events, projection rows, a receipt and outbox effects in **one SQLite transaction**, retries side effects from the outbox, and lets clients resume by cursor. Against Vivianne today:

| T3 does | Vivianne today | What we take |
|---|---|---|
| A command id is recorded; a retry returns the first result, never runs twice | None. A resent thread message would run twice | **Now, in B4.1:** command receipts for thread messages |
| Side effects go through an outbox: lease, retry with `min(30s, 100ms · 2^n)`, 5 attempts, expired leases re-claimed after a crash | None. A "wake that thread" is lost if the app closes | **Now, in B4.1:** an outbox for message delivery and wake-ups |
| One lock per thread, parent before child | One mutex per live thread (held for a whole turn) | Keep. A message to a busy thread goes to its follow-up queue |
| Events + projections + receipt + outbox in one transaction | The whole transcript is rewritten as one JSON file after every turn (up to tens of MB) | **Later, B7**, not now |
| Clients resume with `afterSequence`, or get snapshot then live tail when the cursor is pruned or too old; 1-day event retention | The frontend re-reads the whole transcript when a thread opens; events are fire-and-forget | **B7**; the Android app needs it |
| Run, attempt, node model; a steer or retry is a new attempt on the same run | Turns and `steer` with no record of attempts | Vocabulary for B7 only |
| WAL SQLite with a dedicated checkpoint worker | `rusqlite` (bundled) is already used by the memory store | Reuse it for B4.1 and B7 |

So B4.1 changes (below), and a new Phase B7 is added. Nothing else in the plan moves.

## What the T3 threads report changes (2026-11-04)

| T3's thread model | Vivianne today | What we take |
|---|---|---|
| A thread record carries `lineage` (`parentThreadId`, `relationshipToParent` fork or subagent, `rootThreadId`, `sideChat`); subagents and side chats are listed inside their parent, never in the thread list | Flat sessions; the `task` tool's subagents have no thread of their own | Same fields on our thread row (**B4.0**). The sidebar lists top-level threads only; a family query returns a thread with its forks and children |
| Worktree provisioning is a *staged run phase*: status `preparing`, stages fetch, checkout, setup script, agent; progress events; one failure code; a retry reuses the worktree already bound | Planned as a synchronous `ensure` | **B3.2 changes:** the first turn waits in `preparing` with visible progress; an optional setup script (`.viv/setup.sh` or `.viv/setup.ps1`) runs in the new worktree; a failed preparation shows its reason with Retry |
| Workspace strategy per thread: `root`, `existing_worktree`, or new `worktree` from a base ref | Only "new worktree from the current branch" | Same three, so a thread can attach to a worktree you already have (**B3.2**). A non-git project is `root` |
| One thread per worktree path; `getWorktreeThreadIds` | Not stated | Rule added to B3: a worktree is bound to exactly one thread; the Git tab can list which |
| App-owned subagents are real child threads: the parent's message carries `senderThreadId`, results come back as a message, a per-task wake policy (`always` or `settled_only`), and a parent waits for awaited tasks. Provider-native subagents are read-only and can't receive messages | `task` subagents run in the background-tasks panel; coding-agent subagents arrive as updates | **B4.4 `spawn_thread`:** start a child thread with its own worktree and a wake policy. The existing `task` tool stays as it is. Claude Code's own subagents stay read-only (P5) |
| Side chat: a fork that shares the parent's worktree, so it starts supervised (approval required); it can't start another side chat; context is a bounded digest (8,000 characters) plus a tool to read the parent | None | **B8** |
| Context transfer: `native_fork` when the same agent has a strong native session, else `portable_context` (a token-budgeted summary); provider switch uses a delta or full summary; delivered by injecting history, or inline in the next prompt | Switching a thread's model or agent mid-conversation just continues | **B8**, and it matters for per-thread agent choice: moving a thread from Vivianne's agent to Claude Code needs a handoff |
| Thread lifecycle: archive, settle (auto-settle after a PR merges or N days), snooze, pin, tags, limit-recovery (usage limit → wake at reset) | Delete only | Backlog (**B9**): archive, pin, and usage-limit auto-resume first |
| History pages: 10 user turns, 75 items, 500 turn items, 1 MiB per page; long finished runs collapse into one item with a cursor | The whole transcript is loaded | Numbers go into B7's acceptance |
| Titles can be regenerated on request; a thread has `createdBy` (user, agent, system) and `creationSource` | A title is made once | B2 gains a "Regenerate title" menu entry; thread rows get `createdBy`/`creationSource` (needed to tell a message another thread sent from one you typed) |
| Idle compaction: send `/compact` shortly before the provider's prompt cache expires (5 min, or 1 h on subscriptions), at most once per idle spell | Compaction happens when the window is full | Input for the compaction work, not this plan |
| Runtime modes: approval-required, auto-accept-edits, auto, full-access; side chats default to approval-required | Ask, Auto, bypass | Input for P5's mode mapping |

## Found while building

- `save_current_session_impl` recomputes the name from the first 30 characters on every autosave, so a rename from `cmd_rename_session` is lost after the next turn. B2 fixes it (a stored `title_source` decides).

## Not yet specified

- Phase B5b: the MCP transport per agent (above).
- Phase B1.2: exactly which commands still lack a thread id after the audit (the session-windows table lists the known ones; the grep for the rest is the first task of B1.2).
- Phase B4.1: how a headless (not-open) thread asks for approvals while the user is looking at another thread (fog: researched at pickup; likely a badge on the row plus the existing approval card when opened).
- Phase B3.3: whether "Merge into base" is a button or only an agent action: default is a button in the Git tab, and the agent can do it through `bash`; nothing is ever merged without a click or an approved tool call.

## Always / Never

- Always: sessions saved before this change keep working untouched; the 24/7 agents and their flows are not modified; every phase ends green on `cargo test --no-default-features --lib` (the two known parallel-only `rerank` tests are run with `--test-threads=1`), `npx tsc -p tsconfig.app.json --noEmit` and `npm test`.
- Never: auto-merge or push a thread's branch; delete a worktree that has uncommitted or unmerged work; write to the user's `.gitignore`; touch an old session's messages (B6 may add metadata and a title, only after the backup exists); let a thread message itself; put an API key or token in a thread message; let the bridge listen on anything but 127.0.0.1.
- Out of scope: the 24/7 agents and their stores; the teams UI (already removed); multiplayer projects; the pop-out window shell (`docs/session-windows-plan.md` step 3); compaction and the watcher model; image generation.

## Progress

| Phase | Status |
|---|---|
| A1 Sidebar and icon | done: banner header, icon-only new chat (the existing Mod+Shift+N shortcut stays), focus ring, empty state; verified in the app |
| A2 Banner | done: field and commands (4 Rust tests), header and menu, picker with 8 presets, upload resized to 960x320 JPEG and stored as a file; verified in the app end to end |
| B1 Thread identity | B1.1 and B1.2 done (registry tests; two threads run at once and stop independently, checked in the app). B1.3 partly: agent stores are per session; `TeamRoom` still writes foreign `setState`s; older single-conversation projects stay on the primary |
| B2 Titles | built: `sessions/title.rs` (8 tests), `title_source` on sessions, rename now sticks, title asked for alongside the first message; live check below |
| B3 Worktrees | B3.1 done: `sessions/thread_worktree.rs` (6 tests on real temp repos: create, find again, collisions, project inside a repo, non-repo/empty repo, remove keeps dirty or unmerged work). B3.2 and B3.3 wait on B1.2, as planned |
| B4 Messaging | not started |
| B5 ACP threads | not started |
| B5b MCP bridge for agents | not started |
| B6 Convert old sessions | not started |
| B7 Thread event log with cursors | not started (planned after B6) |
| B8 Forks, side chats, handoffs | not started (planned after B4) |
| B9 Thread lifecycle | not started (later) |
