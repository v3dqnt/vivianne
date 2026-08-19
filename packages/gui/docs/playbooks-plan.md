# Playbooks — architecture plan

A **playbook** is an installable package that extends Vivianne with a domain:
skills, MCP servers, agent tools, system-prompt directives, and — the part
Vivianne doesn't have today — its own top-level tab with its own panel
layout. Installing one doesn't replace Vivianne's core chat; it adds a
parallel surface that reuses Vivianne's existing primitives (docking panels,
tool-card rendering, skill loader, MCP client) plus whatever new pieces the
domain needs.

This plan covers: the package format, what runtime pieces Vivianne is
missing to support it, and the concrete build for the first two playbooks
(Science, Office). It's a plan, not an implementation — nothing here has
been built yet.

## What Vivianne already has (reuse as-is)

| Capability | Where | Reuse for playbooks as |
|---|---|---|
| SKILL.md loading (`~/.viv/skills`, YAML frontmatter, nested dirs) | `coding_agent::skills` | Drop-in — a playbook's skills are just more SKILL.md files in a playbook-scoped subdirectory |
| MCP client (stdio + Streamable HTTP, connect/list/call) | `crate::mcp` | Drop-in — a playbook can ship a list of MCP servers to auto-connect on install |
| Agent tool dispatch (`execute_tool_calls`, JSON tool defs) | `agent::tool_exec` | Drop-in — a playbook's tools are more entries in the same dispatch table, gated by which playbooks are enabled |
| Dockable panel-stack layout (`DockStack`, `layout` state, resizable groups) | `app/page.tsx`, `AgentProvider.tsx` | Drop-in — a playbook's panels are more docked panels; no new layout engine needed |
| Per-tool-name result rendering (`ToolCard.tsx`, `toolMeta.ts`) | `components/ToolCards/` | Extend — needs a registry so a playbook can add a renderer for its own tool names without editing `ToolCard.tsx` |
| Settings page (per-section nav, generic `settings.json` blob) | `SettingsPage.tsx` | Extend — a playbook can contribute its own settings section |

## What's missing

1. **A package format.** Nothing today describes "this bundle of skills + MCP
   servers + tools + prompt text + panels is one unit you can install/enable/
   disable/remove."
2. **An install/registry mechanism.** Skills, MCP configs, and settings all
   currently live in flat, playbook-unaware locations (`~/.viv/skills`,
   `settings.json`'s top-level keys). Need a `~/.viv/playbooks/<id>/` layout
   and a manifest of what's currently installed/enabled.
3. **A tab system.** `currentView` in `page.tsx` is a hardcoded
   `"chat" | "settings" | "agents"` union. A playbook needs to register a new
   tab that, when clicked, swaps the panel layout to its own arrangement.
4. **An artifact-renderer registry.** Science-domain tool results (a protein
   structure, a genome track) need dedicated inline viewers. `ToolCard.tsx`
   currently switches on hardcoded tool names — needs a registry a playbook
   can add to instead of editing core rendering code (this is the one piece
   worth directly borrowing from OpenScience's own design, see below).
5. **Prompt augmentation per playbook.** `directive::assemble_capability_blocks`
   builds the system prompt from a fixed set of blocks — needs an extension
   point for a playbook to append its own directive text (e.g. "you also have
   research-agent skills; use them for literature/experiment work").

## Prior art worth copying directly

`synthetic-sciences/OpenScience` (`C:\Users\v3dqn\Work\Project Versa\synthetic-sciences__openscience`)
is a mature, working example of almost exactly this problem, solved for a
single domain (science). Two things from it map directly onto our gap list:

- **Renderer registry** (`frontend/workspace/src/science/renderers/registry.ts`):
  a `Map<ArtifactKind, Component>` with `register()`/`get()`/`has()`, and a
  single dispatcher component (`ScienceArtifact.tsx`) that's the *only* thing
  that reads the registry. Renderer authors add one file + one `register()`
  call; the chat-rendering code path is never touched. We should build the
  same shape for `ToolCard.tsx`: a `registerToolRenderer(toolName, Component)`
  a playbook calls once, `ToolCard` looks up before falling back to its
  built-in generic rendering.
- **290+ domain skills as plain SKILL.md** (`backend/cli/skills/{biology,chemistry,physics,...}`):
  same frontmatter shape Vivianne's loader already parses (`name`,
  `description`, plus ignorable extra fields like `category`/`tags`/
  `dependencies`). These port with zero transformation — the Science
  playbook's skill set is "copy the relevant subdirectories in."
- Renderer implementations to port (rewrite from Solid to React, same
  underlying libraries): `ProteinStructure.tsx` (Mol*/3Dmol, PDB/mmCIF),
  `Chem2D.tsx` (RDKit-derived 2D depiction from SMILES), `GenomeTrack.tsx`
  (igv.js), `MsaViewer.tsx`, `SequenceViewer.tsx`, `PdfViewer.tsx` (pdfjs-dist
  — Vivianne doesn't render PDFs inline today either), `LatexView.tsx`
  (katex — Streamdown likely already has a math plugin wired, check before
  rebuilding).

## Package format

```
~/.viv/playbooks/
  science/
    playbook.json          # manifest (below)
    skills/                # SKILL.md files, loaded alongside ~/.viv/skills when enabled
      biology/...
      chemistry/...
    mcp-servers.json        # McpServerConfig[] to auto-connect when enabled
    directive.md            # appended to the system prompt when enabled
```

`playbook.json`:
```json
{
  "id": "science",
  "name": "Science",
  "version": "0.1.0",
  "description": "Research agent skills, scientific databases, and inline molecule/genome/plot rendering.",
  "tab": { "label": "Science", "icon": "TestTube02Icon" },
  "tools": ["fetch_uniprot", "fetch_pdb", "run_experiment", "..."],
  "panels": ["science-artifacts", "science-experiment-log"]
}
```

`id` doubles as the on-disk directory name and the tool-name/panel-id
namespace prefix, so two playbooks can never collide.

## Runtime pieces to build

Backend (`src-tauri`):
- `playbooks.rs`: `list_installed()`, `install(path_or_url)`, `enable(id)`,
  `disable(id)`, `uninstall(id)` — mirrors `automation.rs`'s
  load-from-disk/save-to-disk pattern, one `installed.json` tracking
  enabled state.
- On enable: merge the playbook's `skills/` into the skill-discovery path
  list (extend `coding_agent::skills`' scan to accept extra roots), call
  `crate::mcp::add_server` for each entry in `mcp-servers.json`, append
  `directive.md` to `assemble_capability_blocks`'s output, and merge
  `tools` into `get_agent_tool_definitions()`'s dynamic tool list (same
  mechanism as the MCP-tools merge already built this session).
- Tool implementations for playbook-specific tools live in
  `coding_agent/playbooks/<id>/` and get dispatched the same way MCP tools
  are — by a namespaced prefix (`science__fetch_uniprot`) in
  `execute_tool_calls`.

Frontend (`packages/gui/src`):
- `currentView` becomes `string` (not a fixed union), with a registry
  `{ id, label, icon, render() }[]` seeded with the built-in chat/settings/
  agents views. Installed+enabled playbooks contribute additional entries.
  The top bar's view switcher renders one tab per registry entry.
- A playbook's tab click swaps `layout` to a playbook-defined preset (which
  panels are docked where) — reuses the existing `DockStack`/`layout` state,
  just with a different default configuration loaded on tab-enter and the
  previous one restored on tab-exit.
- `registerToolRenderer(toolName, Component)` + `ToolCard` checking that
  registry before its built-in switch, mirroring OpenScience's
  `registry.ts`/`ScienceArtifact.tsx` split described above.
- Settings gets a new "Playbooks" nav section: install (pick a folder or
  paste a URL), list installed with enable/disable toggles, uninstall.

## Science playbook — concrete build list

Sourced from `synthetic-sciences__openscience`:
1. Port skill subdirectories: `biology/`, `chemistry/`, `physics/`,
   `research/`, `scholar-evaluation/` (skip `coding/`, `data-engineering/`,
   `llm-tools/` — Vivianne's base tool set already covers that ground).
2. Scientific database tools: pick the handful actually load-bearing for a
   first cut (UniProt, PDB, PubChem, arXiv, Semantic Scholar) as thin
   `web_fetch`-backed tools rather than porting all ~30 — expand later based
   on which skills actually reference which databases.
3. Renderers (React ports, see library list above): protein-structure,
   chem-2d, genome-track, sequence, msa, pdf. Latex — check Streamdown's
   existing math plugin (`@streamdown/math`, already wired in
   `ai-elements/message.tsx`) before building a new one.
4. Panels: an "Artifacts" panel (docked, shows the renderer for whatever
   the agent's last science-tool result was) and an "Experiment Log" panel
   (running list of hypothesis → run → result, one row per experiment).
5. Directive text: a short paragraph telling the model it has research-agent
   skills available and when to reach for the science tools vs. general
   coding tools.

## Office playbook — concrete build list

Less detailed in the request; proposed scope pending confirmation:
1. Skills: document generation/editing (Word/Excel/PowerPoint — the
   `anthropic-skills:docx/xlsx/pptx` skills already available in this
   environment are a strong starting point, adapted to SKILL.md format if
   not already in it), meeting-notes summarization, email drafting.
2. Tools: none obviously missing beyond what Vivianne's base tools +
   docx/xlsx/pptx skills already cover — likely a skills-only playbook, no
   new renderers needed (rendered documents open in the existing file
   preview panel).
3. Panels: none new needed unless a spreadsheet/slide preview is wanted
   beyond the code-block/text preview `FilePreviewPanel` already gives.

## Suggested build order

1. Renderer registry + `ToolCard` integration (small, unblocks everything
   else, no risk to existing tool rendering since it's a pure addition).
2. `currentView` → tab registry (small, unblocks the science tab).
3. Playbook manifest + install/enable/disable backend, wired to the
   existing skill-loader/MCP/tool-definition merge points.
4. Science playbook: skills port (mechanical), then renderers one at a time
   (each is its own scoped unit of work), then panels, then directive text.
5. Office playbook: skills port + directive text (no new renderers/panels
   expected).
