/**
 * Rank names against what was typed: a name matches when the query's letters appear in it in order. Better matches first: exact, then prefix,
 * then a contiguous substring, then scattered letters; ties keep their original order. An empty query keeps everything in order.
 */
export function rankByName<T>(items: readonly T[], query: string, nameOf: (item: T) => string): T[] {
  const q = query.trim().toLowerCase()
  if (!q) return [...items]
  const scored: { item: T; score: number; index: number }[] = []
  items.forEach((item, index) => {
    const score = scoreName(nameOf(item).toLowerCase(), q)
    if (score !== null) scored.push({ item, score, index })
  })
  scored.sort((a, b) => a.score - b.score || a.index - b.index)
  return scored.map((s) => s.item)
}

/** Lower is better; null when the letters do not appear in order. */
function scoreName(name: string, q: string): number | null {
  if (name === q) return 0
  if (name.startsWith(q)) return 1
  if (name.includes(q)) return 2
  let at = 0
  for (const ch of q) {
    at = name.indexOf(ch, at)
    if (at < 0) return null
    at++
  }
  return 3
}
