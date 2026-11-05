/**
 * What the Git tab's diff view does with a diff before drawing it: parsing it
 * into numbered lines, hiding import-only changes, marking the words that
 * changed inside a line, and the command that re-applies it. Kept apart from
 * the drawing so it can be read and tested on its own.
 */

export interface DiffLine {
  kind: 'add' | 'del' | 'ctx' | 'hunk'
  text: string
  oldNo: number | null
  newNo: number | null
}

export type DiffMode = 'auto' | 'unified' | 'split'

/** The view's settings: the menu beside the diff. */
export interface DiffOptions {
  wrap: boolean
  mode: DiffMode
  /** The whole file around each change, not only a few lines of context. */
  fullFiles: boolean
  /** A rendered view of a file that has one (markdown). */
  richPreview: boolean
  /** The words that changed, marked inside a changed line. */
  wordDiffs: boolean
  hideWhitespace: boolean
  hideImports: boolean
}

export const DEFAULT_DIFF_OPTIONS: DiffOptions = {
  wrap: false,
  mode: 'auto',
  fullFiles: true,
  richPreview: false,
  wordDiffs: true,
  hideWhitespace: false,
  hideImports: false,
}

/** Parse a unified diff into numbered lines. */
export function parseDiff(diff: string): DiffLine[] {
  const out: DiffLine[] = []
  let oldNo = 0
  let newNo = 0
  for (const line of diff.split('\n')) {
    if (line.startsWith('@@')) {
      const m = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/)
      if (m) {
        oldNo = parseInt(m[1], 10)
        newNo = parseInt(m[2], 10)
      }
      out.push({ kind: 'hunk', text: line, oldNo: null, newNo: null })
    } else if (line.startsWith('+++') || line.startsWith('---') || line.startsWith('diff ') || line.startsWith('index ')) {
      continue
    } else if (line.startsWith('+')) {
      out.push({ kind: 'add', text: line.slice(1), oldNo: null, newNo: newNo++ })
    } else if (line.startsWith('-')) {
      out.push({ kind: 'del', text: line.slice(1), oldNo: oldNo++, newNo: null })
    } else if (line.startsWith('\\')) {
      continue
    } else {
      out.push({ kind: 'ctx', text: line.slice(1), oldNo: oldNo++, newNo: newNo++ })
    }
  }
  // Trailing blank from the final newline.
  if (out.length && out[out.length - 1].kind === 'ctx' && out[out.length - 1].text === '') out.pop()
  return out
}

/** A line that opens an import, and whether it stays open to later lines. */
function importStart(text: string, path: string): { is: boolean; open: boolean } {
  const t = text.trim()
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  if (['ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs'].includes(ext)) {
    if (/^import\s*\{[^}]*$/.test(t)) return { is: true, open: true }
    if (/^import\b/.test(t) || /^export\s+(\*|\{[^}]*\})\s+from\b/.test(t) || /^(const|let|var)\s+.+=\s*require\(/.test(t)) return { is: true, open: false }
    return { is: false, open: false }
  }
  if (ext === 'rs') {
    if (/^(pub(\([^)]*\))?\s+)?use\s+[^;]*\{[^}]*$/.test(t)) return { is: true, open: true }
    return { is: /^(pub(\([^)]*\))?\s+)?use\s/.test(t), open: false }
  }
  if (ext === 'py') return { is: /^(from\s+\S+\s+)?import\s/.test(t), open: false }
  if (ext === 'go') return { is: /^import\b/.test(t), open: false }
  return { is: false, open: false }
}

/**
 * The diff without the changes that are only imports. An import written over
 * several lines is hidden whole; a hunk left with nothing changed goes too.
 */
export function hideImports(lines: DiffLine[], path: string): DiffLine[] {
  const kept: DiffLine[] = []
  let inImport = false
  for (const l of lines) {
    if (l.kind === 'hunk' || l.kind === 'ctx') {
      kept.push(l)
      continue
    }
    if (inImport) {
      if (l.text.includes('}')) inImport = false
      continue
    }
    const start = importStart(l.text, path)
    if (start.is) {
      inImport = start.open
      continue
    }
    kept.push(l)
  }
  // A hunk header with no change after it, before the next header, is gone.
  const out: DiffLine[] = []
  for (let i = 0; i < kept.length; i++) {
    if (kept[i].kind !== 'hunk') {
      out.push(kept[i])
      continue
    }
    let j = i + 1
    let changed = false
    for (; j < kept.length && kept[j].kind !== 'hunk'; j++) if (kept[j].kind === 'add' || kept[j].kind === 'del') changed = true
    if (changed) out.push(...kept.slice(i, j))
    i = j - 1
  }
  return out
}

export interface Seg {
  text: string
  changed: boolean
}

const TOKEN = /\s+|\w+|[^\w\s]/g
/** Past this many tokens a line is not worth comparing word by word. */
const MAX_TOKENS = 300

/** The words that differ between an old and a new line, as marked segments. */
export function wordSegments(oldText: string, newText: string): { old: Seg[]; new: Seg[] } | null {
  const a = oldText.match(TOKEN) ?? []
  const b = newText.match(TOKEN) ?? []
  if (a.length > MAX_TOKENS || b.length > MAX_TOKENS) return null
  // Longest common subsequence of tokens.
  const lcs: number[][] = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0))
  for (let i = a.length - 1; i >= 0; i--) for (let j = b.length - 1; j >= 0; j--) lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1])
  const sameA = new Array<boolean>(a.length).fill(false)
  const sameB = new Array<boolean>(b.length).fill(false)
  for (let i = 0, j = 0; i < a.length && j < b.length; ) {
    if (a[i] === b[j]) {
      sameA[i] = sameB[j] = true
      i++
      j++
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) i++
    else j++
  }
  const join = (tokens: string[], same: boolean[]): Seg[] => {
    const segs: Seg[] = []
    tokens.forEach((text, i) => {
      const changed = !same[i]
      const last = segs[segs.length - 1]
      if (last && last.changed === changed) last.text += text
      else segs.push({ text, changed })
    })
    return segs
  }
  return { old: join(a, sameA), new: join(b, sameB) }
}

/**
 * Word segments for the lines of a changed run: the nth removed line is
 * compared with the nth added line, which is how an edited line sits opposite
 * the one it replaced. Keyed by the line's index in `lines`.
 */
export function wordDiffs(lines: DiffLine[]): Map<number, Seg[]> {
  const out = new Map<number, Seg[]>()
  for (let i = 0; i < lines.length; ) {
    if (lines[i].kind !== 'del') {
      i++
      continue
    }
    const dels: number[] = []
    const adds: number[] = []
    while (i < lines.length && lines[i].kind === 'del') dels.push(i++)
    while (i < lines.length && lines[i].kind === 'add') adds.push(i++)
    for (let k = 0; k < Math.min(dels.length, adds.length); k++) {
      const seg = wordSegments(lines[dels[k]].text, lines[adds[k]].text)
      if (!seg) continue
      // A line replaced wholesale is not a word edit: marking all of it says nothing.
      if (seg.old.every((s) => s.changed) && seg.new.every((s) => s.changed)) continue
      out.set(dels[k], seg.old)
      out.set(adds[k], seg.new)
    }
  }
  return out
}

/** The command that applies a patch to a checkout, to paste into a shell. */
export function gitApplyCommand(patch: string): string {
  const body = patch.endsWith('\n') ? patch : `${patch}\n`
  return `git apply <<'VIV_PATCH'\n${body}VIV_PATCH`
}
