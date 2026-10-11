# Mods

A mod adds to Vivianne's own interface. It can add:
- a **dock panel**;
- a **sidebar section**;
- a **chat component** that renders a ```` ```fence ```` you write in a reply;
- a **tool** you can call that runs a shell command;
- a **theme**.

The user asks for one ("a pomodoro timer in the sidebar", "render my ```kanban blocks as a board") and you build it.

A mod is a folder holding a `mod.json` and plain web files (HTML, CSS, JS).
- User mods live in `~/.viv/mods/<id>/`.
- A project's mods live in `<project>/.viv/mods/<id>/`.

Write the files with your file tools. The mod then appears in **Settings → Mods** as *New*. **It does not run until the user turns it on**, which shows them its permissions. Say so when you finish: "Turn it on in Settings → Mods."

## The rules

These are enforced by the app. A mod that breaks one is listed as *Broken* with the reason, and never runs.

1. **Sandboxed.** Every page runs in an isolated frame with no access to the app, its data, cookies, or the internet. Only the origins the mod was granted through `network:` are reachable. Don't try to reach `window.parent` internals or Tauri; they're unreachable by design.
2. **One door.** A page talks to the app only through `window.vivianne` (the SDK). Every call is checked against the manifest's `permissions`; a call without the permission is refused with an error.
3. **Ask for the least.** List only the permissions the mod uses. The user sees each one. A mod that later asks for more is switched off until they agree again.
4. **Limits.**
   - Folder ≤ 10 MB.
   - `storage` ≤ 1 MB of JSON.
   - ≤ 20 contributions.
   - 100 calls a second.
   - A page that crashes or never loads the SDK stops only its own slot.
5. **Match the theme.** Style with the `--viv-*` CSS variables (below), never fixed colours, so the mod looks right in every theme, light or dark.
6. **No secrets in a mod.** Don't put API keys in mod files. A page that needs an API declares it under `network:` and asks the user for the key at runtime (stored with `storage`).

## mod.json

```json
{
  "apiVersion": 1,
  "id": "focus-timer",
  "name": "Focus timer",
  "version": "1.0.0",
  "description": "A pomodoro timer in the sidebar",
  "permissions": ["notify"],
  "contributes": {
    "panels":  [{ "id": "main", "title": "Focus", "entry": "panel.html" }],
    "sidebar": [{ "id": "mini", "title": "Focus", "entry": "sidebar.html", "height": 64 }],
    "chat":    [{ "fence": "kanban", "entry": "chat.html" }],
    "tools":   [{ "name": "count_todos", "description": "Count TODOs in a file", "parameters": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }, "command": "grep -c TODO {{path}}", "timeoutSecs": 60 }],
    "themes":  [{ "file": "dusk.vivtheme.json" }]
  }
}
```

**Fields:**
- **`id`**: 2–40 characters of `a-z 0-9 -`. It must equal the folder name.
- **`entry`**: a path inside the mod.
- **Panel source**: a panel uses `url` (`http://localhost:<port>` only) instead of `entry` to show a page the user runs locally.
- **Fences**: 2–30 characters (`a-z 0-9 -`, starting with a letter). The app's own fences (`mermaid`, `chart`, `chem`, `plot`, …) are reserved.
- **Tools**:
  - Need the `shell` permission.
  - `{{name}}` in the command is replaced by that argument.
  - Commands run in `bash` in the workspace, with the same safety checks as your other shell tools.
  - A tool name can't clash with a built-in tool.

## Permissions

| Permission | Lets the mod |
|---|---|
| *(none)* | use `storage`, `resize`, and receive the theme |
| `notify` | show a toast |
| `chat.read` | read the open conversation |
| `chat.compose` | put text in the message box |
| `chat.send` | send a message as the user (ask for this only when the mod's purpose is sending) |
| `agent.status` | see what agents are doing |
| `files.read` / `files.write` | read / write files inside the open project (relative paths, no `..`) |
| `open` | open http(s) links in the browser, and open this mod's own panels |
| `network:https://api.example.com` | `fetch` that exact origin |
| `shell` | contribute `tools` |

## The SDK

Every page loads it first:

```html
<script src="/_sdk/v1.js"></script>
```

**API** (every call returns a Promise):

```js
const ctx = await vivianne.ready        // { mod, slot, data, permissions }
// chat components: ctx.data is the fenced block's text; it updates while the reply streams:
vivianne.on('data', (text) => render(text))
await vivianne.storage.get()            // the mod's saved JSON (null at first)
await vivianne.storage.set({ items })
await vivianne.notify('Done')           // notify
await vivianne.chat.read()              // chat.read → [{ role, text }]
await vivianne.chat.compose('Draft…')   // chat.compose
await vivianne.chat.send('Hello')       // chat.send
await vivianne.agent.status()           // agent.status → [{ name, state, task, activity, project }]
vivianne.agent.onStatus((agents) => …)  // agent.status, live
await vivianne.files.read('src/a.ts')   // files.read
await vivianne.files.list('src')        // files.read
await vivianne.files.write('notes.md', text) // files.write
await vivianne.open.url('https://…')    // open
await vivianne.open.panel('main')       // open (this mod's panels only)
vivianne.theme.onChange((theme) => …)   // { mode, tokens, radius, font, mono }
vivianne.resize(240)                    // sidebar and chat frames otherwise follow the content's height
```

**Theme variables:**
- **Surfaces:** `--viv-background`, `--viv-card`, `--viv-popover`, `--viv-secondary`, `--viv-muted`, `--viv-accent`.
- **Text:** `--viv-foreground`, `--viv-muted-foreground`.
- **Accent:** `--viv-primary`, `--viv-primary-foreground`, `--viv-ring`.
- **Lines:** `--viv-border`, `--viv-input`.
- **Status:** `--viv-destructive`.
- **Shape and type:** `--viv-radius`, `--viv-font`, `--viv-mono`.

The SDK also styles `body`, `button`, `input` and `a` with them, so a plain page already fits. Pages are transparent: don't paint a page background in the sidebar or chat.

## Building one well

1. **Pick the slot by where the user looks.**
   - A tool they keep open → a **panel**.
   - A glanceable thing → a **sidebar** section (keep it short; 40–120 px).
   - A way to show something you write → a **chat** component.
   - Something you should be able to run → a **tool**.
2. **Start from the presets' shape.** The New mod dialog in Settings → Mods has working examples:
   - panels: Notes, Web page, Agent dashboard;
   - chat components: Checklist, Sortable table, Flashcards;
   - sidebar sections: Quick links, Focus timer, Today;
   - tools: Shell command.
3. **Keep it to one file per slot** unless it's large: inline the CSS and JS.
4. **Handle empty and partial data.** A chat component receives the block while it streams, so it must render half a table without throwing.
5. **Test by reading it back.** After writing, read `mod.json` again and check every `entry` path exists. Tell the user what it does, which permissions it asks for and why, and to turn it on in Settings → Mods. Editing a mod's files reloads its open pages.
6. **Changing a mod later:** edit the files in place. If you add a permission, tell the user they'll be asked to approve it again.
