# Panel rebuild: one right container, three panels

Date: 2026-10-28
Status: approved, awaiting implementation plan

## Problem

The workspace docks every kind of panel — Files, Preview, Terminal, Agents, Plan,
Timeline, Memory, Git review, Multiplayer, Quick access, mod panels — into three
dockview edge groups (left, right, bottom). The bottom container only ever held
the terminal and Quick access, and the rest crowded the right container's tab
strip. The result is a panel system where the three things a person actually
uses while working (files, a browser, a shell) are buried among utilities, and
the terminal lives in a container of its own.

This replaces the whole arrangement with one container and three panels, each
built for its job.

## Decisions

| Question | Decision |
| --- | --- |
| Where do the other panels go? | Retired. Their triggers are removed too. |
| How deep is "retired"? | Panels, registry entries and UI triggers are deleted; underlying stores and data (plan, timeline, multiplayer pairing, mods, bots) are kept. |
| How do the three sit? | Closable tabs in one container, reopenable from the title-bar Panels menu. |
| How real are browser tabs? | One child webview per tab. Inactive tabs are parked off-screen, never destroyed. |
| Where do element comments go? | Into the chat composer as a draft, with element context attached. |
| Is the left container kept? | No. One side container only; the "Centre" chat layout is removed. |
| Where do PDFs and office files open? | As file tabs in the Browser panel, alongside web tabs. |

## 1. Panel set

`PanelKind` becomes exactly:

```ts
type PanelKind = 'chat' | 'files' | 'browser' | 'terminal'
```

`PANELS` keeps four entries. `ADDABLE`, `stack`, `custom` and the `document`
redirect all go away.

### Deleted

Components and their registrations:

- `PreviewPanel`, `LivePreviewPanel`, `BrowserPanel` (camofox feed), `LiveBrowserPanel`
  (replaced by the new browser panel). The dev-server auto-preview goes with
  `LivePreviewPanel`; the Browser panel covers the same ground by navigating to
  the URL, and the agent's `web_preview` event opens it there.
- `Stack.tsx`, `QuickAccess.tsx`
- `IdeView` and `ide` panel
- `ReviewPanel` (git), `DocumentsMode`, `BackgroundTasksPanel`, `MemoryPanel`
- `AgentsPanel`, `PlanPanel`, `TimelinePanel`, `MultiplayerWorkspacePanel`, `ModPanel`
- `FileCard` stays — chat tool cards still show files with it — but its "Open
  in Preview" action becomes "Open in Browser".

Triggers removed:

- `PlanPill` / `ModePill` → `openRight('plan')`
- `ToolCallCard` → plan link
- `Sidebar`, `NewProjectDialog`, `TeamRoom`, `GeneralAgents` → `openPanel('agents')`
- `PairChip` → `openRight('multiplayer')`
- Settings shortcut `panel.preview`

Kept, with no panel surface: `planStore`, timeline data, multiplayer pairing
store, `modsStore` (mods still load; they have nowhere to render a panel),
`botsStore`, `useGitStatus`.

### Retired ids

`RETIRED` grows to cover every removed kind so an old saved layout drops them:
`ide`, `preview`, `review`, `documents`, `document`, `tasks`, `memory`,
`agents`, `plan`, `timeline`, `multiplayer`, `quick`, `stack`, `custom`.

## 2. Container and layout

- `Zone` stays `'left' | 'right'`, but only one side is ever used: the one
  opposite the chat (`sideZone()`), so there is still a single container.
  Moving the chat to the other side moves the container's panels across and
  hides the edge it left.
- `ChatSide` is `'left' | 'right'`. The `'centre'` value is dropped from the type,
  the persisted value and the settings UI; a saved `'centre'` loads as `'left'`.
- The container is a dockview edge group (`zone-left` or `zone-right`), tabs on
  top, minimum width 320, opening at ~34% of the window capped at 480.
- Fit logic keeps the chat's floor (`CHAT_MIN`) and the container's minimum; the
  auto-hide/reveal behaviour stays, with the bottom branch deleted.
- Header actions: expand (two-headed arrow, `openFullScreen`) and close container
  (`closeContainer`). The `+` menu, stack button and mod-panel menu entries are
  removed.
- An empty container shows `ZoneEmptyState` offering the three panels.
- `ZONE_LABELS` becomes a single `Side panel` label for whichever edge is in use.

### Presets

```ts
type Preset = 'default' | 'focus' | 'coding' | 'research'
```

| Preset | Container | Active tab |
| --- | --- | --- |
| default | visible | Files |
| focus | hidden | — |
| coding | visible | Terminal |
| research | visible | Browser |

`full` is removed with Quick access. `applyPreset` seeds all three tabs and
activates the preset's tab.

### Stacked mode removed

The `stack` panel and everything that existed to serve it go: `Stack.tsx`,
`StackedPanels`, `StackItem`, `itemKey`, `stackPanel`, `saveStack`, `stackZone`,
`unstackZone`, `useWorkspace.stacked`, and the `items`/`sizes` panel params.
`openPanel` loses its stack branch; `closePanel` loses its stack scan.

### Persistence

- Layout key: `viv.workspace.v3`.
- On load: restore, drop retired ids, ensure the container exists, then seed any
  of Files/Browser/Terminal that are missing (all three by default).
- If the container has no tabs, open Files.

## 3. Files panel

Component: `FilesPanel` in a new `modules/panels/files/`, replacing
`WorkspaceTree`. `TerminalPanel` moves to `modules/panels/terminal/` so the
three panel bodies sit together; only the lazy import in `panels.tsx` changes.

### Roots

`useWorkspaceStore.folders` — the active workspace's folders, primary first.
More than one root gets a sticky uppercase section header per root, as today.
No primary/secondary switch: every folder in the active workspace is listed.

### Tree

- `cmd_list_dir_tree` per root, depth 8, 5000 entries (unchanged clamps).
- One unreadable root shows an inline error and does not blank the panel.
- Filter matches a file's name **or** its path relative to its root, and
  auto-expands matching branches.
- Indent guides per depth (reuse the current gradient technique).
- File tint by category (source / config / docs / assets), unchanged.

### Change badges

Per root, `cmd_git_diff_stat({ path: root, staged: false })` plus the staged
pass, merged by absolute path:

```ts
interface FileChange { additions: number; deletions: number; status: string }
```

- Rows with a change show `+N −N` right-aligned, `+` in success, `−` in error.
- Untracked files (`status === '??'`) show `+N` only.
- Refreshed on mount, on workspace change, after every agent turn
  (`onTurnEnd`), and on the existing 5s git poll.
- Repos without git (or without changes) simply have no badges.

### Expanded file

Clicking a file row toggles its inline expansion (one open at a time).

- **Changed file** → diff view by default: `cmd_git_diff_file({ path: root, file, staged })`,
  rendered as unified hunks with old/new line numbers and tinted `+`/`−` lines.
  Header carries a `Diff | File` toggle; `File` shows the file's contents.
- **Unchanged file** → contents via the existing `FileContent`.
- **Visual / document file** (image, audio, video, pdf, docx, xlsx, pptx) →
  contents plus an "Open in Browser" button.
- Expanded header: name, relative directory, copy path, and — for visual or
  document files — "Open in Browser". No full-screen dialog here; the
  container's expand action already does that.

### `revealFile(path)`

New function in the files store, called by chat file links:

1. Open/focus the Files panel; reveal the container if hidden.
2. Expand every ancestor folder of the path, in whichever root contains it.
3. Expand that file (diff or content as above).
4. Scroll the row into view and flash a highlight for ~1.2s.

`FileLink` calls `revealFile(full)` instead of `openDocument(full)`. Ctrl+click
still reveals in the OS file manager.

### Files panel header

Filter field with clear button, refresh button, and a muted `N changed` count.

## 4. Browser panel

Component: `BrowserPanel` (`modules/panels/browser/BrowserPanel.tsx`) plus
`browserStore` and a Rust-side label-aware webview layer.

### Tab model

```ts
type BrowserTab =
  | { id: string; kind: 'web'; url: string; title: string; webviewLabel: string }
  | { id: string; kind: 'file'; path: string; title: string }
```

State: `tabs`, `activeId`, `history` per tab is the webview's own. Persisted
(tabs + active id + urls) in `viv.browser.v1`. On launch, web tabs are
recreated at their stored URLs.

- `+` opens a new web tab at the home page.
- Closing the active tab activates its right neighbour, else its left.
- Closing the last tab leaves an empty state with a "New tab" button.
- Tab title comes from `document.title` (polled with the URL), falling back to
  the URL host; file tabs use the file name.

### Web tab

- A child webview per tab, labelled `live_browser_<id>`.
- Only the active tab's webview is positioned into the panel slot; the rest are
  parked off-screen (`hide`) and never destroyed.
- Leaving the panel (tab switch in the container, container hidden, dialog
  opened) parks the active webview; returning re-places it.
- Toolbar: back, forward, reload, URL field (Enter navigates, Esc restores),
  open externally, element picker, new tab.
- URL polling (~1s) keeps the address bar honest as links are clicked.

### Rust changes

`browser/live_browser.rs`:

```rust
cmd_live_browser_show(label: String, url: String, rect: Rect)
cmd_live_browser_hide(label: String)
cmd_live_browser_close(label: String)
cmd_live_browser_url(label: String) -> Option<String>
cmd_live_browser_eval(label: String, js: String) -> Value
```

- `find(app, label)` replaces the single `LABEL` constant.
- `ensure_at` (agent path) keeps using a fixed default label.
- The agent's `live_browser_agent` module takes the default label.
- Each webview is created with the window's browser args, as today.
- Race handling for "already exists" stays.

### File tab

Rendered in React, no webview:

| Kind | Viewer |
| --- | --- |
| pdf | `PdfView` |
| docx | `DocxView` |
| xlsx / xls / xlsm / ods / csv | `SheetView` |
| pptx | `SlidesView` |
| image (png/jpg/gif/svg/webp/ico) | `cmd_read_file_preview` → data URL |
| audio (mp3/wav/ogg/m4a/flac) | `<audio controls>` over a data URL |
| video (mp4/webm/mov) | `<video controls>` over a data URL |

CSP change: `media-src 'self' data: blob:` stays sufficient for data URLs; no
asset protocol is enabled or required.

Media is read whole, so audio and video over 50 MB are not inlined: the tab
shows the file's metadata and an "Open externally" button instead. Images and
documents keep their existing size handling.

`panelStore.openDocument(path)` opens a file tab here. `openUrl(url)` opens a
web tab. `previewMode`, `openDocs`, `activeDoc`, `closeDocument` and the
`viv.previewMode` / `viv.previewDocs` keys are removed.

### Element picker

Toggle in the toolbar. One picker per web tab; pick mode ends on navigation,
tab switch, Esc, or a completed pick.

1. **Inject.** `cmd_live_browser_eval` adds a style block, a hover overlay and
   capture-phase `mouseover`/`click`/`keydown` listeners. Cursor becomes
   crosshair; hovering draws an outline plus a small label with `tag.class`.
2. **Record.** A click is swallowed and writes
   `window.__vivPick = { selector, tag, text, rect, url, title, styles }`
   where `selector` is a unique CSS path (id → data-testid → nth-of-type chain)
   and `styles` is a small computed-style summary (font, size, weight, colour,
   background, spacing).
3. **Collect.** The parent polls `cmd_live_browser_eval('window.__vivPick')`
   every 200ms **only while picking**, then clears it.
4. **Compose.** The overlay stays; a React composer opens in the panel footer
   with an element chip (`button.cta — "Get started"`), a textarea, Send and
   Cancel. Enter sends, Shift+Enter newlines, Esc cancels.
5. **Send.** Appends to the chat draft through a `viv:comment` window event,
   handled in `Composer` next to the existing `viv:attach-paths` handler:

   ```
   Comment on button.cta "Get started" — http://localhost:5173/#hero
   <the comment>
   Screenshot: <temp path>   (omitted when capture failed)
   ```

   The composer appends rather than replaces, focuses the box, and the picker
   exits.
6. **Screenshot (best effort).** WebView2 `CapturePreview` on the tab's
   controller, cropped to `rect` × device scale; fallback to xcap window
   capture, fallback to no image. Written to
   `%TEMP%/viv-comments/<timestamp>.png` and referenced by path so the agent
   can read it.

Limits, documented in the panel's empty state: cross-origin iframes and pages
that refuse script execution cannot be picked.

## 5. Terminal panel

`TerminalPanel` unchanged: one xterm session per panel, shell spawned on mount,
cwd = active workspace root, PTY killed on unmount. Registered as `terminal`,
closable, reopenable from the Panels menu. `openBottom()` opens/focuses it.

## 6. Agent integration

The `set_panel` tool names panels by a fixed list. It follows the three-panel
set:

- `SETTABLE_PANELS` (`agent/tools/exec.rs`) becomes
  `["files", "browser", "terminal"]`; the `panel` enum in `schemas.rs` matches.
- `preview_dev`, `preview_browser` and `documents` are dropped from the schema.
  A model that still sends one gets the unknown-panel error, which lists the
  valid names.
- `mod:<key>/<panel>` is no longer a valid target; the validation branch and
  its error text go.
- `agentStore`'s `panel_focus_requested` handler collapses to three cases;
  `RIGHT_PANEL_KINDS`, `openCustomPanel`, `closeRight` and the
  documents/preview-mode branches go.
- `web_preview` events (camofox) no longer drive a screenshot feed: the
  `webPreviewStore` and the camofox `BrowserPanel` are deleted, and the event
  opens a Browser web tab at the reported URL instead. The agent's own browser
  tools are unchanged — they still drive the embedded view.

## 7. Title bar, settings, shortcuts

- Panels menu: one "Side panel" toggle (whichever edge is in use), then Files /
  Browser / Terminal toggles. Bottom and left/right container entries removed.
- Settings → Layout: chat side Left/Right only; presets trimmed to four.
- Shortcuts: `Mod+Shift+E` Files, `Mod+Shift+B` Browser, `` Mod+` `` Terminal.
  `panel.preview` removed.

## 8. What the user sees at launch

One right container with three tabs (Files active), the chat beside it, and the
terminal one click away. Closing the container leaves the chat alone; the
Panels menu brings the container back with all three tabs and their state.

## 9. Build order

1. **Surgery.** Registry, layout, zones, presets, title bar, shortcuts, delete
   dead components and triggers, bump the layout key. App compiles and shows
   three tabs with placeholder bodies.
2. **Files.** Tree, filter, change badges, inline diff, `revealFile` wired to
   chat links.
3. **Browser web tabs.** Rust label-aware webviews, tab strip, toolbar, state
   persistence, parking on hide.
4. **Browser file tabs.** PDF/office/image/audio/video viewers, `openDocument`
   retarget.
5. **Element picker.** Inject, poll, compose, send, screenshot.
6. **Verification.** Typecheck, launch, walk every panel and the picker on a
   local dev server.

## 10. Risks

- **Dead triggers.** Removing panels means removing every caller; a missed
  `openRight('agents')` is a type error at best, a dead button at worst. The
  typecheck covers the first.
- **Multi-webview positioning.** N child webviews means N rects to keep honest.
  Mitigation: only the active one is ever positioned; the rest are parked.
- **Picker polling.** Polling every 200ms only while picking; the picker must
  clear itself on navigation or the next page inherits a stale overlay.
- **Screenshot capture.** WebView2 capture can return black on some drivers;
  the fallback chain ends at "send without an image", which still carries
  selector, text and URL.
- **Layout migration.** A saved v2 layout names panels that no longer exist;
  `RETIRED` plus re-seeding must leave a valid three-tab container.
