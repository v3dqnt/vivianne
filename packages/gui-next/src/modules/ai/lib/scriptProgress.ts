/**
 * Progress lines a running script reports with `log()` and `phase()`.
 *
 * Each line arrives as a `script_progress` event naming the tool call it belongs to. Only the most recent lines are kept, so a script
 * that logs in a loop cannot grow the card without bound.
 */
export const MAX_PROGRESS_LINES = 200

export interface ProgressEvent {
  callId: string
  text: string
}

/** `lines` with `event` added when it belongs to `callId`, trimmed to the newest `MAX_PROGRESS_LINES`. */
export function withProgress(lines: readonly string[], event: ProgressEvent | null | undefined, callId: string): string[] {
  if (!event || event.callId !== callId || typeof event.text !== 'string' || event.text === '') return lines as string[]
  const next = [...lines, event.text]
  return next.length > MAX_PROGRESS_LINES ? next.slice(next.length - MAX_PROGRESS_LINES) : next
}

/** A phase marker line (`== name ==`) as the phase's name, else null. */
export function phaseName(line: string): string | null {
  const m = /^== (.+) ==$/.exec(line)
  return m ? m[1] : null
}
