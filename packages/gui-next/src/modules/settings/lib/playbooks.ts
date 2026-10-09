/**
 * Playbooks — user-defined tools AND user-defined dock panels, no Rust
 * code, no restart. Backed by `coding_agent::playbooks::PlaybookDef` (no
 * `#[serde(rename_all)]`, so wire fields are plain as-declared names —
 * verified against the Rust struct directly, not assumed). Exactly one of
 * `command`/`panel` is set — see `isTool`/`isPanel` below, which mirror the
 * Rust `PlaybookDef::is_tool`/`is_panel` exactly (presence-based, no
 * separate "kind" field).
 */
export interface PlaybookDef {
  name: string
  description: string
  parameters: Record<string, unknown>
  command: string | null
  panel: { url: string } | null
}

export function isTool(p: Pick<PlaybookDef, 'command'>): boolean {
  return !!p.command?.trim()
}

export function isPanel(p: Pick<PlaybookDef, 'panel'>): boolean {
  return !!p.panel
}
