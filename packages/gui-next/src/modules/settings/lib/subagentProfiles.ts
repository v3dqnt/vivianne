/** Mirrors `coding_agent::subagent_profiles::SubagentModelOverride` —
 * `#[serde(rename_all = "camelCase")]`. Enough of a `ModelRef` to actually
 * run a turn with; a bare model id isn't resolvable Rust-side on its own. */
export interface SubagentModelOverride {
  id: string
  name: string
  api: string
  provider: string
  baseUrl: string | null
  maxTokens: number
  contextWindow: number
}

export const SUBAGENT_COLORS = ['amber', 'red', 'orange', 'green', 'teal', 'blue', 'purple', 'pink'] as const

export const SUBAGENT_COLOR_HEX: Record<(typeof SUBAGENT_COLORS)[number], string> = {
  amber: '#f5b942',
  red: '#e5484d',
  orange: '#f5793a',
  green: '#3dd68c',
  teal: '#2dd4bf',
  blue: '#4a9eff',
  purple: '#a78bfa',
  pink: '#ec4899',
}
