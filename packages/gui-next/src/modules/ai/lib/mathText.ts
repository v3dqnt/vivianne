/**
 * Normalise the ways models write maths into the `$…$` / `$$…$$` form that
 * remark-math parses.
 *
 * - `\(…\)` and `\[…\]` (the LaTeX-native delimiters many models prefer) are
 *   rewritten to `$…$` and `$$…$$`.
 * - A lone dollar sign on a line — "it costs $5, or $8 with tax" — is escaped,
 *   so prices are never swallowed into an inline formula. A dollar only opens
 *   maths when it has a partner on the same line.
 *
 * Fenced and inline code are left untouched: a `$` in a shell snippet is not
 * maths, and neither is `\(` in a regex.
 */
export function normalizeMath(text: string): string {
  if (!text.includes('$') && !text.includes('\\(') && !text.includes('\\[')) return text
  const out: string[] = []
  let inFence = false
  for (const line of text.split('\n')) {
    if (/^\s*(```|~~~)/.test(line)) {
      inFence = !inFence
      out.push(line)
      continue
    }
    out.push(inFence ? line : normalizeLine(line))
  }
  return out.join('\n')
}

function normalizeLine(line: string): string {
  // Split out inline code spans so they pass through verbatim.
  const parts = line.split(/(`+[^`]*`+)/)
  return parts
    .map((part, i) => {
      if (i % 2 === 1) return part
      const s = part
        .replace(/\\\[/g, '$$$$')
        .replace(/\\\]/g, '$$$$')
        .replace(/\\\(/g, '$$')
        .replace(/\\\)/g, '$$')
      return escapeCurrency(s)
    })
    .join('')
}

/**
 * Pandoc's rule for inline maths: `$…$` only counts when the opening dollar
 * is followed by a non-space, the closing one is preceded by a non-space, and
 * the closing one is not followed by a digit. Every dollar that can't be part
 * of such a pair is a literal — "$5, or $8" is two prices, not a formula.
 */
function escapeCurrency(s: string): string {
  const singles: number[] = []
  for (let i = 0; i < s.length; i++) {
    if (s[i] !== '$') continue
    if (s[i + 1] === '$') {
      i++ // `$$` is display maths; leave it to remark-math
      continue
    }
    if (i > 0 && (s[i - 1] === '\\' || s[i - 1] === '$')) continue
    singles.push(i)
  }
  const literal = new Set<number>()
  let k = 0
  while (k < singles.length) {
    const open = singles[k]
    const close = singles[k + 1]
    const valid =
      close !== undefined &&
      !/\s/.test(s[open + 1] ?? ' ') &&
      !/\s/.test(s[close - 1] ?? ' ') &&
      !/\d/.test(s[close + 1] ?? '')
    if (valid) {
      k += 2
    } else {
      literal.add(open)
      k += 1
    }
  }
  if (!literal.size) return s
  let out = ''
  for (let i = 0; i < s.length; i++) out += literal.has(i) ? '\\$' : s[i]
  return out
}
