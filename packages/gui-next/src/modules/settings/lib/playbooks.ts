/**
 * Playbooks — user-defined tools, no Rust code, no restart. Backed by
 * `coding_agent::playbooks::PlaybookDef` (no `#[serde(rename_all)]`, so wire
 * fields are plain as-declared names — verified against the Rust struct
 * directly, not assumed).
 */
export interface PlaybookDef {
  name: string
  description: string
  parameters: Record<string, unknown>
  command: string
}
