/**
 * A message as one line of plain text, for lists and notifications: a
 * ```ui summary card becomes its title (or "Summary") instead of JSON, and
 * other fenced blocks shrink to a word.
 */
export function previewText(text: string): string {
  return text
    .replace(/```ui\s*\n([\s\S]*?)(?:```|$)/g, (_all, body: string) => {
      const title = /"title"\s*:\s*"((?:[^"\\]|\\.)*)"/.exec(body)?.[1]
      return title ? `▤ ${title}` : '▤ Summary'
    })
    .replace(/```(\w*)\s*\n[\s\S]*?(?:```|$)/g, (_all, lang: string) => (lang ? `[${lang}]` : '[code]'))
    .replace(/\s+/g, ' ')
    .trim()
}
