# Extending Vivianne: tools, skills, and panels

Two independent ways to extend Vivianne without writing Rust or touching
the frontend build — each is a JSON or Markdown file dropped into a folder
under `~/.viv/`, picked up live (no restart). Each also has an in-app editor
in Settings if you'd rather not hand-write the file.

Playbooks are one manifest shape that does double duty as either a
model-callable tool or a dock panel, decided purely by which field is
present — see §1 below. There is no separate "panels" system or directory;
an earlier version of this app had one (`~/.viv/panels/`, its own Settings
tab), and it was merged into playbooks because the two were doing the same
"drop a manifest, get an extension" job with needless duplication.

> **Note on scope.** `packages/gui/docs/playbooks-plan.md` in this repo
> describes a much larger, unbuilt vision — "playbooks" as installable
> packages bundling skills + MCP servers + tools + a whole custom tab/panel
> layout. That plan predates the current frontend (it references
> `app/page.tsx`/`AgentProvider.tsx`, which no longer exist — the app was
> rewritten as `gui-next`) and was never implemented. This guide documents
> what's actually built today: three separate, focused extension points, not
> that unified package system. Worth knowing the bigger vision exists if
> you're ever picking up that project again, but don't confuse it with what's
> described below.

---

## 1. Playbooks (tools *and* panels)

A **playbook** is one JSON file that is *either* a model-callable tool *or* a
dock panel — never both, decided purely by which of two optional fields is
present: `command` makes it a tool, `panel` makes it a panel. There's no
separate `"kind"`/`"type"` field to keep in sync; `PlaybookDef::is_tool()` /
`is_panel()` (Rust) and `isTool()` / `isPanel()` (`lib/playbooks.ts`) are the
single source of truth on each side, and both just check field presence.

**Location:** `~/.viv/playbooks/<anything>.json`
**Backend:** `packages/gui/src-tauri/src/coding_agent/playbooks.rs`
**In-app editor:** Settings → Mods → Playbooks → New (has a Tool/Panel toggle)

### Tool format

```json
{
  "name": "count_todos",
  "description": "Count TODO/FIXME comments in a file.",
  "parameters": {
    "type": "object",
    "properties": {
      "path": { "type": "string", "description": "File to search" }
    },
    "required": ["path"]
  },
  "command": "grep -c 'TODO\\|FIXME' {{path}} || echo 0"
}
```

- `name` — the tool name the model calls. Must be unique across every
  built-in tool, MCP tool, and other playbook (unchecked at save time —
  a duplicate silently shadows whichever loads last).
- `description` — shown to the model exactly as written. Be as specific as
  a built-in tool's description; this is the only thing the model has to
  decide whether/how to call it.
- `parameters` — a standard JSON-schema object (`type`, `properties`,
  `required`). Omit entirely for a no-argument tool; it defaults to
  `{"type":"object","properties":{}}`.
- `command` — a bash command (always run via `bash -c`, in the current
  workspace's directory). `{{key}}` is replaced with that argument's value —
  strings substitute raw, everything else (numbers, booleans, arrays)
  substitutes as its JSON text form. There's no escaping/quoting done for
  you: if an argument might contain spaces or shell metacharacters, quote it
  yourself in the template (`"{{path}}"`) or the model's model-supplied value
  could break your command — this is a real command running with your own
  shell environment, not a sandbox.

### Panel format

```json
{
  "name": "storybook",
  "description": "Local Storybook instance",
  "panel": { "url": "http://localhost:6006" }
}
```

- `name` — a short slug. This is what `set_panel`'s `custom:<name>` value
  refers to, and what shows in the title bar's Panels menu and the panel's
  own header — playbooks have no separate `id`/`title` field, `name` serves
  both roles for tools and panels alike.
- `panel.url` — **must** start with `http://localhost:` or
  `http://127.0.0.1:`. This isn't an arbitrary restriction: the app's
  Content-Security-Policy (`tauri.conf.json`'s `frame-src`) only allows
  framing `'self'` and localhost origins. A panel pointed at any other URL is
  rejected at save time (`is_allowed_panel_url`) and skipped (with a warning)
  if it somehow ends up in the file anyway — it would just render blank,
  CSP-blocked, so failing loudly earlier is more useful than a panel that
  silently doesn't work.

A panel is for **your own local tooling** — a Storybook instance, a local
metrics dashboard, a project-specific dev server you already run — not for
embedding arbitrary third-party websites. If you want that, you're looking
for a browser tab, not a dock panel; dock panels run inside the app's own
webview with access to the same window chrome, a meaningfully different
trust boundary than a sandboxed browser tab.

### Opening a panel

- Manually, from the title bar's **Panels** menu — panel playbooks appear
  below the built-in list, separated visually.
- From the agent, via the existing `set_panel` tool:
  `{"panel": "custom:storybook", "action": "open"}`. The backend validates
  the name against what's actually installed right now (not a fixed list
  baked into the schema, since playbooks are added/removed live) — an
  unknown name fails with a clear error rather than silently doing nothing.

### How it's loaded

`create_coding_tools()` (called fresh on every single turn) calls
`playbooks::load_playbooks()`, which scans `~/.viv/playbooks/*.json` and
parses each as a `PlaybookDef`, then filters to `.is_tool()` before
registering — panel-type playbooks are skipped here since they're not
model-callable. A file that fails to parse, or a panel whose URL fails
`is_allowed_panel_url`, is skipped (logged as a warning) rather than breaking
every other entry. Each valid tool becomes a `PlaybookTool` — the same
`Tool` trait every built-in tool implements — so from the model's
perspective a playbook tool is indistinguishable from a hand-written one.

Execution goes through the exact same command-safety check every shell-backed
tool (`bash`, `powershell`) already uses (`sandbox::check_command` — the same
denylist and environment scrubbing), a 120-second timeout, and output
truncated the same way `bash`'s output is (400 lines / 50,000 bytes, longer
output spilled to disk with the path told to the model).

On the frontend, `panelStore`'s `rightKind` has one value, `'custom'`,
alongside the fixed built-ins (`review`/`files`/`preview`/`tasks`/
`multiplayer`) — but unlike those, `'custom'` is a single shared slot: which
panel it's actually showing is tracked separately as `customPanelId` (a
playbook's `name`), and `CustomPanel.tsx` looks that name up against
`playbookStore`'s list, filtered to `isPanel`, on every render (so switching
which panel is open, or a panel being removed out from under you, both just
work without the shell needing to know the installed list itself). It
renders as a plain `<iframe>` — no special message-passing bridge to the
host app; a panel is exactly as capable as any other page loaded in an
iframe with `allow-scripts allow-forms allow-same-origin allow-popups`, no
more.

### Authoring in-app vs. by hand

Settings → Mods → Playbooks → **New** opens a form with a Tool/Panel toggle
at the top. In Tool mode: name, description, a parameter list builder
(name/type/description/required per row — this generates the JSON schema for
you), and the command template. In Panel mode: name, description, and a
single URL field. **Import file** instead picks an already-written `.json`
file from disk and copies it in verbatim (useful for sharing a playbook
someone else wrote), valid as either a tool or a panel. Both paths write the
exact same file format described above — there's no difference in what
runs.

---

## 2. Skills

A **skill** is a Markdown file with YAML frontmatter — a name, a description,
and instructions in the body. Unlike a tool, a skill isn't something the
model *calls*; it's reference material the model reads when the
description matches what it's doing (or when the user explicitly invokes it
by name, if your frontend exposes that).

**Location:** `~/.viv/skills/` (nested subdirectories are scanned too)
**Backend:** `packages/gui/src-tauri/src/coding_agent/skills.rs`

### Format

```markdown
---
name: pr-description
description: Write a pull request description in this repo's house style — a Summary bullet list, then a Test plan checklist.
---

When asked to write a PR description:

1. Run `git log main..HEAD --oneline` and `git diff main...HEAD` to see
   what's actually in the branch.
2. Structure it as:
   - `## Summary` — 1-3 bullets, each starting with a verb
   - `## Test plan` — a markdown checklist
3. Keep it under 150 words total. Don't restate the diff; explain intent.
```

Frontmatter fields (all optional except `name`/`description`, which is what
the model sees before deciding to read further):

| Field | Type | Meaning |
|---|---|---|
| `name` | string | Shown in the skill list; also how a user invokes it by `/name` if the frontend wires that up |
| `description` | string | What the model sees in its skill list — specific enough that it can tell *when* to reach for this without reading the body |
| `disable_model_invocation` | bool | If `true`, the model can't select this skill on its own — only a user-triggered invocation loads it |
| `requires_provider` | string | e.g. `"openai"` — the skill needs that provider's API key configured; Settings shows it as unavailable otherwise, so a skill that calls a specific provider's API doesn't silently fail |

The rest of the file (everything after the closing `---`) is the skill's
actual content — plain Markdown, no special syntax beyond what the model
itself understands. Write it the way you'd write instructions for a new
teammate: concrete steps, house-style rules, gotchas specific to this repo.

### How it's loaded

`load_skills_from_dir` walks the directory (recursively — organize related
skills into subfolders freely), parses each `.md` file's frontmatter, and
skips (with a reported error, visible in Settings → Skills) anything that
doesn't parse. A skill whose `requires_provider` isn't configured still
loads — it's just marked `"missing_dependency"` so Settings can show why it's
greyed out, rather than hiding it entirely.

---

## Adding a third extension point

If you're adding a new kind of user-extensible thing, this codebase's
established shape for "drop a file in `~/.viv/<name>/`, no restart" is worth
copying rather than reinventing:

Before reaching for a brand new directory/module, consider whether what
you're adding is actually just a third *kind* of playbook (another optional
field on `PlaybookDef`, another `is_<kind>()` presence check) — that's
almost always less duplication than a fully separate system, which is
exactly the mistake panels-as-their-own-system made the first time around.

If it's genuinely unrelated to playbooks, this codebase's established shape
for "drop a file in `~/.viv/<name>/`, no restart" is:

1. **Directory helper** in `agent/paths.rs` — `viv_<name>_dir()`, one line,
   creates the directory if missing (see `viv_playbooks_dir` for the
   template).
2. **A `<name>.rs` module** in `coding_agent/` with: a `#[derive(Serialize,
   Deserialize)]` struct for the manifest shape, a `load_<name>s()` that
   scans the directory and skips-and-logs anything invalid (never fails the
   whole load over one bad file), and whatever runtime hook actually wires
   it in (a `Tool` impl for something the model calls, a Tauri command for
   something the frontend reads).
3. **Tauri commands** in `agent/commands.rs` — `cmd_list_<name>s` (read-only,
   mirrors what the runtime hook sees) and, if in-app authoring makes sense,
   `cmd_save_<name>` (create/update, with the same "rename removes the old
   file" handling `cmd_save_playbook` does) and `cmd_remove_<name>`. Register
   all three in `lib.rs`'s `invoke_handler![...]` list.
4. **Frontend store** — a small Zustand store (`<name>Store.ts`) with
   `refresh`/`save`/`remove`, matching `playbookStore.ts`'s shape exactly.
5. **Settings section** — a list + create/edit dialog, matching
   `ModsSection.tsx`'s `PlaybooksSubsection` shape.

The reason every one of these reloads live with no restart: the loader
function is called fresh on every use (`create_coding_tools` rebuilds the
whole tool list every turn; `load_playbooks` is called on every
`cmd_list_playbooks`) rather than cached at startup. That's a deliberate
choice worth keeping — don't cache a `load_<name>s()` result at app startup
without a real reason to, or you reintroduce the "have to restart to see my
change" friction this whole pattern exists to avoid.
