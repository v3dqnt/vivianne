/**
 * How a finished shell command ended, for the small capsule on its row.
 *
 * A non-zero exit is the command's own answer (grep found nothing, a test failed), not the tool failing, so the row isn't red; the capsule says
 * what happened instead. A clean exit shows nothing.
 */
export interface ShellOutcome {
  label: string
  /** `bad` for a timeout or a signal; `warn` for an ordinary non-zero exit. */
  tone: 'warn' | 'bad'
}

export function shellOutcome(details: unknown): ShellOutcome | null {
  if (!details || typeof details !== 'object') return null
  const d = details as { exit_code?: unknown; timed_out?: unknown; streamed?: unknown }
  if (d.timed_out === true) return { label: 'timed out', tone: 'bad' }
  // Only shell results carry these; other tools' details never do.
  if (d.streamed !== true) return null
  if (typeof d.exit_code === 'number') return d.exit_code === 0 ? null : { label: `exit ${d.exit_code}`, tone: 'warn' }
  if (d.exit_code === null) return { label: 'killed', tone: 'bad' }
  return null
}
