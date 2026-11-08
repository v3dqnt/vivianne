/**
 * Turns a report-shaped markdown reply (headings, a table, lists of
 * "**Label**: detail" items) into a summary-card spec, so it reads as a card
 * whatever the model chose to write. Deterministic: no model call, and the
 * original text is one click away.
 *
 * Returns null for anything conversational, anything with a code fence, and
 * anything that already carries its own ```ui / ```widget / ```chart figure.
 */

export interface AutoCard {
  /** Plain text above the card. */
  intro: string
  /** The `ui` spec, as JSON. */
  json: string
  /** A closing question or offer, below the card. */
  outro: string
}

type B =
  | { t: 'heading'; level: number; text: string }
  | { t: 'table'; columns: string[]; rows: string[][] }
  | { t: 'list'; items: string[]; ordered: boolean }
  | { t: 'para'; text: string }

const EMOJI = /^(\p{Extended_Pictographic}️?(?:‍\p{Extended_Pictographic}️?)*)\s*/u

const plain = (s: string) => s.replace(/\*\*|__/g, '').replace(/`/g, '').trim()

function cells(line: string): string[] {
  return line
    .trim()
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
    .map((c) => c.trim())
}

/** A heading's leading emoji, read as the tone it was meant to carry. */
const TONE_OF: Record<string, 'error' | 'warning' | 'success' | 'info'> = {
  '\u{1F534}': 'error',
  '\u{274C}': 'error',
  '\u{1F7E0}': 'warning',
  '\u{1F7E1}': 'warning',
  '\u{26A0}': 'warning',
  '\u{1F7E2}': 'success',
  '\u{2705}': 'success',
  '\u{1F535}': 'info',
}

function tokenize(md: string): B[] {
  const lines = md.replace(/\r\n/g, '\n').split('\n')
  const out: B[] = []
  let para: string[] = []
  const flush = () => {
    if (para.length) out.push({ t: 'para', text: para.join(' ').trim() })
    para = []
  }
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const h = /^(#{1,4})\s+(.*\S)\s*$/.exec(line)
    if (h) {
      flush()
      out.push({ t: 'heading', level: h[1].length, text: h[2].replace(/#+\s*$/, '').trim() })
      continue
    }
    if (/^\s*\|/.test(line) && i + 1 < lines.length && /^\s*\|?[\s:|-]+\|[\s:|-]*$/.test(lines[i + 1]) && /-/.test(lines[i + 1])) {
      flush()
      const columns = cells(line)
      const rows: string[][] = []
      i += 2
      while (i < lines.length && /^\s*\|/.test(lines[i])) rows.push(cells(lines[i++]))
      i--
      out.push({ t: 'table', columns, rows })
      continue
    }
    const li = /^\s{0,3}(?:([-*•])|(\d+)[.)])\s+(.*)$/.exec(line)
    if (li) {
      flush()
      const ordered = !!li[2]
      const items = [li[3].trim()]
      while (i + 1 < lines.length) {
        const next = lines[i + 1]
        const m = /^\s{0,3}(?:([-*•])|(\d+)[.)])\s+(.*)$/.exec(next)
        if (m && !!m[2] === ordered) {
          items.push(m[3].trim())
          i++
        } else if (/^\s{2,}\S/.test(next) && !/^\s*\|/.test(next)) {
          items[items.length - 1] += ' ' + next.trim()
          i++
        } else break
      }
      out.push({ t: 'list', items, ordered })
      continue
    }
    if (!line.trim()) {
      flush()
      continue
    }
    if (/^\s*([-*_])\1{2,}\s*$/.test(line)) {
      flush()
      continue
    }
    para.push(line.trim())
  }
  flush()
  return out
}

/** "**Label**: detail" or "**Label** - detail". */
const LABELLED = /^\*\*(.+?)\*\*\s*[:–—-]?\s*(.*)$/

export function autoCard(md: string): AutoCard | null {
  if (md.length < 220 || /```/.test(md)) return null
  const blocks = tokenize(md)
  const headings = blocks.filter((b) => b.t === 'heading').length
  const tables = blocks.filter((b) => b.t === 'table').length
  const items = blocks.reduce((n, b) => n + (b.t === 'list' ? b.items.length : 0), 0)
  const reportShaped = headings >= 2 || tables >= 1 || (headings >= 1 && items >= 4) || items >= 8
  if (!reportShaped) return null

  // Plain text before the first structured block stays above the card.
  let start = 0
  const intro: string[] = []
  while (start < blocks.length && blocks[start].t === 'para') intro.push((blocks[start++] as { text: string }).text)
  // A closing question or offer stays below it.
  let end = blocks.length
  const outro: string[] = []
  const last = blocks[end - 1]
  if (end - start > 1 && last?.t === 'para' && (/\?\s*\**$/.test(last.text) || /^\**(want|should|would|let me know|shall|need)/i.test(last.text))) {
    outro.push(last.text)
    end--
  }
  const body = blocks.slice(start, end)
  if (body.length === 0) return null

  const spec: Record<string, unknown> = {}
  const out: Record<string, unknown>[] = []
  let at = 0
  const first = body[0]
  if (first.t === 'heading' && first.level <= 2) {
    const raw = plain(first.text)
    const e = EMOJI.exec(raw)
    if (e) spec.icon = e[1]
    spec.title = raw.replace(EMOJI, '').trim()
    at = 1
  }
  for (; at < body.length; at++) {
    const b = body[at]
    if (b.t === 'heading') {
      const raw = plain(b.text)
      const e = EMOJI.exec(raw)
      const tone = e ? TONE_OF[e[1].replace(/\uFE0F/g, '')] : undefined
      out.push({ type: 'heading', text: raw.replace(EMOJI, '').trim() || raw, ...(tone ? { tone } : {}) })
    } else if (b.t === 'table') {
      out.push({ type: 'table', columns: b.columns, rows: b.rows })
    } else if (b.t === 'para') {
      out.push({ type: 'text', text: b.text })
    } else {
      const labelled = b.items.map((it) => LABELLED.exec(it))
      const hits = labelled.filter((m) => m && m[2].trim()).length
      if (!b.ordered && b.items.length >= 2 && hits >= Math.ceil(b.items.length * 0.7)) {
        out.push({
          type: 'cards',
          items: b.items.map((it, i) => {
            const m = labelled[i]
            if (!m || !m[2].trim()) return { title: plain(it) }
            return { title: plain(m[1]).replace(/:$/, ''), text: m[2].trim() }
          }),
        })
      } else if (b.ordered && b.items.length >= 2) {
        out.push({ type: 'steps', items: b.items.map((it) => ({ title: it, state: 'todo' })) })
      } else {
        out.push({ type: 'list', items: b.items })
      }
    }
  }
  if (out.length === 0) return null
  spec.blocks = out
  return { intro: intro.join('\n\n'), json: JSON.stringify(spec), outro: outro.join('\n\n') }
}
