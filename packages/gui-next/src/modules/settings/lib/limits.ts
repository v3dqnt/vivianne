/**
 * The agent limits in Settings → Reliability, edited as a staged draft: changes sit in the form until Save, a limit that differs from the
 * built-in default is marked, and each can be reset to its default.
 *
 * The ranges mirror `agent/tools/limits.rs`, which clamps whatever is stored, so a value outside them here is caught before it is saved
 * rather than silently changed afterwards.
 */

export interface LimitDef {
  /** The key in the settings file. */
  key: 'shellTimeoutSecs' | 'maxActiveSubagents'
  label: string
  help: string
  unit: string
  fallback: number
  min: number
  max: number
}

export const LIMITS: readonly LimitDef[] = [
  {
    key: 'shellTimeoutSecs',
    label: 'Shell command timeout',
    help: 'How long a command may run before it is stopped and what it printed so far is returned. A command can ask for a different time, up to 10 minutes.',
    unit: 'seconds',
    fallback: 120,
    min: 5,
    max: 600,
  },
  {
    key: 'maxActiveSubagents',
    label: 'Subagents at once',
    help: 'How many subagents may work at the same time. Each is a full agent run, so more of them use more of your plan and your machine.',
    unit: 'agents',
    fallback: 8,
    min: 1,
    max: 16,
  },
]

export type LimitValues = Record<LimitDef['key'], number>

/** The saved values, with the default wherever nothing (or something unusable) is saved. */
export function savedLimits(settings: Record<string, unknown> | null | undefined): LimitValues {
  const out = {} as LimitValues
  for (const d of LIMITS) {
    const raw = settings?.[d.key]
    out[d.key] = typeof raw === 'number' && Number.isFinite(raw) ? Math.min(d.max, Math.max(d.min, Math.round(raw))) : d.fallback
  }
  return out
}

/** Why a typed value cannot be saved, or null when it can. */
export function limitProblem(def: LimitDef, text: string): string | null {
  if (text.trim() === '') return 'Enter a number.'
  const n = Number(text)
  if (!Number.isInteger(n)) return 'Use a whole number.'
  if (n < def.min || n > def.max) return `Between ${def.min} and ${def.max} ${def.unit}.`
  return null
}

/** Whether a value differs from the built-in default. */
export function isOverridden(def: LimitDef, value: number): boolean {
  return value !== def.fallback
}

/** The settings keys whose staged value differs from what is saved. */
export function changedKeys(saved: LimitValues, staged: Record<LimitDef['key'], string>): LimitDef['key'][] {
  return LIMITS.filter((d) => Number(staged[d.key]) !== saved[d.key]).map((d) => d.key)
}
