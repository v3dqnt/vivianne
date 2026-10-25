/**
 * File names in a prompt: which words are files, what a half-typed one is
 * probably meant to be, and what a mistyped one probably was.
 */

export interface KnownFile {
  /** The file's name: "ChatComposer.tsx". */
  name: string
  /** The path to put in the prompt when the name alone is ambiguous. */
  path: string
}

/** A word-ish run: letters, digits, dots, slashes, dashes, underscores, an
 * optional leading @. */
const TOKEN = /@?[\w.\\/-]+/g

const FILE_LIKE = /\.[A-Za-z0-9]{1,8}$/

/** The token the caret is at the end of, when there is one. */
export function tokenBeforeCaret(text: string, caret: number): { start: number; end: number; token: string } | null {
  if (caret <= 0 || caret > text.length) return null
  // The caret must be at the end of a word, not in the middle of one.
  if (caret < text.length && /[\w.\\/-]/.test(text[caret])) return null
  let start = caret
  while (start > 0 && /[\w.\\/@-]/.test(text[start - 1])) start--
  const token = text.slice(start, caret)
  return token ? { start, end: caret, token } : null
}

export function editDistance(a: string, b: string, cap = 3): number {
  if (Math.abs(a.length - b.length) > cap) return cap + 1
  let prev = Array.from({ length: b.length + 1 }, (_, i) => i)
  for (let i = 1; i <= a.length; i++) {
    const cur = [i]
    let best = i
    for (let j = 1; j <= b.length; j++) {
      const v = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1))
      cur.push(v)
      if (v < best) best = v
    }
    if (best > cap) return cap + 1
    prev = cur
  }
  return prev[b.length]
}

export interface Suggestion {
  start: number
  end: number
  /** What the word should become. */
  replacement: string
  /** "complete": a prefix of a file. "fix": a near miss of one. */
  kind: 'complete' | 'fix'
}

const norm = (p: string) => p.replace(/\\/g, '/').toLowerCase()

/** Whether `word` names a known file, by name or by path. */
export function isKnown(word: string, files: KnownFile[]): boolean {
  const w = norm(word.replace(/^@/, ''))
  if (!w) return false
  return files.some((f) => f.name.toLowerCase() === w || norm(f.path) === w || norm(f.path).endsWith('/' + w))
}

/** What the word being typed should become, if anything. */
export function suggest(text: string, caret: number, files: KnownFile[]): Suggestion | null {
  const t = tokenBeforeCaret(text, caret)
  if (!t || t.token.startsWith('@')) return null // mentions have their own menu
  const word = t.token
  if (word.length < 3 || !/[A-Za-z]/.test(word) || isKnown(word, files)) return null
  const lower = word.toLowerCase()

  // A prefix of a file's name: the shortest such name, never an ambiguous one
  // chosen blindly.
  const prefixed = files.filter((f) => f.name.toLowerCase().startsWith(lower) && f.name.length > word.length)
  if (prefixed.length > 0) {
    const shortest = prefixed.map((f) => f.name.length).reduce((a, b) => Math.min(a, b))
    const best = prefixed.filter((f) => f.name.length === shortest)
    const names = new Set(best.map((f) => f.name))
    if (names.size === 1) {
      // Several files may share this name: the name is all that is known,
      // and choosing one folder for the user would be a guess.
      return { start: t.start, end: t.end, replacement: best[0].name, kind: 'complete' }
    }
  }

  // A near miss of a file's name, only for words that read as file names.
  if (FILE_LIKE.test(word)) {
    const cap = word.length < 8 ? 1 : 2
    const near = files.filter((f) => f.name.length > 2 && editDistance(lower, f.name.toLowerCase(), cap) <= cap)
    const names = new Set(near.map((f) => f.name))
    if (names.size === 1) {
      return { start: t.start, end: t.end, replacement: near[0].name, kind: 'fix' }
    }
  }
  return null
}

/** The ranges of `text` that are file names: mentions, and words that match a
 * known file. */
export function highlights(text: string, files: KnownFile[]): { start: number; end: number }[] {
  const out: { start: number; end: number }[] = []
  for (const m of text.matchAll(TOKEN)) {
    let word = m[0]
    let start = m.index ?? 0
    // Trailing sentence punctuation is not part of the name.
    const trimmed = word.replace(/[.,;:!?)]+$/, '')
    word = trimmed
    if (word.length < 3) continue
    const mention = word.startsWith('@')
    if (mention || (FILE_LIKE.test(word) && isKnown(word, files)) || (!FILE_LIKE.test(word) && isKnown(word, files) && word.includes('/'))) {
      out.push({ start, end: start + word.length })
    }
  }
  return out
}
