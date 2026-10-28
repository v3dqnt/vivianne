export interface DiffLine {
  kind: 'add' | 'del' | 'ctx'
  oldNo: number | null
  newNo: number | null
  text: string
}

export interface DiffHunk {
  /** The `@@ … @@` line, kept verbatim for the sticky header. */
  header: string
  lines: DiffLine[]
}

const HUNK = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/

/** Split unified diff text into hunks with old and new line numbers. File
 * headers (`diff --git`, `---`, `+++`, `index`) are dropped: the panel already
 * knows which file it is showing. */
export function parseDiff(text: string): DiffHunk[] {
  const hunks: DiffHunk[] = []
  let hunk: DiffHunk | null = null
  let oldNo = 0
  let newNo = 0

  // The diff ends with a newline; the empty string after it is not a line.
  for (const raw of text.replace(/\r?\n$/, '').split(/\r?\n/)) {
    const m = HUNK.exec(raw)
    if (m) {
      oldNo = Number(m[1])
      newNo = Number(m[2])
      hunk = { header: raw.trimEnd(), lines: [] }
      hunks.push(hunk)
      continue
    }
    if (!hunk) continue
    if (raw.startsWith('\\')) continue // "\ No newline at end of file"
    const first = raw[0]
    if (first === '+') hunk.lines.push({ kind: 'add', oldNo: null, newNo: newNo++, text: raw.slice(1) })
    else if (first === '-') hunk.lines.push({ kind: 'del', oldNo: oldNo++, newNo: null, text: raw.slice(1) })
    else if (first === ' ' || raw === '') hunk.lines.push({ kind: 'ctx', oldNo: oldNo++, newNo: newNo++, text: raw.slice(1) })
    else hunk = null // a new file header: the hunk is over
  }
  return hunks
}
