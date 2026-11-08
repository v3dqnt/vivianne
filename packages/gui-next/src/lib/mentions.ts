/**
 * `@skill` and `@file` mentions in the composer's text.
 *
 * A mention starts at an `@` that is preceded by whitespace or the start of the text (so an email address is not one), and runs to the
 * cursor: either a plain path (`@src/agent/loop`) or, for a path with spaces, an open quote (`@"My Folder/notes`). Every function takes the
 * cursor position and defaults to the end of the text, so editing in the middle of a message works the same as at its tail.
 */

export interface MentionRange {
  /** Index of the `@`. */
  start: number
  /** Where the mention ends: the cursor, or the end of the word the cursor is inside. */
  end: number
  /** What has been typed after the `@`, without a quote. */
  query: string
  quoted: boolean
}

const PATH_CHARS = 'A-Za-z0-9_./\\\\-'
const OPEN = new RegExp(`(^|\\s)@("[^"\\n]*|[${PATH_CHARS}]*)$`)
const REST_PLAIN = new RegExp(`^[${PATH_CHARS}]*`)

/** The mention being typed at `cursor`, if the cursor is in one. */
export function mentionAt(value: string, cursor: number = value.length): MentionRange | null {
  const before = value.slice(0, cursor)
  const m = before.match(OPEN)
  if (!m) return null
  const token = m[2]
  const quoted = token.startsWith('"')
  const start = before.length - token.length - 1
  // Past the cursor, the rest of the word belongs to the mention too, so completing it replaces the whole word.
  const after = value.slice(cursor)
  const restLen = quoted ? (after.match(/^[^"\n]*"?/)?.[0].length ?? 0) : (after.match(REST_PLAIN)?.[0].length ?? 0)
  return { start, end: cursor + restLen, query: quoted ? token.slice(1) : token, quoted }
}

/** The partial name or path after the `@`, or null when the cursor is not in a mention. */
export function getSkillMentionQuery(value: string, cursor: number = value.length): string | null {
  return mentionAt(value, cursor)?.query ?? null
}

/** A mention as it is written: a path with spaces in quotes, anything else bare. */
export function formatMention(name: string): string {
  return /\s/.test(name) ? `@"${name}"` : `@${name}`
}

function replaceMention(value: string, cursor: number, name: string): { text: string; cursor: number } {
  const r = mentionAt(value, cursor)
  if (!r) return { text: value, cursor }
  const mention = `${formatMention(name)} `
  const text = value.slice(0, r.start) + mention + value.slice(r.end).replace(/^ /, '')
  return { text, cursor: r.start + mention.length }
}

/** Replace the mention at the cursor with a completed `@name `. */
export function applySkillMention(value: string, skillName: string, cursor: number = value.length): string {
  return replaceMention(value, cursor, skillName).text
}

/** Same, for a workspace-relative file path (quoted when it has spaces). */
export function applyFileMention(value: string, path: string, cursor: number = value.length): string {
  return replaceMention(value, cursor, path).text
}

/** Where the cursor should land after completing the mention at `cursor`. */
export function cursorAfterMention(value: string, name: string, cursor: number = value.length): number {
  return replaceMention(value, cursor, name).cursor
}
