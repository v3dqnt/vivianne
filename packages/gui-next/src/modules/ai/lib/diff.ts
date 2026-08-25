/**
 * Small line-based diff (classic O(N*M) LCS table — fine for the file sizes
 * an edit/write/apply_patch call realistically touches; no need for a real
 * diff library dependency for this).
 *
 * Two guards keep a pathological input (a full-file rewrite of a large file)
 * from stalling the render thread: past `MAX_LCS_CELLS` the O(N*M) table is
 * skipped in favor of an O(N+M) prefix/suffix trim (not a minimal diff, but
 * bounded), and the emitted line list is capped at `MAX_RENDERED_LINES`
 * regardless of which path produced it, so the DOM stays bounded too.
 */
export type DiffLine = { kind: 'add' | 'remove' | 'context'; text: string }

const MAX_LCS_CELLS = 250_000
const MAX_RENDERED_LINES = 2000

export function diffLines(before: string, after: string): DiffLine[] {
  const a = before.split('\n')
  const b = after.split('\n')
  const lines = a.length * b.length > MAX_LCS_CELLS ? coarseDiff(a, b) : lcsDiff(a, b)
  return capLines(lines)
}

function capLines(lines: DiffLine[]): DiffLine[] {
  if (lines.length <= MAX_RENDERED_LINES) return lines
  const head = lines.slice(0, MAX_RENDERED_LINES)
  head.push({ kind: 'context', text: `… ${lines.length - MAX_RENDERED_LINES} more lines truncated …` })
  return head
}

// Exact minimal diff via the LCS table. Only cheap for inputs under
// MAX_LCS_CELLS — an n*m table for a 2000-line rewrite is ~4M cells and
// multi-second on the main thread.
function lcsDiff(a: string[], b: string[]): DiffLine[] {
  const n = a.length
  const m = b.length

  // lcs[i][j] = length of the longest common subsequence of a[i:] and b[j:]
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0))
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1])
    }
  }

  const out: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ kind: 'context', text: a[i] })
      i++
      j++
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      out.push({ kind: 'remove', text: a[i] })
      i++
    } else {
      out.push({ kind: 'add', text: b[j] })
      j++
    }
  }
  while (i < n) {
    out.push({ kind: 'remove', text: a[i] })
    i++
  }
  while (j < m) {
    out.push({ kind: 'add', text: b[j] })
    j++
  }
  return out
}

// O(N+M) fallback for large inputs: trims the common prefix and suffix, then
// emits the differing middle as one remove-block followed by one add-block.
// Not a minimal diff (no line-level matching inside the changed region), but
// it never blows up — the trim alone already captures most of a real edit's
// true changed region.
function coarseDiff(a: string[], b: string[]): DiffLine[] {
  let start = 0
  const maxStart = Math.min(a.length, b.length)
  while (start < maxStart && a[start] === b[start]) start++

  let aEnd = a.length
  let bEnd = b.length
  while (aEnd > start && bEnd > start && a[aEnd - 1] === b[bEnd - 1]) {
    aEnd--
    bEnd--
  }

  const out: DiffLine[] = []
  for (let i = 0; i < start; i++) out.push({ kind: 'context', text: a[i] })
  for (let i = start; i < aEnd; i++) out.push({ kind: 'remove', text: a[i] })
  for (let j = start; j < bEnd; j++) out.push({ kind: 'add', text: b[j] })
  for (let i = aEnd; i < a.length; i++) out.push({ kind: 'context', text: a[i] })
  return out
}
