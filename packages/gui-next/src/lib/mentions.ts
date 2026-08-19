/**
 * Detects an in-progress `@skill` mention being typed at the very end of the
 * input (the composer is a plain textarea, not a rich cursor-aware editor,
 * so this only triggers for the token currently being typed at the tail,
 * not an arbitrary mid-text cursor position).
 * Requires the `@` to be preceded by whitespace or the start of the string
 * so it doesn't fire mid-word or on something like an email address.
 * Returns the partial name after `@`, or null when not in trigger state.
 */
export function getSkillMentionQuery(value: string): string | null {
  // Broadened to include `/`, `.`, and other path chars so an in-progress
  // file path (`@src/agent/loop_...`) is a valid mention query too.
  const match = value.match(/(^|\s)@([A-Za-z0-9_./\\-]*)$/)
  return match ? match[2] : null
}

/** Replace the trailing in-progress `@partial` with a completed `@name `,
 * preserving whatever preceded it (start-of-string or the whitespace char). */
export function applySkillMention(value: string, skillName: string): string {
  return value.replace(/(^|\s)@([A-Za-z0-9_./\\-]*)$/, (_full, lead: string) => `${lead}@${skillName} `)
}

/** Same as `applySkillMention` but for a workspace-relative file path. */
export function applyFileMention(value: string, path: string): string {
  return value.replace(/(^|\s)@([A-Za-z0-9_./\\-]*)$/, (_full, lead: string) => `${lead}@${path} `)
}
