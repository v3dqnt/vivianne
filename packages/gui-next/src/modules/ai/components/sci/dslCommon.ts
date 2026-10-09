/**
 * The small toolkit every science script parser shares: numbered lines,
 * quote- and bracket-aware tokens, numbers with units, and one error type
 * that always says which line was at fault.
 */

/** A script problem, with the 1-based line of the script it came from. */
export class DslError extends Error {
  line?: number
  constructor(message: string, line?: number) {
    super(message)
    this.name = 'DslError'
    this.line = line
  }
}

export interface Line {
  /** 1-based line in the script as written. */
  n: number
  text: string
}

/** The meaningful lines of a script: trimmed, without blanks or comments (`#` or `//` at the start of a line). */
export function scriptLines(src: string): Line[] {
  const out: Line[] = []
  src.replace(/\r\n?/g, '\n').split('\n').forEach((raw, i) => {
    const text = raw.trim()
    if (!text || text.startsWith('//') || /^#(\s|$)/.test(text)) return
    out.push({ n: i + 1, text })
  })
  return out
}

/**
 * Split a line into tokens on spaces, keeping "quoted text" together (quotes
 * removed) and `(x, y)` points together.
 */
export function tokenize(text: string, line?: number): string[] {
  const out: string[] = []
  let i = 0
  while (i < text.length) {
    const c = text[i]
    if (c === ' ' || c === '\t' || c === ',') {
      i++
    } else if (c === '"') {
      const end = text.indexOf('"', i + 1)
      if (end < 0) throw new DslError('A quoted text is missing its closing quote.', line)
      out.push(`"${text.slice(i + 1, end)}`)
      i = end + 1
    } else if (c === '(') {
      const end = text.indexOf(')', i)
      if (end < 0) throw new DslError('A point is missing its closing parenthesis.', line)
      out.push(text.slice(i, end + 1).replace(/\s+/g, ''))
      i = end + 1
    } else {
      let j = i
      while (j < text.length && !' \t,'.includes(text[j]) && text[j] !== '"') j++
      out.push(text.slice(i, j))
      i = j
    }
  }
  return out
}

/** Whether a token came from a quoted string. */
export const isQuoted = (t: string) => t.startsWith('"')
/** The text of a quoted token (or the token itself). */
export const unquote = (t: string) => (t.startsWith('"') ? t.slice(1) : t)

/** A number, with `pi`, fractions (`1/3`) and an optional SI-ish unit suffix stripped. */
export function parseNumber(tok: string, line?: number, what = 'number'): number {
  const t = tok.trim().replace(/[−–]/g, '-').replace(/^\+/, '')
  const frac = /^(-?[\d.]+(?:e-?\d+)?)\/([\d.]+)$/i.exec(t)
  if (frac) {
    const v = Number(frac[1]) / Number(frac[2])
    if (Number.isFinite(v)) return v
  }
  if (/^-?pi$/i.test(t)) return t.startsWith('-') ? -Math.PI : Math.PI
  const m = /^(-?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?)([a-zA-Zµ°Ω%/^0-9]*)$/.exec(t)
  if (!m) throw new DslError(`Expected a ${what}, got "${tok}".`, line)
  return Number(m[1])
}

/** An angle in degrees: `30`, `30deg`, `45°`, or `pi/6rad`. */
export function parseAngle(tok: string, line?: number): number {
  const t = tok.trim()
  const rad = /^(.+?)rad$/i.exec(t)
  if (rad) return (parseNumber(rad[1], line, 'angle') * 180) / Math.PI
  return parseNumber(t.replace(/°$/, ''), line, 'angle')
}

/** `(x,y)` or `x,y` as two numbers. */
export function parsePoint(tok: string, line?: number): [number, number] {
  const m = /^\(?\s*(-?[\w.+\-/]+)\s*[, ]\s*(-?[\w.+\-/]+)\s*\)?$/.exec(tok.trim())
  if (!m) throw new DslError(`Expected a point like (2, 3), got "${tok}".`, line)
  return [parseNumber(m[1], line, 'coordinate'), parseNumber(m[2], line, 'coordinate')]
}

export const isPoint = (tok: string) => /^\(.*\)$/.test(tok)

/** `key=value` tokens split from the positional ones. */
export function splitOptions(tokens: string[]): { pos: string[]; opts: Record<string, string> } {
  const pos: string[] = []
  const opts: Record<string, string> = {}
  for (const t of tokens) {
    const m = !isQuoted(t) && /^([A-Za-z][\w-]*)=(.+)$/.exec(t)
    if (m) opts[m[1].toLowerCase()] = m[2]
    else pos.push(t)
  }
  return { pos, opts }
}

/** `key: value` when a line looks like one with a known key. */
export function keyValue(text: string, known: ReadonlySet<string>): { key: string; value: string } | null {
  const m = /^([A-Za-z][\w-]*)\s*:\s*(.*)$/.exec(text)
  if (!m) return null
  const key = m[1].toLowerCase()
  return known.has(key) ? { key, value: m[2].trim() } : null
}

/** A `#rrggbb` / named colour is passed through; anything else is rejected. */
export function parseColor(tok: string, line?: number): string {
  if (/^#[0-9a-f]{3,8}$/i.test(tok) || /^[a-z]+$/i.test(tok)) return tok
  throw new DslError(`Expected a colour like #3987e5, got "${tok}".`, line)
}

/** Integers like `1-4,7,9-10` as a list. */
export function parseRanges(text: string, line?: number): number[] {
  const out: number[] = []
  for (const part of text.split(/[\s,]+/).filter(Boolean)) {
    const m = /^(\d+)(?:\s*[-–.]{1,2}\s*(\d+))?$/.exec(part)
    if (!m) throw new DslError(`Expected numbers like 1-4,7, got "${part}".`, line)
    const a = Number(m[1])
    const b = m[2] === undefined ? a : Number(m[2])
    if (b < a || b - a > 5000) throw new DslError(`The range "${part}" is backwards or too long.`, line)
    for (let i = a; i <= b; i++) out.push(i)
  }
  return out
}

/** The colours a figure is drawn in; light and dark come from the app theme. */
export interface SciColors {
  ink: string
  muted: string
  line: string
  paper: string
  dark: boolean
  accents: string[]
}
