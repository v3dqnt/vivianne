# Session log — Astryx design system migration + v1.5 wrap-up

Date: 2026-07-20/21

## Context

This picks up after v1.5 shipped (installer built, changelog written, GitHub
release published). The user installed `@astryxdesign/core` (Meta's Astryx
design system — StyleX-based, ships pre-built CSS, no build-plugin
integration needed) and asked to port Vivianne's UI onto it: sidebar, chat
composer/message/layout, message metadata, breadcrumbs, badges, and the
settings layout components — under `/ponytail` (lazy-but-correct, question
scope rather than silently drop it).

## Done

### 1. Astryx setup
- `globals.css`: added the documented Tailwind-layer bridge (`@layer reset,
  theme, base, astryx-base, astryx-theme, components, utilities;` +
  `reset.css` / `astryx.css` / `tailwind-theme.css` imports).
- `src/components/AstryxThemeProvider.tsx` (new): bridges `next-themes`'
  resolved light/dark mode into Astryx's `<Theme>` provider, nested inside
  the existing `ThemeProvider` in `layout.tsx`.

### 2. Custom theme matching Vivianne's brand
- `src/lib/astryxTheme.ts` (new): `defineTheme({ extends: neutralTheme,
  tokens: {...} })` mapping Vivianne's exact ink/canvas/hairline/surface-soft
  hex values (light **and** dark, from `globals.css`'s `:root`/`.dark`
  blocks) onto Astryx's token surface (`--color-text-primary`,
  `--color-background-surface`, `--color-border`, `--color-accent`, etc.).
  Buttons/badges forced to fully pill-shaped via `components: { button: {...},
  badge: {...} } }`, matching VIV-DESIGN.md's "pill-shaped, near-black
  primary" button spec — Astryx's default blue accent was replaced with
  Vivianne's ink color since VIV-DESIGN.md keeps color out of UI chrome
  almost entirely.
- Removed the static `@astryxdesign/theme-neutral/theme.css` import from
  `globals.css` (superseded by the runtime-injected custom theme; keeping
  both would let the neutral theme's hardcoded values fight the custom
  ones).
- Icon convention: Astryx's `icon` props expect a plain
  `ComponentType<SVGProps<SVGSVGElement>>` (e.g. Heroicons/Lucide), **not**
  Hugeicons' `<HugeiconsIcon icon={dataObject}/>` wrapper convention. Used
  `lucide-react` (already an existing dependency) for every icon passed
  into an Astryx component; kept `@hugeicons/react` everywhere else
  unchanged.

### 3. Sidebar → `SideNav`
- `src/components/Sidebar.tsx` rewritten on `SideNav` / `SideNavHeading` /
  `SideNavSection` / `SideNavItem` / `MoreMenu` / `Badge` / `TextInput`
  (search) / `Button` / `IconButton`.
- Real bug hit and fixed: `SideNavItem` renders as a native `<button>` when
  it only has `onClick` (no `href`), and `MoreMenu`'s trigger is also a
  `<button>` — nesting session-select + per-row `MoreMenu` triggered React's
  "`<button>` cannot be a descendant of `<button>`" hydration error. Fixed
  by giving the session `SideNavItem` an `href="#"` (+ `preventDefault()` in
  `onClick`), which makes it render as `<a>` instead — matches the pattern
  Astryx's own `shell-side-nav` reference template uses for exactly this
  case.
- Added a live "Agents" nav entry (was previously a fully dead code path —
  `AppSidebar`'s `onNavigate` prop type already accepted `"agents"` but
  nothing ever called it and `page.tsx` never rendered anything for it).
  Wired to the `AgentsView` modal (tabs: **Subagents** — tracks the
  autonomous `task`-tool spawns that were already fully built server-side
  this session but had zero UI — and **Instances** — the pre-existing,
  also-orphaned `AgentsPanel.tsx` manual-orchestrator UI). Shows a live
  badge with the active-subagent count.

### 4. Settings — full redesign (not just a token swap)
First pass wrapped every setting row in its own boxed `Section` card, which
the user flagged as looking bad ("flat grey boxes"). Root cause: that's
explicitly against Astryx's own guidance (`AGENTS.md`: "Dense data = rows —
never Card-wrapped list items"). Rebuilt properly against Astryx's real
`settings-dialog` template:
- `SettingsPage.tsx` now renders as an actual Astryx `Dialog`
  (`purpose="form"`) instead of a hand-rolled fixed-overlay div.
- Left nav: `Layout` → `LayoutPanel` (`role="navigation"`) → `List` /
  `ListItem`, replacing the custom pill-button nav.
- Per-section header: real `DialogHeader` with the active section name as
  the title.
- `SettingRow` (used ~28 times across every tab) and `SectionHeader` (~15
  times) are shared helper functions — fixed their *internals* once
  (`HStack` + `VStack` + `Text` + `Divider`, no more boxed `Section`), which
  automatically fixed every call site with a two-function edit instead of
  touching 40+ call sites individually.
- `SettingsPage` prop signature changed from `{ onToggleSidebar? }` to
  `{ isOpen, onClose }` (Dialog-controlled); `page.tsx`'s custom
  `AnimatePresence`/backdrop wrapper was deleted since `Dialog` owns its own
  backdrop/animation/close now.

### 5. Chat shell → `ChatLayout` / `ChatComposer` / `ChatMessage`
The largest, highest-risk piece — `ChatInterface.tsx` has ~15 features
threaded through it (tool cards, reasoning blocks, image lightbox, source
citations, checkpoints/revert, retry/fallback banners, queue panel, todo
panel, `@` mentions for skills+files, attachments, model/thinking/persona
selectors, steer/abort during generation). Scoped deliberately: swap the
**shell/chrome** to Astryx, keep all the bespoke message-content rendering
(`ToolCard`, `Reasoning`, `MessageResponse`/Streamdown) as children inside
Astryx's slots — not a from-scratch rewrite of tool-call rendering.

- `ChatLayout` replaces the hand-rolled scroll container + manual
  stick-to-bottom/scroll-button tracking (`ChatLayout` has built-in
  auto-scroll + scroll-to-bottom).
- `ChatComposer` + `ChatComposerInput` replace the old composer `<div>` +
  `<Textarea>`. Existing custom controls (model selector trigger, thinking
  level dropdown, persona dropdown, auto/permission mode toggle, workspace
  panel trigger, attach-image button, context-usage toggle) moved into
  `headerActions`; the steer button (only shown mid-generation) moved into
  `sendActions`; `onStop`/`isStopShown` wired to the existing `abortAgent`.
  Attachment thumbnails moved into `ChatComposerDrawer`.
- **`@` mentions were re-implemented on `ChatComposerInput`'s native
  `triggers` API** instead of keeping the old hand-rolled
  `UnifiedMentionMenu` + manual arrow-key/tab interception. This wasn't
  optional — `ChatComposerInput` owns ArrowUp/Down (message-history recall)
  and Enter (submit) internally with no exposed raw `onKeyDown`, so the old
  approach would have fought the component's own key handling. Implemented
  a custom `SearchSource` (`search`/`bootstrap`) mixing skills (already
  fetched via `listSkills`) + workspace files (lazily fetched via
  `listDirTree` on first `@`, cached) into one `@` trigger; `onSelect`
  returns the literal insertion text, replacing what
  `applySkillMention`/`applyFileMention` used to do.
- `ChatMessage` / `ChatMessageBubble` / `ChatMessageMetadata` replace the
  old `ai-elements/message.tsx` `Message`/`MessageContent` wrapper — user
  messages get a filled bubble, assistant messages a `ghost`-variant bubble
  (no visible boundary, matching the old plain-prose look) with an
  `Avatar` using Vivianne's actual logo file. Tool-result rows
  (`ActivityRow`'s `isTool` branch) were deliberately **left untouched** —
  Astryx's `ChatToolCalls` only fits *inside* a `ChatMessage`, and
  Vivianne's tool cards render as standalone top-level entries grouped by
  `ActivityCluster`; forcing that into Astryx's shape would've meant
  reworking the grouping logic, out of scope for "swap the message
  bubbles."
- Real mistake caught mid-edit and fixed before it shipped: briefly swapped
  `MessageResponse` (the actual Streamdown markdown renderer — code blocks,
  tables, math, mermaid) for `TextGenerateEffect` (an unrelated word-reveal
  animation component) while restructuring the assistant branch. Caught by
  re-reading my own diff before moving on; reverted to `MessageResponse`.
- Hit a **structural JSX break mid-edit** (several sequential `Edit` calls
  left the composer's JSX in an inconsistent state — dangling old
  Textarea-based "Row 2" markup, an unclosed `headerActions` fragment, and
  the message-list `children` accidentally dropped during a consolidation
  edit) that surfaced as a real Next.js build error. Fixed by reading the
  *entire* file fresh rather than continuing to patch blindly, then
  rewriting the whole `return` statement in one clean pass.
- Bug reported by the user after it compiled clean: the composer looked
  **off-center** relative to the message list above it. Cause: the composer
  wrapper still used the old `max-w-2xl` (672px) while the message list
  used `max-w-[1280px]` — different max-widths under independent `mx-auto`
  don't visually "line up" as a column even though both are individually
  centered. Fixed by matching both to the same `max-w-[1280px] mx-auto
  px-4 md:px-6`.
- Dead code cleanup after the swap: removed the now-unused manual
  scroll-tracking state/refs, the old mention-menu state
  (`mentionIndex`/`setMentionIndex`), and three now-dead icon imports
  (`ProviderIcon`, `SentIcon`, `BotIcon`).

### 6. Badge → Astryx `Badge`
Swapped every `@/components/ui/badge` usage (Settings' MCP-server transport
badge, "Experimental" engine badge, Docker status pills, skill
missing-dependency/manual-only badges; `ai-elements/tool.tsx`'s
`getStatusBadge`) to `@astryxdesign/core/Badge`, mapping each ad-hoc
`bg-emerald-50`/`bg-amber-50`/etc. `className` combo to the matching
semantic `variant` (`success`/`warning`/`info`/`neutral`/`error`).

### 7. Workspace panel + Diff panel → Astryx
Both rebuilt on `Layout` / `LayoutPanel` / `LayoutContent` / `Toolbar` /
`IconButton` / `Badge` / `EmptyState`, replacing the hand-rolled
header-div + raw-button chrome — same pattern as Sidebar/Settings.
`FileTree` (Workspace panel) and `DiffView` (Diff panel) kept as-is inside
the new shell; both are non-trivial custom logic (tree expand/collapse,
syntax-highlighted diff rendering) not worth reimplementing against an
Astryx primitive. Diff panel's file list moved from raw buttons to
`List`/`ListItem` (matches "dense data = rows" guidance) inside a
`LayoutPanel`.

### 8. Tool-call JSON leaking into assistant text (real bug fix)
User reported assistant messages sometimes showing raw
`{"type":"toolCall","id":"...","name":"task","arguments":{...}}` JSON as
visible text (seen with `deepseek-ai/deepseek-v4-flash`, routed through
NVIDIA NIM's `OpenAiChat` wire format) instead of a real tool call. Root
cause is a model/provider limitation (the model narrates a tool call in
prose instead of only emitting a structured `tool_calls` delta) rather than
a Vivianne parsing gap — no text-based tool-calling fallback protocol
exists in the codebase that this could be leaking from. Added a defensive
sanitizer regardless of root cause, since malformed model output should
never reach the UI raw:
- `ai/utils.rs::strip_leaked_tool_call_json` — finds `"type":"toolCall"`
  (or the spaced-colon variant), walks back to the enclosing `{`, forward
  tracking brace depth (ignoring braces inside strings) to the matching
  `}`, and removes that whole span. 5 unit tests (leaked-after-prose,
  nested-objects/braces-in-strings, ordinary text untouched, unrelated JSON
  untouched, spaced-colon variant).
- Wired into `finalize_output` in both `openai_completions.rs` and
  `openai_responses.rs`.

## Verification discipline maintained throughout
Every backend change checked with `cargo check --message-format=short` (and
periodically `cargo test --lib`, 112+ passing) and every frontend change
checked with `npx tsc --noEmit -p tsconfig.json`, after essentially every
edit — same discipline as the rest of this session. Visual verification for
Astryx changes was harder than usual: the Browser pane's `computer`
screenshot action repeatedly timed out against this app (tool-level flake,
not app-level — console + accessibility-tree reads showed no errors each
time), so verification leaned on `read_page`/`read_console_messages` plus
the user's own screenshots from the real Tauri window.

## In progress / not yet finished

### Terminal — real PTY backend + tabs + shell picker
`TerminalPanel.tsx` was, until now, a fully static mock (`<div>$ Terminal
initialized.</div>` + a permanently-`disabled` input, literally captioned
"Note: Full shell execution requires backend integration"). No PTY crate,
no terminal emulator library — this is a from-scratch feature, not a
redesign. Building it on the standard stack: `portable-pty` (Rust,
cross-platform PTY spawning) + `@xterm/xterm` + `@xterm/addon-fit`
(frontend terminal emulator). Both dependencies installed.

**Backend — done and wired into `lib.rs`, not yet compiled/tested:**
- `src-tauri/src/terminal.rs` (new): `TerminalRegistry` (process-wide,
  `HashMap<tab_id, PtySession>` behind a `StdMutex`, same shape as
  `SubagentRegistry`). `available_shells()` detects what's actually
  installed — PowerShell + cmd always on Windows, Git Bash only if
  `C:\Program Files\Git\bin\bash.exe` (or the x86 path) exists; bash/zsh by
  name on non-Windows. `spawn()` opens a pty via `native_pty_system()`,
  builds the right `CommandBuilder` per shell id (`"powershell"`, `"cmd"`,
  `"gitbash:<path>"`, or a bare shell name), spawns the child, and starts
  two background threads per session: one pumping raw output bytes into
  `terminal-output` events (lossy UTF-8, since xterm.js parses ANSI itself
  and terminal output is overwhelmingly valid UTF-8/ASCII anyway), one
  polling `try_wait()` to detect exit and emit `terminal-exit` — both share
  the `Child` via `Arc<StdMutex<Box<dyn Child + Send + Sync>>>` so
  `close()` can also kill it on demand. `write()`/`resize()` round out the
  session API.
- `AppState`: `app.manage(Arc::new(TerminalRegistry::new()))` added in
  `lib.rs`'s setup.
- Commands added to `agent/commands.rs` and registered in `lib.rs`'s
  `invoke_handler`: `cmd_terminal_list_shells`, `cmd_terminal_spawn(tab_id,
  shell_id, cwd)`, `cmd_terminal_write(tab_id, data)`,
  `cmd_terminal_resize(tab_id, rows, cols)`, `cmd_terminal_close(tab_id)`.

**Not started yet:**
1. `cargo check` / `cargo test --lib` on the new `terminal.rs` +
   `agent/commands.rs` additions — written but never compiled. Likely
   trouble spots to check first: `portable_pty::ExitStatus::exit_code()`'s
   actual return type (assumed `u32`, converted via `.try_into().ok()` to
   `Option<i32>` — needs verifying against the real API, not just
   documentation-shaped guessing), and whether `MasterPty`/`Child` trait
   objects satisfy `Send`/`Send + Sync` the way the struct fields assume
   under `portable-pty 0.9.0` specifically (version resolved by `cargo add`,
   not pinned in advance).
2. Frontend: `TerminalPanel.tsx` needs a full rewrite — tab bar (add/close/
   switch tabs), each tab owning its own `xterm.js` `Terminal` instance +
   `FitAddon` bound to its own `tab_id`, a shell-picker dropdown (populated
   from `cmd_terminal_list_shells`) shown when opening a new tab, wiring
   `terminal-output`/`terminal-exit` event listeners (scoped by `tab_id`,
   same pattern as `AgentProvider`'s existing event-listener conventions),
   `onData`/`onResize` from xterm.js calling `cmd_terminal_write`/
   `cmd_terminal_resize`, and closing a tab calling `cmd_terminal_close`.
   Should also get the same `Layout`/`Toolbar`/`TabList` Astryx treatment
   as the other redesigned panels, per the "style everything per
   VIV-DESIGN.md" ask — `TabList` (already in Astryx's component set) is
   the natural fit for the tab bar itself.
3. xterm.js ships its own CSS (`@xterm/xterm/css/xterm.css`) that needs
   importing once (not yet done) or the terminal will render with no
   styling at all.
4. No cleanup-on-app-exit path yet for orphaned shell child processes if
   the user closes Vivianne without closing terminal tabs first — worth a
   `close()` sweep over every live session on app shutdown once the rest of
   the feature is working.

## Session-management notes for next time
- The Browser pane's `computer` screenshot action was unreliable this
  stretch (repeated 30s timeouts) even when the app was rendering
  correctly per `read_page`/console checks — don't treat a screenshot
  timeout alone as evidence of a real app bug; cross-check with
  `read_page`/`read_console_messages` first.
- `astryx build "<idea>"`, `astryx component <Name>`, `astryx docs <topic>`,
  and `astryx template <name>` (CLI, via `npx astryx ...`) were far faster
  and more reliable than reading Astryx's source directly — worth reaching
  for first on any remaining Astryx work (terminal's `TabList`, future
  panels) rather than guessing prop shapes.
