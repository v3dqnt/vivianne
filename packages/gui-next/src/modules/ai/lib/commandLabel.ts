/** A shell command as a tool row shows it: without the `cd <folder> &&` an
 * agent puts in front of nearly every call, which pushed the part that
 * differs off the end of the row. The full command is still in the card. */
export function commandLabel(command: string): string {
  const m = /^\s*cd\s+(?:"[^"]*"|'[^']*'|[^\s;&|]+)\s*(?:&&|;)\s*/.exec(command)
  const rest = m ? command.slice(m[0].length) : command
  return rest.trim() || command.trim()
}
