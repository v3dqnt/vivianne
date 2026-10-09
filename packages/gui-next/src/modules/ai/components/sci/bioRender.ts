import { CHART_SERIES } from '../../lib/chartPalette'
import { leavesOf, resolveColor, type Feature, type NewickNode, type PlasmidSpec, type SequenceSpec, type TreeSpec } from './bioDsl'
import type { SciColors } from './dslCommon'
import { escapeHtml, svgText } from './symbols'

/**
 * Sequence maps, plasmid maps and phylogenies as plain SVG strings. Each
 * function returns a complete `<svg>` that sizes itself from its content.
 */

const FONT = 'system-ui, -apple-system, "Segoe UI", sans-serif'
const MONO = '"Geist Mono", ui-monospace, "SFMono-Regular", Menlo, Consolas, monospace'

function wrap(w: number, h: number, body: string, c: SciColors): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" class="sci-svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" font-family="${FONT}" color="${c.ink}">${body}</svg>`
}

const series = (c: SciColors) => CHART_SERIES[c.dark ? 'dark' : 'light']
const num = (n: number) => Math.round(n * 100) / 100

// ── Sequence ────────────────────────────────────────────────────────────

const CODONS: Record<string, string> = {}
{
  const bases = 'TCAG'
  const aa = 'FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG'
  let i = 0
  for (const a of bases) for (const b of bases) for (const d of bases) CODONS[a + b + d] = aa[i++]
}

const COMPLEMENT: Record<string, string> = { A: 'T', T: 'A', U: 'A', G: 'C', C: 'G', N: 'N', R: 'Y', Y: 'R', K: 'M', M: 'K', S: 'S', W: 'W', B: 'V', V: 'B', D: 'H', H: 'D', '-': '-' }

export function reverseComplement(s: string): string {
  let out = ''
  for (let i = s.length - 1; i >= 0; i--) out += COMPLEMENT[s[i]] ?? 'N'
  return out
}

/** Translate from `offset`; returns one letter per complete codon (`?` for an ambiguous one). */
export function translate(dna: string, offset = 0): string {
  const rna = dna.replace(/U/g, 'T')
  let out = ''
  for (let i = offset; i + 3 <= rna.length; i += 3) out += CODONS[rna.slice(i, i + 3)] ?? '?'
  return out
}

const BASE_COLORS = { A: '#1baf7a', C: '#3987e5', G: '#d99100', T: '#e34948', U: '#e34948' } as Record<string, string>
const AA_CLASS: Record<string, string> = {}
for (const a of 'AVLIMFWC') AA_CLASS[a] = '#d99100'
for (const a of 'STNQYGP') AA_CLASS[a] = '#14a3a3'
for (const a of 'KRH') AA_CLASS[a] = '#3987e5'
for (const a of 'DE') AA_CLASS[a] = '#e34948'

export function sequenceSvg(spec: SequenceSpec, c: SciColors): string {
  const n = spec.seq.length
  const cw = 9.4
  const gap = 7
  const gutter = spec.numbers ? 54 : 14
  const W = spec.width
  const xOf = (col: number) => gutter + col * cw + Math.floor(col / 10) * gap
  const rowCount = Math.ceil(n / W)
  const rowW = xOf(W) + 14

  // Features: each keeps one lane for its whole length.
  const order = [...spec.features].sort((a, b) => a.start - b.start || b.end - a.end)
  const laneEnd: number[] = []
  const lane = new Map<Feature, number>()
  for (const f of order) {
    let l = laneEnd.findIndex((e) => e < f.start)
    if (l < 0) l = laneEnd.length
    laneEnd[l] = f.end
    lane.set(f, l)
  }
  const palette = series(c)
  const colorOf = (f: Feature, i: number) => resolveColor(f.color, palette[i % palette.length])

  const frames = spec.translate
  const out: string[] = []
  let y = 12
  if (spec.title) {
    out.push(svgText(rowW / 2, 18, spec.title, { size: 13.5, weight: 600, fill: c.ink }))
    y += 26
  }
  for (let r = 0; r < rowCount; r++) {
    const from = r * W
    const to = Math.min(n, from + W)
    const cols = to - from
    const row = spec.seq.slice(from, to)
    const rowFeatures = order.filter((f) => f.start - 1 < to && f.end > from)
    const lanes = rowFeatures.length ? Math.max(...rowFeatures.map((f) => lane.get(f) ?? 0)) + 1 : 0
    const seqY = y + 13
    // Letters, with one x per glyph so the grid holds whatever the font.
    const xs = [...row].map((_, i) => num(xOf(i) + cw / 2))
    const colored = spec.color === 'base' && spec.type !== 'protein'
    if (spec.type === 'protein' && spec.color === 'base') {
      ;[...row].forEach((ch, i) => out.push(`<text x="${xs[i]}" y="${seqY}" text-anchor="middle" font-family='${MONO}' font-size="13.5" font-weight="600" fill="${AA_CLASS[ch] ?? c.ink}">${escapeHtml(ch)}</text>`))
    } else if (colored) {
      ;[...row].forEach((ch, i) => out.push(`<text x="${xs[i]}" y="${seqY}" text-anchor="middle" font-family='${MONO}' font-size="13.5" font-weight="600" fill="${BASE_COLORS[ch] ?? c.ink}">${escapeHtml(ch)}</text>`))
    } else {
      out.push(`<text x="${xs.join(' ')}" y="${seqY}" text-anchor="middle" font-family='${MONO}' font-size="13.5" fill="${c.ink}">${escapeHtml(row)}</text>`)
    }
    if (spec.numbers) {
      out.push(`<text x="${gutter - 10}" y="${seqY}" text-anchor="end" font-size="10.5" fill="${c.muted}">${from + 1}</text>`)
      out.push(`<text x="${xOf(cols - 1) + cw + 6}" y="${seqY}" font-size="10.5" fill="${c.muted}" opacity=".7">${to}</text>`)
    }
    let cy = seqY + 4
    if (spec.complement && spec.type !== 'protein') {
      const comp = [...row].map((ch) => COMPLEMENT[ch] ?? 'N').join('')
      out.push(`<text x="${xs.join(' ')}" y="${cy + 12}" text-anchor="middle" font-family='${MONO}' font-size="13.5" fill="${c.muted}">${escapeHtml(comp)}</text>`)
      cy += 17
    }
    // Feature bars.
    if (lanes) {
      for (const f of rowFeatures) {
        const i0 = Math.max(f.start - 1, from) - from
        const i1 = Math.min(f.end, to) - from
        const x0 = xOf(i0)
        const x1 = xOf(i1 - 1) + cw
        const ly = cy + 4 + (lane.get(f) ?? 0) * 17
        const col = colorOf(f, order.indexOf(f))
        out.push(`<rect x="${num(x0)}" y="${ly}" width="${num(x1 - x0)}" height="13" rx="3" fill="${col}" opacity=".9"><title>${escapeHtml(f.label)} ${f.start}-${f.end}</title></rect>`)
        const maxChars = Math.floor((x1 - x0 - 6) / 6.2)
        // The label goes on the widest piece of the feature, once.
        const wholeIn = f.start - 1 >= from && f.end <= to
        if (maxChars >= 3 && (wholeIn || i1 - i0 >= Math.min(f.end - f.start + 1, W) * 0.5)) {
          const text = f.label.length > maxChars ? `${f.label.slice(0, Math.max(1, maxChars - 1))}…` : f.label
          out.push(`<text x="${num((x0 + x1) / 2)}" y="${ly + 10}" text-anchor="middle" font-size="10" font-weight="600" fill="#fff">${escapeHtml(text)}</text>`)
        }
      }
      cy += 8 + lanes * 17
    }
    // Translation lanes, one per frame; each residue sits under its middle base.
    for (const frame of frames) {
      const rc = frame < 0
      const src = rc ? reverseComplement(spec.seq) : spec.seq
      const offset = Math.abs(frame) - 1
      const prot = translate(src, offset)
      const parts: string[] = []
      for (let k = 0; k < prot.length; k++) {
        const mid = rc ? n - 1 - (offset + k * 3 + 1) : offset + k * 3 + 1
        if (mid < from || mid >= to) continue
        const ch = prot[k]
        const col = ch === '*' ? '#e34948' : ch === 'M' ? '#1baf7a' : c.ink
        parts.push(`<text x="${num(xOf(mid - from) + cw / 2)}" y="${cy + 12}" text-anchor="middle" font-family='${MONO}' font-size="12" font-weight="${ch === '*' || ch === 'M' ? 700 : 400}" fill="${col}">${escapeHtml(ch)}</text>`)
      }
      out.push(`<text x="${gutter - 10}" y="${cy + 12}" text-anchor="end" font-size="9.5" fill="${c.muted}">${frame > 0 ? '+' : '−'}${Math.abs(frame)}</text>${parts.join('')}`)
      cy += 16
    }
    y = cy + 14
  }
  return wrap(rowW + 20, y + 4, out.join(''), c)
}

// ── Plasmid ─────────────────────────────────────────────────────────────

interface Label {
  angle: number
  text: string
  color: string
  weight: number
  sub?: string
}

/** Spread labels down one side of the plasmid so they never overlap. */
function spreadLabels(labels: (Label & { y: number })[], minY: number, maxY: number, gap: number) {
  labels.sort((a, b) => a.y - b.y)
  for (let i = 0; i < labels.length; i++) labels[i].y = Math.max(labels[i].y, i ? labels[i - 1].y + gap : minY)
  for (let i = labels.length - 1; i >= 0; i--) {
    const limit = i === labels.length - 1 ? maxY : labels[i + 1].y - gap
    if (labels[i].y > limit) labels[i].y = limit
  }
}

export function plasmidSvg(spec: PlasmidSpec, c: SciColors): string {
  const R = 130
  const band = 15
  const W = 640
  const cx = W / 2
  const top = 40
  const cy = top + R + 70
  const H = cy + R + 64
  const ang = (pos: number, end = false) => ((end ? pos : pos - 1) / spec.size) * Math.PI * 2 - Math.PI / 2
  const pt = (r: number, a: number) => [cx + r * Math.cos(a), cy + r * Math.sin(a)] as const
  const palette = series(c)
  const out: string[] = []
  out.push(svgText(cx, 22, spec.title, { size: 14, weight: 600, fill: c.ink }))
  out.push(`<circle cx="${cx}" cy="${cy}" r="${R}" fill="none" stroke="${c.muted}" stroke-width="2.4" opacity=".7"/>`)
  out.push(svgText(cx, cy - 4, spec.title, { size: 15, weight: 600, fill: c.ink }))
  out.push(svgText(cx, cy + 16, `${spec.size.toLocaleString('en-US')} bp`, { size: 12, fill: c.muted }))

  const labels: (Label & { y: number })[] = []
  spec.features.forEach((f, i) => {
    const color = resolveColor(f.color, palette[i % palette.length])
    let a0 = ang(f.start)
    let a1 = ang(f.end, true)
    if (f.end < f.start) a1 += Math.PI * 2
    const span = a1 - a0
    const dir = f.dir ?? 'cw'
    const tip = dir === 'none' ? 0 : Math.min(span * 0.4, 0.13)
    const r1 = R - band / 2
    const r2 = R + band / 2
    const rm = R
    const big = span > Math.PI ? 1 : 0
    const P = (r: number, a: number) => pt(r, a).map(num).join(' ')
    let d: string
    if (dir === 'ccw') {
      d = `M ${P(r2, a0 + tip)} A ${r2} ${r2} 0 ${big} 1 ${P(r2, a1)} L ${P(r1, a1)} A ${r1} ${r1} 0 ${big} 0 ${P(r1, a0 + tip)} L ${P(rm, a0)} Z`
    } else {
      d = `M ${P(r2, a0)} A ${r2} ${r2} 0 ${big} 1 ${P(r2, a1 - tip)} L ${P(rm, a1)} L ${P(r1, a1 - tip)} A ${r1} ${r1} 0 ${big} 0 ${P(r1, a0)} Z`
    }
    out.push(`<path d="${d}" fill="${color}" opacity=".92"><title>${escapeHtml(f.label)} ${f.start}..${f.end}</title></path>`)
    a0 = (a0 + a1) / 2
    labels.push({ angle: a0, text: f.label, sub: `${f.start}..${f.end}`, color, weight: 600, y: 0 })
  })
  for (const s of spec.sites) {
    const a = ang(s.pos)
    const [x0, y0] = pt(R - band / 2 - 4, a)
    const [x1, y1] = pt(R + band / 2 + 4, a)
    out.push(`<line x1="${num(x0)}" y1="${num(y0)}" x2="${num(x1)}" y2="${num(y1)}" stroke="${c.ink}" stroke-width="1.6"/>`)
    labels.push({ angle: a, text: s.name, sub: String(s.pos), color: c.ink, weight: 500, y: 0 })
  }
  // Origin tick.
  const [ox0, oy0] = pt(R - 9, -Math.PI / 2)
  const [ox1, oy1] = pt(R + 9, -Math.PI / 2)
  out.push(`<line x1="${num(ox0)}" y1="${num(oy0)}" x2="${num(ox1)}" y2="${num(oy1)}" stroke="${c.muted}" stroke-width="1"/>`)

  // Labels hang off both sides on leader lines.
  for (const side of [1, -1]) {
    const mine = labels.filter((l) => (Math.cos(l.angle) >= 0 ? 1 : -1) === side).map((l) => ({ ...l, y: cy + (R + 22) * Math.sin(l.angle) }))
    spreadLabels(mine, top + 8, H - 28, 25)
    for (const l of mine) {
      const [px, py] = pt(R + band / 2 + 6, l.angle)
      const ex = cx + side * (R + 38)
      const tx = cx + side * (R + 46)
      out.push(`<polyline points="${num(px)},${num(py)} ${num(ex - side * 6)},${num(l.y - 3)} ${num(tx - side * 4)},${num(l.y - 3)}" fill="none" stroke="${c.muted}" stroke-width=".8" opacity=".7"/>`)
      out.push(svgText(tx, l.y, l.text, { size: 11.5, anchor: side === 1 ? 'start' : 'end', weight: l.weight, fill: l.color === c.ink ? c.ink : l.color }))
      if (l.sub) out.push(`<text x="${tx}" y="${l.y + 11}" text-anchor="${side === 1 ? 'start' : 'end'}" font-size="9.5" fill="${c.muted}">${escapeHtml(l.sub)}</text>`)
    }
  }
  return wrap(W, H, out.join(''), c)
}

// ── Tree ────────────────────────────────────────────────────────────────

interface Placed {
  node: NewickNode
  x: number
  y: number
  kids: Placed[]
  leafIndex?: number
}

function nice(v: number): number {
  const p = 10 ** Math.floor(Math.log10(v))
  const f = v / p
  return (f >= 5 ? 5 : f >= 2 ? 2 : 1) * p
}

/** The smallest clade containing every named taxon. */
function mrca(root: Placed, taxa: Set<string>): Placed | null {
  const has = (p: Placed): number => (p.kids.length ? p.kids.reduce((s, k) => s + has(k), 0) : taxa.has(p.node.name ?? '') ? 1 : 0)
  let cur = root
  if (has(cur) < taxa.size) return null
  for (;;) {
    const next = cur.kids.find((k) => has(k) === taxa.size)
    if (!next) return cur
    cur = next
  }
}

export function treeSvg(spec: TreeSpec, c: SciColors): string {
  const leaves = leavesOf(spec.root)
  const hasLengths = spec.layout !== 'cladogram' && leavesOf(spec.root).some((l) => l.length !== undefined)
  const palette = series(c)
  const hlColor = (i: number, h: TreeSpec['highlights'][number]) => resolveColor(h.color, palette[(i + 1) % palette.length])

  // Depth of every node: summed branch lengths, or levels for a cladogram.
  const place = (n: NewickNode, depth: number): Placed => ({ node: n, x: depth, y: 0, kids: n.children.map((k) => place(k, depth + (hasLengths ? (k.length ?? 0) : 1))) })
  const root = place(spec.root, 0)
  let li = 0
  const heightOf = (p: Placed): number => (p.kids.length ? 1 + Math.max(...p.kids.map(heightOf)) : 0)
  const rootH = heightOf(root)
  const assignY = (p: Placed) => {
    if (!p.kids.length) {
      p.leafIndex = li
      p.y = li++
    } else {
      p.kids.forEach(assignY)
      p.y = p.kids.reduce((s, k) => s + k.y, 0) / p.kids.length
    }
  }
  assignY(root)
  // A cladogram lines every tip up on the right.
  if (!hasLengths) {
    const fix = (p: Placed) => {
      p.x = (rootH - heightOf(p)) / (rootH || 1)
      p.kids.forEach(fix)
    }
    fix(root)
  }
  const maxX = Math.max(...flat(root).map((p) => p.x), 1e-9)
  const norm = (v: number) => v / maxX
  const labelW = Math.max(...leaves.map((l) => (l.name ?? '').length)) * 6.6 + 12
  const out: string[] = []
  const clades = spec.highlights.map((h, i) => ({ h, color: hlColor(i, h), at: mrca(root, new Set(h.taxa)) }))
  const colorOfLeaf = (p: Placed) => clades.find((cl) => cl.at && flat(cl.at).includes(p))?.color
  const titleH = spec.title ? 30 : 6
  let width: number
  let height: number

  if (spec.layout === 'circular') {
    const R = 150
    const margin = labelW + 24
    const cx = R + margin
    const cy = R + margin + titleH - 10
    width = cx * 2
    height = cy + R + margin
    const ang = (p: Placed) => (p.y / Math.max(1, leaves.length)) * Math.PI * 2
    const pos = (p: Placed, a = ang(p)) => [cx + norm(p.x) * R * Math.cos(a), cy + norm(p.x) * R * Math.sin(a)] as const
    const draw = (p: Placed, color: string) => {
      for (const k of p.kids) {
        const kc = clades.find((cl) => cl.at === k)?.color ?? color
        const [bx, by] = pos(k, ang(k))
        const r = norm(p.x) * R
        const [ax, ay] = [cx + r * Math.cos(ang(k)), cy + r * Math.sin(ang(k))]
        out.push(`<line x1="${num(ax)}" y1="${num(ay)}" x2="${num(bx)}" y2="${num(by)}" stroke="${kc}" stroke-width="1.6" stroke-linecap="round"/>`)
        draw(k, kc)
      }
      if (p.kids.length) {
        const as = p.kids.map(ang)
        const a0 = Math.min(...as)
        const a1 = Math.max(...as)
        const r = norm(p.x) * R
        const s = [cx + r * Math.cos(a0), cy + r * Math.sin(a0)]
        const e = [cx + r * Math.cos(a1), cy + r * Math.sin(a1)]
        out.push(`<path d="M ${num(s[0])} ${num(s[1])} A ${num(r)} ${num(r)} 0 ${a1 - a0 > Math.PI ? 1 : 0} 1 ${num(e[0])} ${num(e[1])}" fill="none" stroke="${color}" stroke-width="1.6"/>`)
      }
    }
    draw(root, c.ink)
    for (const p of flat(root).filter((q) => !q.kids.length)) {
      const a = ang(p)
      const r = R + 8
      const flip = Math.cos(a) < 0
      const deg = (a * 180) / Math.PI + (flip ? 180 : 0)
      out.push(`<text transform="translate(${num(cx + r * Math.cos(a))} ${num(cy + r * Math.sin(a))}) rotate(${num(deg)})" text-anchor="${flip ? 'end' : 'start'}" dominant-baseline="middle" font-size="11.5" fill="${colorOfLeaf(p) ?? c.ink}">${escapeHtml(p.node.name ?? '')}</text>`)
    }
  } else {
    const rowH = 21
    const treeW = 330
    const left = 16
    const rightPad = clades.some((cl) => cl.h.label) ? Math.max(...clades.map((cl) => cl.h.label.length)) * 6.4 + 22 : 8
    const X = (p: Placed) => left + norm(p.x) * treeW
    const Y = (p: Placed) => titleH + 10 + p.y * rowH
    width = left + treeW + labelW + rightPad
    height = titleH + 20 + leaves.length * rowH + (spec.scale && hasLengths ? 34 : 6)
    for (const cl of clades) {
      if (!cl.at) continue
      const ys = flat(cl.at).filter((q) => !q.kids.length).map(Y)
      const x0 = X(cl.at) - 5
      out.push(`<rect x="${num(x0)}" y="${num(Math.min(...ys) - rowH / 2 + 1)}" width="${num(width - rightPad - x0 + 2)}" height="${num(Math.max(...ys) - Math.min(...ys) + rowH - 2)}" rx="5" fill="${cl.color}" opacity=".13"/>`)
      if (cl.h.label) out.push(`<text x="${num(width - rightPad + 8)}" y="${num((Math.min(...ys) + Math.max(...ys)) / 2 + 4)}" font-size="11" font-weight="600" fill="${cl.color}">${escapeHtml(cl.h.label)}</text>`)
    }
    const draw = (p: Placed) => {
      if (p.kids.length) {
        const ys = p.kids.map(Y)
        out.push(`<line x1="${num(X(p))}" y1="${num(Math.min(...ys))}" x2="${num(X(p))}" y2="${num(Math.max(...ys))}" stroke="${c.ink}" stroke-width="1.5"/>`)
      }
      for (const k of p.kids) {
        out.push(`<line x1="${num(X(p))}" y1="${num(Y(k))}" x2="${num(X(k))}" y2="${num(Y(k))}" stroke="${c.ink}" stroke-width="1.5"/>`)
        draw(k)
      }
      if (!p.kids.length) {
        out.push(`<text x="${num(X(p) + 6)}" y="${num(Y(p) + 4)}" font-size="12" fill="${colorOfLeaf(p) ?? c.ink}">${escapeHtml(p.node.name ?? '')}</text>`)
      } else if (p.node.name) {
        out.push(`<text x="${num(X(p) - 4)}" y="${num(Y(p) - 4)}" text-anchor="end" font-size="9.5" fill="${c.muted}">${escapeHtml(p.node.name)}</text>`)
      }
    }
    draw(root)
    if (spec.scale && hasLengths) {
      const unit = nice(maxX * 0.2)
      const w = (unit / maxX) * treeW
      const y = height - 14
      out.push(`<line x1="${left}" y1="${y}" x2="${num(left + w)}" y2="${y}" stroke="${c.ink}" stroke-width="1.5"/><line x1="${left}" y1="${y - 4}" x2="${left}" y2="${y + 4}" stroke="${c.ink}"/><line x1="${num(left + w)}" y1="${y - 4}" x2="${num(left + w)}" y2="${y + 4}" stroke="${c.ink}"/>`)
      out.push(`<text x="${num(left + w / 2)}" y="${y + 14}" text-anchor="middle" font-size="10" fill="${c.muted}">${unit}</text>`)
    }
  }
  if (spec.title) out.unshift(svgText(width / 2, 20, spec.title, { size: 13.5, weight: 600, fill: c.ink }))
  return wrap(Math.ceil(width), Math.ceil(height), out.join(''), c)
}

function flat(p: Placed): Placed[] {
  return [p, ...p.kids.flatMap(flat)]
}
