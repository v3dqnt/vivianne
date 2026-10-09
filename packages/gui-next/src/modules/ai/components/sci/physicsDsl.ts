import {
  DslError,
  isPoint,
  isQuoted,
  keyValue,
  parseAngle,
  parseColor,
  parseNumber,
  parsePoint,
  scriptLines,
  splitOptions,
  tokenize,
  unquote,
  type Line,
} from './dslCommon'

/**
 * The ```physics script: one concise line per object, compiled to a `Scene`
 * of plain primitives (polylines, text, curves) that JSXGraph then draws.
 *
 *   physics mechanics "Block on a ramp"      kinds: mechanics, optics, field,
 *   ramp 30deg "θ"                                  circuit, wave, levels
 *   block m on ramp s=4 "m"
 *   force m down 5 "mg"
 *
 * Keeping the compile step free of any drawing library is what lets
 * `check_diagram` and the unit tests run it without a browser.
 */

export type Pt = [number, number]

export interface Stroke {
  color?: string
  width?: number
  /** 0 solid, 1 dashed, 2 dotted. */
  dash?: 0 | 1 | 2
}

export type Prim =
  | ({ t: 'line'; pts: Pt[]; arrow?: 'end' | 'start' | 'both'; closed?: boolean; fill?: string; fillOpacity?: number } & Stroke)
  | { t: 'text'; at: Pt; text: string; size?: number; color?: string; anchor?: 'l' | 'c' | 'r'; v?: 't' | 'm' | 'b'; bold?: boolean }
  /** y = f(x, t) over [from, to]; redrawn as the time slider moves. */
  | ({ t: 'fcurve'; f: (x: number, t: number) => number; from: number; to: number; samples?: number } & Stroke)

export interface Scene {
  title?: string
  prims: Prim[]
  bbox: [number, number, number, number]
  /** Axes with ticks (graphs of waves); other scenes hide them. */
  axes?: boolean
  /** A time parameter the figure animates over, with its period. */
  time?: { period: number; unit: string; play: boolean }
  /** Lines the reader should know about: what the figure computed (image distance, wavelength). */
  notes: string[]
}

export type PhysicsKind = 'mechanics' | 'optics' | 'field' | 'circuit' | 'wave' | 'levels'
const KINDS: Record<string, PhysicsKind> = {
  mechanics: 'mechanics', 'free-body': 'mechanics', freebody: 'mechanics', fbd: 'mechanics', incline: 'mechanics', spring: 'mechanics', pulley: 'mechanics',
  optics: 'optics', ray: 'optics', rays: 'optics', field: 'field', fields: 'field', circuit: 'circuit', circuits: 'circuit', wave: 'wave', waves: 'wave',
  levels: 'levels', energy: 'levels', 'energy-levels': 'levels',
}

export interface Statement {
  head: string
  tokens: string[]
  line: number
}

export interface ParsedPhysics {
  kind: PhysicsKind
  title?: string
  statements: Statement[]
}

export function parsePhysics(src: string): ParsedPhysics {
  const lines = scriptLines(src)
  if (!lines.length) throw new DslError('Empty script: start with a kind — mechanics, optics, field, circuit, wave or levels.', 1)
  const h = /^(\S+)(?:\s+(.*))?$/.exec(lines[0].text)!
  const kind = KINDS[h[1].toLowerCase()]
  if (!kind) throw new DslError(`The first line names the kind of figure: mechanics, optics, field, circuit, wave or levels (got "${h[1]}").`, lines[0].n)
  let title: string | undefined
  if (h[2]) title = unquote(h[2].startsWith('"') ? tokenize(h[2], lines[0].n)[0] : h[2])
  const statements: Statement[] = []
  for (const l of lines.slice(1)) {
    const kv = keyValue(l.text, new Set(['title']))
    if (kv) {
      title = kv.value
      continue
    }
    const tokens = tokenize(l.text, l.n)
    statements.push({ head: tokens[0].toLowerCase(), tokens: tokens.slice(1), line: l.n })
  }
  return { kind, title, statements }
}

const rad = (d: number) => (d * Math.PI) / 180
const add = (a: Pt, b: Pt): Pt => [a[0] + b[0], a[1] + b[1]]
const mul = (a: Pt, k: number): Pt => [a[0] * k, a[1] * k]
const len = (a: Pt) => Math.hypot(a[0], a[1])
const unit = (a: Pt): Pt => {
  const l = len(a) || 1
  return [a[0] / l, a[1] / l]
}
const label = (toks: string[]) => toks.find(isQuoted)?.slice(1)
/** A range written `0..12` or as two numbers. */
const rangeOf = (pos: string[], line: number): [number, number] => {
  const m = /^(.+?)\.\.(.+)$/.exec(pos[0] ?? '')
  const r: [number, number] = m ? [parseNumber(m[1], line, 'start'), parseNumber(m[2], line, 'end')] : [parseNumber(pos[0], line, 'start'), parseNumber(pos[1], line, 'end')]
  if (r[0] >= r[1]) throw new DslError('A range runs from small to large: 0..12.', line)
  return r
}
const colorOf = (toks: string[], line: number) => {
  const c = toks.find((t) => /^#[0-9a-f]{3,8}$/i.test(t))
  return c ? parseColor(c, line) : undefined
}
const num = (v: string | undefined, def: number, line: number, what: string) => (v === undefined ? def : parseNumber(v, line, what))

function circlePts(c: Pt, r: number, n = 48): Pt[] {
  return Array.from({ length: n + 1 }, (_, i) => [c[0] + r * Math.cos((i / n) * Math.PI * 2), c[1] + r * Math.sin((i / n) * Math.PI * 2)] as Pt)
}

function boundsOf(prims: Prim[], pad = 0.1): [number, number, number, number] {
  let x1 = Infinity
  let y1 = Infinity
  let x2 = -Infinity
  let y2 = -Infinity
  const take = (p: Pt) => {
    x1 = Math.min(x1, p[0])
    x2 = Math.max(x2, p[0])
    y1 = Math.min(y1, p[1])
    y2 = Math.max(y2, p[1])
  }
  for (const p of prims) {
    if (p.t === 'line') p.pts.forEach(take)
    else if (p.t === 'text') take(p.at)
    else {
      take([p.from, 0])
      take([p.to, 0])
    }
  }
  if (!Number.isFinite(x1)) return [-5, -5, 5, 5]
  const w = Math.max(x2 - x1, 1)
  const h = Math.max(y2 - y1, 1)
  return [x1 - w * pad, y2 + h * pad, x2 + w * pad, y1 - h * pad]
}

// ── mechanics ───────────────────────────────────────────────────────────

interface Body {
  id: string
  kind: 'block' | 'ball' | 'pulley'
  c: Pt
  w: number
  h: number
  rot: number
}

function buildMechanics(p: ParsedPhysics): Scene {
  const prims: Prim[] = []
  const notes: string[] = []
  const bodies = new Map<string, Body>()
  let ramp: { angle: number; length: number } | null = null
  const resolve = (tok: string, line: number): { c: Pt; body?: Body } => {
    if (isPoint(tok)) return { c: parsePoint(tok, line) }
    const b = bodies.get(tok)
    if (!b) throw new DslError(`"${tok}" is not a defined object. Define it first: block ${tok} at (0, 0)`, line)
    return { c: b.c, body: b }
  }
  /** Where a line from `body` towards `to` leaves its outline. */
  const edge = (r: { c: Pt; body?: Body }, to: Pt): Pt => {
    const b = r.body
    if (!b) return r.c
    const d = unit([to[0] - r.c[0], to[1] - r.c[1]])
    if (b.kind === 'block') {
      const cs = Math.cos(-b.rot)
      const sn = Math.sin(-b.rot)
      const local: Pt = [d[0] * cs - d[1] * sn, d[0] * sn + d[1] * cs]
      const k = Math.min(local[0] ? b.w / 2 / Math.abs(local[0]) : Infinity, local[1] ? b.h / 2 / Math.abs(local[1]) : Infinity)
      return add(r.c, mul(d, k))
    }
    return add(r.c, mul(d, b.w / 2))
  }
  const dirOf = (tok: string, line: number): Pt => {
    const t = tok.toLowerCase()
    const th = ramp ? rad(ramp.angle) : 0
    const named: Record<string, Pt> = {
      up: [0, 1], down: [0, -1], left: [-1, 0], right: [1, 0], weight: [0, -1], gravity: [0, -1],
      normal: [-Math.sin(th), Math.cos(th)], 'into-slope': [Math.sin(th), -Math.cos(th)],
      'up-slope': [Math.cos(th), Math.sin(th)], 'down-slope': [-Math.cos(th), -Math.sin(th)],
    }
    if (named[t]) {
      if (/slope|normal/.test(t) && !ramp) throw new DslError(`"${tok}" needs a ramp: add "ramp 30deg" first.`, line)
      return named[t]
    }
    const a = rad(parseAngle(tok, line))
    return [Math.cos(a), Math.sin(a)]
  }
  const arrow = (from: Pt, to: Pt, color: string | undefined, text?: string, dash: 0 | 1 = 0) => {
    prims.push({ t: 'line', pts: [from, to], arrow: 'end', color, width: 2.4, dash })
    if (text) {
      const d = unit([to[0] - from[0], to[1] - from[1]])
      prims.push({ t: 'text', at: add(to, mul(d, 0.35)), text: text, color, size: 15, anchor: d[0] > 0.3 ? 'l' : d[0] < -0.3 ? 'r' : 'c', v: d[1] > 0.3 ? 'b' : d[1] < -0.3 ? 't' : 'm' })
    }
  }
  const hatch = (a: Pt, b: Pt, side: Pt) => {
    const d = unit([b[0] - a[0], b[1] - a[1]])
    const n = Math.floor(len([b[0] - a[0], b[1] - a[1]]) / 0.4)
    for (let i = 0; i <= n; i++) {
      const s = add(a, mul(d, i * 0.4))
      prims.push({ t: 'line', pts: [s, add(add(s, mul(side, 0.35)), mul(d, -0.25))], width: 1, color: '#888888' })
    }
  }
  const stmts = p.statements
  // Ramps first, so objects placed "on ramp" and slope-relative forces can find it.
  for (const s of stmts) {
    if (s.head !== 'ramp') continue
    const { pos, opts } = splitOptions(s.tokens)
    if (!pos.length) throw new DslError('ramp needs an angle: ramp 30deg', s.line)
    const angle = parseAngle(pos[0], s.line)
    if (angle <= 0 || angle >= 85) throw new DslError('A ramp angle is between 0 and 85 degrees.', s.line)
    const length = num(opts.length, 8, s.line, 'length')
    ramp = { angle, length }
    const th = rad(angle)
    const top: Pt = [length * Math.cos(th), length * Math.sin(th)]
    prims.push({ t: 'line', pts: [[0, 0], [top[0], 0], top], closed: true, fill: '#888888', fillOpacity: 0.12, width: 2 })
    prims.push({ t: 'line', pts: [[-1.2, 0], [top[0] + 1.2, 0]], width: 2 })
    hatch([-1.2, 0], [top[0] + 1.2, 0], [0, -1])
    // Angle mark at the foot of the ramp.
    const arc: Pt[] = Array.from({ length: 16 }, (_, i) => [1.7 * Math.cos((th * i) / 15), 1.7 * Math.sin((th * i) / 15)] as Pt)
    prims.push({ t: 'line', pts: arc, width: 1.4 })
    const lab = label(s.tokens)
    if (lab) prims.push({ t: 'text', at: [2.1 * Math.cos(th / 2), 2.1 * Math.sin(th / 2)], text: lab, size: 15, anchor: 'l', v: 'm' })
  }

  for (const s of stmts) {
    const { pos, opts } = splitOptions(s.tokens)
    const lab = label(s.tokens)
    const at = () => {
      const i = s.tokens.indexOf('at')
      if (i < 0 || !s.tokens[i + 1]) throw new DslError(`${s.head} needs a position: ${s.head} ${pos[0] ?? 'id'} at (0, 0)`, s.line)
      return parsePoint(s.tokens[i + 1], s.line)
    }
    switch (s.head) {
      case 'ramp':
        break
      case 'floor': {
        const y = num(opts.y ?? pos.find((x) => !isQuoted(x)), 0, s.line, 'height')
        const x1 = num(opts.x1, -6, s.line, 'x1')
        const x2 = num(opts.x2, 6, s.line, 'x2')
        prims.push({ t: 'line', pts: [[x1, y], [x2, y]], width: 2 })
        hatch([x1, y], [x2, y], [0, -1])
        break
      }
      case 'wall': {
        const x = num(opts.x ?? pos.find((t) => !isQuoted(t)), 0, s.line, 'position')
        const y1 = num(opts.y1, 0, s.line, 'y1')
        const y2 = num(opts.y2, 4, s.line, 'y2')
        prims.push({ t: 'line', pts: [[x, y1], [x, y2]], width: 2 })
        hatch([x, y1], [x, y2], [-1, 0])
        break
      }
      case 'block': {
        const id = pos[0]
        if (!id || isQuoted(id)) throw new DslError('block needs an id: block m at (0, 0) "m"', s.line)
        const [w, h] = opts.size ? opts.size.split('x').map((v) => parseNumber(v, s.line, 'size (like 2x1.2)')) : [2, 1.2]
        let c: Pt
        let rot = 0
        if (s.tokens.includes('on')) {
          if (!ramp) throw new DslError('"on ramp" needs a ramp: add "ramp 30deg" first.', s.line)
          const th = rad(ramp.angle)
          const d = num(opts.s, ramp.length / 2, s.line, 'distance along the ramp')
          c = [d * Math.cos(th) - (h / 2) * Math.sin(th), d * Math.sin(th) + (h / 2) * Math.cos(th)]
          rot = th
        } else c = at()
        if (bodies.has(id)) throw new DslError(`"${id}" is already defined.`, s.line)
        bodies.set(id, { id, kind: 'block', c, w, h, rot })
        const corners = ([[-1, -1], [1, -1], [1, 1], [-1, 1]] as Pt[]).map(([sx, sy]) => {
          const lx = (sx * w) / 2
          const ly = (sy * h) / 2
          return [c[0] + lx * Math.cos(rot) - ly * Math.sin(rot), c[1] + lx * Math.sin(rot) + ly * Math.cos(rot)] as Pt
        })
        prims.push({ t: 'line', pts: corners, closed: true, fill: colorOf(s.tokens, s.line) ?? '#3987e5', fillOpacity: 0.16, width: 2, color: colorOf(s.tokens, s.line) })
        if (lab) prims.push({ t: 'text', at: c, text: lab, size: 15, anchor: 'c', v: 'm' })
        break
      }
      case 'ball': {
        const id = pos[0]
        if (!id || isQuoted(id)) throw new DslError('ball needs an id: ball b at (0, 0) r=0.5', s.line)
        const r = num(opts.r, 0.5, s.line, 'radius')
        const c = at()
        if (bodies.has(id)) throw new DslError(`"${id}" is already defined.`, s.line)
        bodies.set(id, { id, kind: 'ball', c, w: r * 2, h: r * 2, rot: 0 })
        prims.push({ t: 'line', pts: circlePts(c, r), closed: true, fill: colorOf(s.tokens, s.line) ?? '#3987e5', fillOpacity: 0.2, width: 2, color: colorOf(s.tokens, s.line) })
        if (lab) prims.push({ t: 'text', at: c, text: lab, size: 15, anchor: 'c', v: 'm' })
        break
      }
      case 'pulley': {
        const id = pos[0]
        if (!id || isQuoted(id)) throw new DslError('pulley needs an id: pulley P at (0, 4) r=0.6', s.line)
        const r = num(opts.r, 0.6, s.line, 'radius')
        const c = at()
        bodies.set(id, { id, kind: 'pulley', c, w: r * 2, h: r * 2, rot: 0 })
        prims.push({ t: 'line', pts: circlePts(c, r), closed: true, fill: '#888888', fillOpacity: 0.15, width: 2 })
        prims.push({ t: 'line', pts: circlePts(c, 0.08, 12), closed: true, fill: '#888888', fillOpacity: 1, width: 1 })
        prims.push({ t: 'line', pts: [c, [c[0], c[1] + r + 0.8]], width: 1.4, color: '#888888' })
        if (lab) prims.push({ t: 'text', at: [c[0], c[1] + r + 1.1], text: lab, size: 14, anchor: 'c', v: 'b' })
        break
      }
      case 'spring': {
        const a = pos[0]
        const b = pos[1]
        if (!a || !b) throw new DslError('spring needs two ends (an object or a point): spring wall (0, 1) m', s.line)
        const ra = resolve(a, s.line)
        const rb = resolve(b, s.line)
        const p1 = edge(ra, rb.c)
        const p2 = edge(rb, ra.c)
        const coils = Math.round(num(opts.coils, 8, s.line, 'coils'))
        const d = unit([p2[0] - p1[0], p2[1] - p1[1]])
        const n: Pt = [-d[1], d[0]]
        const L = len([p2[0] - p1[0], p2[1] - p1[1]])
        const lead = Math.min(0.5, L * 0.15)
        const pts: Pt[] = [p1, add(p1, mul(d, lead))]
        const span = L - 2 * lead
        for (let i = 0; i < coils * 2; i++) pts.push(add(add(p1, mul(d, lead + (span * (i + 0.5)) / (coils * 2))), mul(n, i % 2 ? -0.28 : 0.28)))
        pts.push(add(p1, mul(d, L - lead)), p2)
        prims.push({ t: 'line', pts, width: 1.8 })
        if (lab) prims.push({ t: 'text', at: add(add(p1, mul(d, L / 2)), mul(n, 0.7)), text: lab, size: 14, anchor: 'c', v: 'm' })
        break
      }
      case 'rope': {
        const refs = pos.map((t) => resolve(t, s.line))
        if (refs.length < 2) throw new DslError('rope joins two or more objects or points: rope m1 P m2', s.line)
        const pts: Pt[] = []
        refs.forEach((r, i) => {
          if (r.body?.kind === 'pulley') {
            const r0 = r.body.w / 2
            const prev = refs[i - 1]?.c ?? r.c
            const left = prev[0] < r.c[0]
            const start: Pt = [r.c[0] + (left ? -r0 : r0), r.c[1]]
            const end: Pt = [r.c[0] + (left ? r0 : -r0), r.c[1]]
            pts.push(start)
            for (let k = 1; k < 12; k++) {
              const a = left ? Math.PI - (Math.PI * k) / 12 : (Math.PI * k) / 12
              pts.push([r.c[0] + r0 * Math.cos(a), r.c[1] + r0 * Math.sin(a)])
            }
            pts.push(end)
          } else {
            const toward = refs[i + 1]?.c ?? refs[i - 1]?.c ?? r.c
            const next = refs[i + 1]?.body?.kind === 'pulley' ? ([toward[0] + (r.c[0] < toward[0] ? -refs[i + 1].body!.w / 2 : refs[i + 1].body!.w / 2), toward[1]] as Pt) : toward
            const prev = refs[i - 1]?.body?.kind === 'pulley' ? ([refs[i - 1].c[0] + (r.c[0] < refs[i - 1].c[0] ? -refs[i - 1].body!.w / 2 : refs[i - 1].body!.w / 2), refs[i - 1].c[1]] as Pt) : toward
            pts.push(edge(r, i === 0 ? next : prev))
          }
        })
        prims.push({ t: 'line', pts, width: 1.6 })
        break
      }
      case 'force': {
        const target = pos[0]
        const dir = pos[1]
        const mag = pos[2]
        if (!target || !dir || !mag || isQuoted(mag)) throw new DslError('force needs a target, a direction and a size: force m down 5 "mg"', s.line)
        const r = resolve(target, s.line)
        const d = dirOf(dir, s.line)
        const m = parseNumber(mag, s.line, 'force size')
        const scale = num(opts.scale, 0.55, s.line, 'scale')
        const from = r.c
        const tip = add(from, mul(d, m * scale))
        arrow(from, tip, colorOf(s.tokens, s.line), lab, s.tokens.includes('dashed') ? 1 : 0)
        break
      }
      case 'arrow': {
        const pts = pos.filter(isPoint).map((t) => parsePoint(t, s.line))
        if (pts.length < 2) throw new DslError('arrow needs two points: arrow (0, 0) (3, 2) "v"', s.line)
        arrow(pts[0], pts[1], colorOf(s.tokens, s.line), lab, s.tokens.includes('dashed') ? 1 : 0)
        break
      }
      case 'line': {
        const pts = pos.filter(isPoint).map((t) => parsePoint(t, s.line))
        if (pts.length < 2) throw new DslError('line needs two points: line (0, 0) (3, 2) dashed', s.line)
        prims.push({ t: 'line', pts, width: 1.4, dash: s.tokens.includes('dashed') ? 1 : s.tokens.includes('dotted') ? 2 : 0, color: colorOf(s.tokens, s.line) })
        break
      }
      case 'dim': {
        const pts = pos.filter(isPoint).map((t) => parsePoint(t, s.line))
        if (pts.length < 2) throw new DslError('dim needs two points: dim (0, 0) (4, 0) "4 m"', s.line)
        prims.push({ t: 'line', pts: [pts[0], pts[1]], arrow: 'both', width: 1.2, color: '#888888' })
        if (lab) prims.push({ t: 'text', at: mul(add(pts[0], pts[1]), 0.5), text: lab, size: 14, anchor: 'c', v: 'b' })
        break
      }
      case 'label': {
        const pt = pos.find(isPoint)
        if (!pt || !lab) throw new DslError('label needs a point and a text: label (2, 3) "text"', s.line)
        prims.push({ t: 'text', at: parsePoint(pt, s.line), text: lab, size: 15, anchor: 'c', v: 'm' })
        break
      }
      case 'axes': {
        const o = pos[0] && isPoint(pos[0]) ? parsePoint(pos[0], s.line) : ([-4, -2] as Pt)
        arrow(o, add(o, [1.4, 0]), '#888888', 'x')
        arrow(o, add(o, [0, 1.4]), '#888888', 'y')
        break
      }
      case 'note':
        if (lab) notes.push(lab)
        break
      default:
        throw new DslError(`Unknown mechanics statement "${s.head}". Use ramp, floor, wall, block, ball, pulley, spring, rope, force, arrow, line, dim, label or axes.`, s.line)
    }
  }
  if (!prims.length) throw new DslError('Nothing to draw.', 1)
  return { title: p.title, prims, bbox: boundsOf(prims, 0.12), notes }
}

// ── optics ──────────────────────────────────────────────────────────────

function buildOptics(p: ParsedPhysics): Scene {
  const prims: Prim[] = []
  const notes: string[] = []
  interface Element {
    id: string
    kind: 'lens' | 'concave' | 'convex' | 'plane'
    x: number
    f: number
    h: number
    line: number
  }
  const elements: Element[] = []
  let obj: { x: number; h: number; text?: string } | null = null
  const interfaces: { x: number; n1: number; n2: number }[] = []
  const rays: { from: Pt; angle: number; color?: string; line: number }[] = []
  let principal = true
  let axisRange: [number, number] | null = null

  for (const s of p.statements) {
    const { pos, opts } = splitOptions(s.tokens)
    const lab = label(s.tokens)
    const xAt = () => {
      const i = s.tokens.indexOf('at')
      if (i < 0 || !s.tokens[i + 1]) throw new DslError(`${s.head} needs a position: ${s.head} at 0`, s.line)
      return parseNumber(s.tokens[i + 1], s.line, 'position')
    }
    switch (s.head) {
      case 'lens': {
        const f = parseNumber(opts.f ?? '', s.line, 'focal length f')
        if (!f) throw new DslError('A lens needs a non-zero focal length: lens L at 0 f=10 (negative f for a diverging lens).', s.line)
        elements.push({ id: pos[0] ?? 'L', kind: 'lens', x: xAt(), f, h: num(opts.h, 6, s.line, 'height'), line: s.line })
        break
      }
      case 'mirror': {
        const type = pos.find((t) => /^(concave|convex|plane)$/i.test(t))?.toLowerCase() as 'concave' | 'convex' | 'plane' | undefined
        if (!type) throw new DslError('A mirror is concave, convex or plane: mirror M at 0 concave f=8', s.line)
        const f = type === 'plane' ? Infinity : Math.abs(parseNumber(opts.f ?? '', s.line, 'focal length f')) * (type === 'convex' ? -1 : 1)
        elements.push({ id: pos[0] ?? 'M', kind: type, x: xAt(), f, h: num(opts.h, 6, s.line, 'height'), line: s.line })
        break
      }
      case 'object': {
        obj = { x: xAt(), h: num(opts.h, 3, s.line, 'height'), text: lab }
        break
      }
      case 'interface': {
        interfaces.push({ x: xAt(), n1: num(opts.n1, 1, s.line, 'n1'), n2: num(opts.n2, 1.5, s.line, 'n2') })
        break
      }
      case 'ray': {
        const from = pos.find(isPoint)
        if (!from) throw new DslError('ray needs a start point and an angle: ray (-8, 3) angle=20', s.line)
        rays.push({ from: parsePoint(from, s.line), angle: parseAngle(opts.angle ?? '0', s.line), color: colorOf(s.tokens, s.line), line: s.line })
        break
      }
      case 'rays':
        principal = !/^(off|none|no|false)$/i.test(pos[0] ?? 'on')
        break
      case 'axis':
        axisRange = rangeOf(pos, s.line)
        break
      case 'note':
        if (lab) notes.push(lab)
        break
      case 'screen': {
        const x = xAt()
        prims.push({ t: 'line', pts: [[x, -4], [x, 4]], width: 4, color: '#888888' })
        prims.push({ t: 'text', at: [x, 4.4], text: 'screen', size: 13, anchor: 'c', v: 'b' })
        break
      }
      default:
        throw new DslError(`Unknown optics statement "${s.head}". Use lens, mirror, object, interface, ray, rays, axis, screen or note.`, s.line)
    }
  }
  if (!elements.length && !interfaces.length) throw new DslError('Add a lens, a mirror or an interface.', 1)
  const xs = [...elements.map((e) => e.x), ...interfaces.map((i) => i.x), obj?.x ?? 0, ...rays.map((r) => r.from[0])]
  const lo = Math.min(...xs) - 12
  const hi = Math.max(...xs) + 24
  const [ax1, ax2] = axisRange ?? [lo, hi]
  prims.push({ t: 'line', pts: [[ax1, 0], [ax2, 0]], width: 1.2, color: '#888888', dash: 2 })
  const RAY = '#e66767'
  const ray = (pts: Pt[], dashed = false, color = RAY) => prims.push({ t: 'line', pts, width: 2, color, dash: dashed ? 1 : 0 })
  const arrowAt = (a: Pt, b: Pt, color: string, dash: 0 | 1 = 0) => prims.push({ t: 'line', pts: [a, b], arrow: 'end', width: 3, color, dash })
  const dot = (c: Pt, text: string, below = true) => {
    prims.push({ t: 'line', pts: circlePts(c, 0.22, 10), closed: true, fill: '#888888', fillOpacity: 1, width: 1, color: '#888888' })
    prims.push({ t: 'text', at: [c[0], c[1] + (below ? -0.5 : 0.5)], text, size: 13, anchor: 'c', v: below ? 't' : 'b' })
  }

  for (const e of elements) {
    if (e.kind === 'lens') {
      const converging = e.f > 0
      const tip = converging ? 0.45 : -0.45
      prims.push({ t: 'line', pts: [[e.x, -e.h / 2], [e.x, e.h / 2]], width: 2.4, color: '#3987e5' })
      prims.push({ t: 'line', pts: [[e.x - tip, -e.h / 2 + Math.abs(tip)], [e.x, -e.h / 2], [e.x + tip, -e.h / 2 + Math.abs(tip)]], width: 2.4, color: '#3987e5' })
      prims.push({ t: 'line', pts: [[e.x - tip, e.h / 2 - Math.abs(tip)], [e.x, e.h / 2], [e.x + tip, e.h / 2 - Math.abs(tip)]], width: 2.4, color: '#3987e5' })
      dot([e.x - Math.abs(e.f), 0], 'F')
      dot([e.x + Math.abs(e.f), 0], "F'")
    } else {
      const R = e.kind === 'plane' ? Infinity : 2 * Math.abs(e.f)
      const pts: Pt[] = []
      for (let i = 0; i <= 24; i++) {
        const y = -e.h / 2 + (e.h * i) / 24
        const sag = R === Infinity ? 0 : R - Math.sqrt(Math.max(0, R * R - y * y))
        // The reflecting side faces left: a concave mirror's edges curve towards the object.
        pts.push([e.x + (e.kind === 'concave' ? -sag : e.kind === 'convex' ? sag : 0), y])
      }
      prims.push({ t: 'line', pts, width: 2.6, color: '#3987e5' })
      for (let i = 0; i <= 10; i++) {
        const y = -e.h / 2 + (e.h * i) / 10
        prims.push({ t: 'line', pts: [[e.x + 0.02, y], [e.x + 0.5, y + 0.5]], width: 1, color: '#888888' })
      }
      if (e.kind !== 'plane') dot([e.x - e.f, 0], 'F')
    }
  }
  for (const it of interfaces) {
    prims.push({ t: 'line', pts: [[it.x, -8], [it.x, 8]], width: 2, color: '#3987e5' })
    prims.push({ t: 'text', at: [it.x - 0.6, 7.2], text: `n = ${it.n1}`, size: 13, anchor: 'r', v: 'm' })
    prims.push({ t: 'text', at: [it.x + 0.6, 7.2], text: `n = ${it.n2}`, size: 13, anchor: 'l', v: 'm' })
  }

  // The object and its image through the first element.
  const first = elements[0]
  if (obj) {
    arrowAt([obj.x, 0], [obj.x, obj.h], '#1baf7a')
    if (obj.text) prims.push({ t: 'text', at: [obj.x, obj.h + 0.5], text: obj.text, size: 14, anchor: 'c', v: 'b' })
    if (first && principal) {
      const u = first.x - obj.x
      if (u <= 0) throw new DslError('The object must be in front of (to the left of) the lens or mirror.', first.line)
      const top: Pt = [obj.x, obj.h]
      const X = first.x
      if (first.kind === 'lens') {
        const f = first.f
        const v = 1 / (1 / f - 1 / u)
        const m = -v / u
        const imgX = X + v
        const imgH = m * obj.h
        const real = v > 0
        notes.push(`Image ${real ? 'real' : 'virtual'}, ${m < 0 ? 'inverted' : 'upright'}, ${Math.abs(v).toFixed(2)} ${real ? 'beyond' : 'in front of'} the lens; magnification ${m.toFixed(2)}.`)
        const reach = hi - X
        // 1. Parallel to the axis, then through the far focus (or as if from the near one).
        const hit1: Pt = [X, obj.h]
        const slope1 = -obj.h / f
        ray([top, hit1])
        ray([hit1, [X + reach, obj.h + slope1 * reach]])
        // 2. Through the optical centre, undeviated.
        const slope2 = -obj.h / u
        ray([top, [X + reach, obj.h + slope2 * (u + reach)]])
        // 3. Aimed at the near focus, leaving parallel.
        const slope3 = -obj.h / (X - f - obj.x)
        const y3 = obj.h + slope3 * u
        ray([top, [X, y3]])
        ray([[X, y3], [X + reach, y3]])
        if (!real) {
          ray([hit1, [imgX, imgH]], true)
          ray([[X, y3], [imgX, imgH]], true)
          ray([[X, 0], [imgX, imgH]], true)
        }
        arrowAt([imgX, 0], [imgX, imgH], '#e87a2b', real ? 0 : 1)
        prims.push({ t: 'text', at: [imgX, imgH + (imgH >= 0 ? 0.5 : -0.5)], text: 'image', size: 13, anchor: 'c', v: imgH >= 0 ? 'b' : 't' })
      } else if (first.kind === 'plane') {
        notes.push(`Image virtual, upright, ${u.toFixed(2)} behind the mirror.`)
        arrowAt([X + u, 0], [X + u, obj.h], '#e87a2b', 1)
        for (const k of [0.35, 0.75]) {
          const yh = obj.h * k
          ray([top, [X, yh]])
          ray([[X, yh], [X - 18, yh + ((yh - obj.h) * 18) / u]])
          ray([[X, yh], [X + u, obj.h]], true)
        }
      } else {
        const f = first.f
        const di = 1 / (1 / f - 1 / u)
        const m = -di / u
        const imgX = X - di
        const imgH = m * obj.h
        const real = di > 0
        notes.push(`Image ${real ? 'real' : 'virtual'}, ${m < 0 ? 'inverted' : 'upright'}, ${Math.abs(di).toFixed(2)} ${real ? 'in front of' : 'behind'} the mirror; magnification ${m.toFixed(2)}.`)
        // 1. Parallel to the axis, reflects through the focus.
        const hit1: Pt = [X, obj.h]
        const s1 = obj.h / f
        ray([top, hit1])
        ray([hit1, [X - 18, obj.h - s1 * 18]])
        // 2. Towards the vertex, reflects at the same angle.
        const s2 = obj.h / u
        ray([top, [X, 0]])
        ray([[X, 0], [X - 18, s2 * 18]])
        if (!real) {
          ray([hit1, [imgX, imgH]], true)
          ray([[X, 0], [imgX, imgH]], true)
        }
        arrowAt([imgX, 0], [imgX, imgH], '#e87a2b', real ? 0 : 1)
      }
    }
  }

  // Free rays: refracted at every interface by Snell's law.
  for (const r of rays) {
    let pos: Pt = r.from
    let theta = rad(r.angle)
    const pts: Pt[] = [pos]
    const sorted = [...interfaces].sort((a, b) => a.x - b.x).filter((i) => i.x > pos[0])
    for (const it of sorted) {
      const dx = it.x - pos[0]
      const hit: Pt = [it.x, pos[1] + Math.tan(theta) * dx]
      pts.push(hit)
      const s = (Math.sin(theta) * it.n1) / it.n2
      if (Math.abs(s) > 1) {
        notes.push(`Total internal reflection at x = ${it.x}.`)
        theta = Math.PI - theta
        pts.push([hit[0] - 6, hit[1] + Math.tan(Math.PI - theta) * 6 * -1])
        pos = hit
        break
      }
      theta = Math.asin(s)
      pos = hit
    }
    pts.push([pos[0] + 12, pos[1] + Math.tan(theta) * 12])
    prims.push({ t: 'line', pts, width: 2, color: r.color ?? RAY, arrow: 'end' })
  }

  const bbox = boundsOf(prims, 0.08)
  const yMax = Math.max(Math.abs(bbox[1]), Math.abs(bbox[3]), 5)
  bbox[1] = Math.min(yMax, 14)
  bbox[3] = -Math.min(yMax, 14)
  bbox[2] = Math.min(bbox[2], hi)
  bbox[0] = Math.max(bbox[0], lo)
  return { title: p.title, prims, bbox, notes }
}

// ── field ───────────────────────────────────────────────────────────────

function buildField(p: ParsedPhysics): Scene {
  const prims: Prim[] = []
  const notes: string[] = []
  interface Charge {
    q: number
    at: Pt
  }
  const charges: Charge[] = []
  const wires: { i: number; at: Pt }[] = []
  let lines = 10
  let equi = false
  let uniform: { angle: number } | null = null
  let box: [number, number, number, number] = [-6, 4, 6, -4]
  for (const s of p.statements) {
    const { pos, opts } = splitOptions(s.tokens)
    const i = s.tokens.indexOf('at')
    const at = (): Pt => {
      if (i < 0 || !s.tokens[i + 1]) throw new DslError(`${s.head} needs a position: ${s.head} ... at (0, 0)`, s.line)
      return parsePoint(s.tokens[i + 1], s.line)
    }
    switch (s.head) {
      case 'charge': {
        const qTok = pos.find((t) => /^[+\-−]?\d*\.?\d+$|^[+\-−]$/.test(t))
        if (!qTok) throw new DslError('charge needs a signed size: charge q1 +2 at (-2, 0)', s.line)
        const q = qTok === '+' ? 1 : qTok === '-' || qTok === '−' ? -1 : parseNumber(qTok, s.line, 'charge')
        if (!q) throw new DslError('A charge of 0 has no field.', s.line)
        charges.push({ q, at: at() })
        break
      }
      case 'wire':
        wires.push({ i: /^(in|into|-)$/i.test(pos[0] ?? '') ? -1 : 1, at: at() })
        break
      case 'uniform':
        uniform = { angle: parseAngle(opts.angle ?? pos[0] ?? '0', s.line) }
        break
      case 'lines':
        lines = Math.round(parseNumber(pos[0] ?? '10', s.line, 'number of lines'))
        if (lines < 2 || lines > 40) throw new DslError('lines is 2 to 40 per unit charge.', s.line)
        break
      case 'equipotentials':
        equi = !/^(off|no|false)$/i.test(pos[0] ?? 'on')
        break
      case 'view':
        box = [parseNumber(pos[0], s.line, 'x1'), parseNumber(pos[3], s.line, 'y2'), parseNumber(pos[2], s.line, 'x2'), parseNumber(pos[1], s.line, 'y1')]
        break
      case 'note': {
        const l = label(s.tokens)
        if (l) notes.push(l)
        break
      }
      default:
        throw new DslError(`Unknown field statement "${s.head}". Use charge, wire, uniform, lines, equipotentials, view or note.`, s.line)
    }
  }
  if (!charges.length && !wires.length && !uniform) throw new DslError('Add a charge: charge q1 +1 at (0, 0).', 1)
  const xs = [...charges.map((c) => c.at[0]), ...wires.map((w) => w.at[0])]
  const ys = [...charges.map((c) => c.at[1]), ...wires.map((w) => w.at[1])]
  if (xs.length) {
    const cx = (Math.min(...xs) + Math.max(...xs)) / 2
    const cy = (Math.min(...ys) + Math.max(...ys)) / 2
    const w = Math.max(6, (Math.max(...xs) - Math.min(...xs)) / 2 + 4)
    if (!p.statements.some((s) => s.head === 'view')) box = [cx - w, cy + w * 0.7, cx + w, cy - w * 0.7]
  }
  const [bx1, by1, bx2, by2] = box
  const E = (x: number, y: number): Pt => {
    let ex = 0
    let ey = 0
    for (const c of charges) {
      const dx = x - c.at[0]
      const dy = y - c.at[1]
      const r2 = dx * dx + dy * dy + 1e-6
      const r = Math.sqrt(r2)
      ex += (c.q * dx) / (r2 * r)
      ey += (c.q * dy) / (r2 * r)
    }
    if (uniform) {
      ex += Math.cos(rad(uniform.angle)) * 0.5
      ey += Math.sin(rad(uniform.angle)) * 0.5
    }
    return [ex, ey]
  }
  const inBox = (x: number, y: number) => x > bx1 && x < bx2 && y > by2 && y < by1
  const trace = (start: Pt, sign: number): Pt[] => {
    const pts: Pt[] = [start]
    let [x, y] = start
    const h = 0.08
    for (let n = 0; n < 1600; n++) {
      const k1 = unit(E(x, y))
      const k2 = unit(E(x + (sign * h * k1[0]) / 2, y + (sign * h * k1[1]) / 2))
      const k3 = unit(E(x + (sign * h * k2[0]) / 2, y + (sign * h * k2[1]) / 2))
      const k4 = unit(E(x + sign * h * k3[0], y + sign * h * k3[1]))
      x += (sign * h * (k1[0] + 2 * k2[0] + 2 * k3[0] + k4[0])) / 6
      y += (sign * h * (k1[1] + 2 * k2[1] + 2 * k3[1] + k4[1])) / 6
      pts.push([x, y])
      if (!inBox(x, y)) break
      if (charges.some((c) => Math.hypot(x - c.at[0], y - c.at[1]) < 0.22 && (c.q > 0) !== sign > 0)) break
    }
    return pts
  }
  const drawn: { pts: Pt[]; sign: number }[] = []
  const mid = (pts: Pt[]) => {
    const half = Math.floor(pts.length / 2)
    return [pts[half], pts[Math.min(pts.length - 1, half + 2)]] as [Pt, Pt]
  }
  const sources = charges.filter((c) => c.q > 0)
  for (const c of sources.length ? sources : charges) {
    const n = Math.max(4, Math.round(lines * Math.abs(c.q)))
    for (let k = 0; k < n; k++) {
      const a = (k / n) * Math.PI * 2 + 0.05
      drawn.push({ pts: trace([c.at[0] + 0.26 * Math.cos(a), c.at[1] + 0.26 * Math.sin(a)], sources.length ? 1 : -1), sign: sources.length ? 1 : -1 })
    }
  }
  if (uniform && !charges.length) {
    for (let k = 0; k <= lines; k++) {
      const a = rad(uniform.angle)
      const t = -2 + (4 * k) / lines
      const start: Pt = [(bx1 + bx2) / 2 + -Math.sin(a) * t * 1.6 - Math.cos(a) * 5, (by1 + by2) / 2 + Math.cos(a) * t * 1.6 - Math.sin(a) * 5]
      drawn.push({ pts: trace(start, 1), sign: 1 })
    }
  }
  for (const { pts, sign } of drawn) {
    const [a, b] = mid(pts)
    prims.push({ t: 'line', pts, width: 1.6, color: '#3987e5' })
    // A small head halfway along says which way the field points.
    const d = mul(unit([b[0] - a[0], b[1] - a[1]]), sign)
    prims.push({ t: 'line', pts: [add(a, mul(d, -0.08)), add(a, mul(d, 0.12))], arrow: 'end', width: 2.4, color: '#3987e5' })
  }
  // Magnetic field of straight wires: circles around them.
  for (const w of wires) {
    for (const r of [0.9, 1.6, 2.4, 3.3]) {
      const pts = circlePts(w.at, r, 64)
      prims.push({ t: 'line', pts, width: 1.6, color: '#1baf7a' })
      const a = w.i > 0 ? Math.PI * 0.25 : Math.PI * 0.25 + Math.PI
      const base: Pt = [w.at[0] + r * Math.cos(a), w.at[1] + r * Math.sin(a)]
      // Out of the page (⊙): counter-clockwise. Into the page (⊗): clockwise.
      const d: Pt = w.i > 0 ? [-Math.sin(a), Math.cos(a)] : [Math.sin(a), -Math.cos(a)]
      prims.push({ t: 'line', pts: [add(base, mul(d, -0.08)), add(base, mul(d, 0.12))], arrow: 'end', width: 2.4, color: '#1baf7a' })
    }
    prims.push({ t: 'line', pts: circlePts(w.at, 0.35), closed: true, fill: '#1baf7a', fillOpacity: 0.2, width: 2, color: '#1baf7a' })
    prims.push({ t: 'text', at: w.at, text: w.i > 0 ? '⊙' : '⊗', size: 18, anchor: 'c', v: 'm' })
  }
  if (equi && charges.length) {
    // Potential V = sum q / r; contours by marching squares on a grid.
    const nx = 90
    const ny = 60
    const V = (x: number, y: number) => charges.reduce((a, c) => a + c.q / Math.max(0.15, Math.hypot(x - c.at[0], y - c.at[1])), 0)
    const grid: number[] = []
    for (let j = 0; j <= ny; j++) for (let i = 0; i <= nx; i++) grid.push(V(bx1 + ((bx2 - bx1) * i) / nx, by2 + ((by1 - by2) * j) / ny))
    const levels = [-4, -2, -1, -0.5, 0, 0.5, 1, 2, 4]
    for (const lv of levels) {
      for (let j = 0; j < ny; j++) {
        for (let i = 0; i < nx; i++) {
          const v = [grid[j * (nx + 1) + i], grid[j * (nx + 1) + i + 1], grid[(j + 1) * (nx + 1) + i + 1], grid[(j + 1) * (nx + 1) + i]]
          const X = (k: number) => bx1 + ((bx2 - bx1) * k) / nx
          const Y = (k: number) => by2 + ((by1 - by2) * k) / ny
          const pts: Pt[] = []
          const edge = (a: number, b: number, ax: number, ay: number, bx: number, by: number) => {
            if (v[a] - lv > 0 !== v[b] - lv > 0) {
              const t = (lv - v[a]) / (v[b] - v[a])
              pts.push([ax + (bx - ax) * t, ay + (by - ay) * t])
            }
          }
          edge(0, 1, X(i), Y(j), X(i + 1), Y(j))
          edge(1, 2, X(i + 1), Y(j), X(i + 1), Y(j + 1))
          edge(3, 2, X(i), Y(j + 1), X(i + 1), Y(j + 1))
          edge(0, 3, X(i), Y(j), X(i), Y(j + 1))
          if (pts.length >= 2) prims.push({ t: 'line', pts: [pts[0], pts[1]], width: 1, color: '#e5832b', dash: 1 })
          if (pts.length === 4) prims.push({ t: 'line', pts: [pts[2], pts[3]], width: 1, color: '#e5832b', dash: 1 })
        }
      }
    }
  }
  for (const c of charges) {
    const col = c.q > 0 ? '#e34948' : '#3987e5'
    prims.push({ t: 'line', pts: circlePts(c.at, 0.3), closed: true, fill: col, fillOpacity: 0.9, width: 1.5, color: col })
    prims.push({ t: 'text', at: c.at, text: c.q > 0 ? '+' : '−', size: 17, anchor: 'c', v: 'm', color: '#ffffff', bold: true })
  }
  return { title: p.title, prims, bbox: [bx1, by1, bx2, by2], notes }
}

// ── circuit ─────────────────────────────────────────────────────────────

const TWO_TERMINAL = new Set(['resistor', 'r', 'capacitor', 'c', 'inductor', 'l', 'battery', 'source', 'v', 'cell', 'acsource', 'ac', 'currentsource', 'switch', 's', 'diode', 'd', 'led', 'lamp', 'voltmeter', 'ammeter', 'wire'])

const circle = (p: (along: number, across?: number) => Pt, r: number, n = 32): Pt[] => Array.from({ length: n + 1 }, (_, i) => p(Math.cos((i / n) * Math.PI * 2) * r, Math.sin((i / n) * Math.PI * 2) * r))

/** One two-terminal part drawn along a→b: leads to its body, then the body. */
function symbol(kind: string, a: Pt, b: Pt, opts: { open?: boolean }): Prim[] {
  const out: Prim[] = []
  const L = len([b[0] - a[0], b[1] - a[1]])
  const d = unit([b[0] - a[0], b[1] - a[1]])
  const n: Pt = [-d[1], d[0]]
  const mid = mul(add(a, b), 0.5)
  const p = (along: number, across = 0): Pt => add(add(mid, mul(d, along)), mul(n, across))
  const line = (pts: Pt[], extra: Partial<Extract<Prim, { t: 'line' }>> = {}) => out.push({ t: 'line', pts, width: 2.2, ...extra })
  const dot = (c: Pt) => out.push({ t: 'line', pts: circlePts(c, 0.09, 12), closed: true, fill: '#888888', fillOpacity: 1, width: 1, color: '#888888' })
  // Half the length of the body, per part; the rest is lead wire.
  const body: Record<string, number> = { resistor: 0.7, r: 0.7, inductor: 0.7, l: 0.7, capacitor: 0.14, c: 0.14, battery: 0.14, source: 0.14, v: 0.14, cell: 0.14, diode: 0.4, d: 0.4, led: 0.4, switch: 0.7, s: 0.7 }
  const half = Math.min(body[kind] ?? 0.55, L * 0.45)
  line([a, p(-half)])
  line([p(half), b])
  switch (kind) {
    case 'resistor':
    case 'r': {
      const step = (half * 2) / 12
      line([p(-half), ...[1, 3, 5, 7, 9, 11].map((k, i) => p(-half + step * k, i % 2 ? -0.32 : 0.32)), p(half)])
      break
    }
    case 'inductor':
    case 'l': {
      const loops = 4
      const pts: Pt[] = []
      for (let i = 0; i <= loops * 16; i++) {
        const th = (i / 16) * Math.PI * 2
        pts.push(p(-half + (half * 2 * i) / (loops * 16) - 0.2 * Math.sin(th), 0.26 * (1 - Math.cos(th)) - 0.13))
      }
      line(pts)
      break
    }
    case 'capacitor':
    case 'c':
      line([p(-0.14, -0.55), p(-0.14, 0.55)], { width: 3 })
      line([p(0.14, -0.55), p(0.14, 0.55)], { width: 3 })
      break
    case 'battery':
    case 'source':
    case 'v':
    case 'cell':
      line([p(-0.14, -0.6), p(-0.14, 0.6)], { width: 2.6 })
      line([p(0.14, -0.3), p(0.14, 0.3)], { width: 5 })
      out.push({ t: 'text', at: p(0.55, 0.7), text: '+', size: 14, anchor: 'c', v: 'm' }, { t: 'text', at: p(-0.55, 0.7), text: '−', size: 14, anchor: 'c', v: 'm' })
      break
    case 'acsource':
    case 'ac':
      line(circle(p, 0.55), { closed: true })
      line(Array.from({ length: 21 }, (_, i) => p(-0.3 + (0.6 * i) / 20, 0.18 * Math.sin((i / 20) * Math.PI * 2))), { width: 1.8 })
      break
    case 'currentsource':
      line(circle(p, 0.55), { closed: true })
      line([p(-0.35), p(0.3)], { arrow: 'end', width: 2 })
      break
    case 'switch':
    case 's':
      line([p(-half), p(half - 0.1, opts.open ? 0.7 : 0)])
      dot(p(-half))
      dot(p(half))
      break
    case 'diode':
    case 'd':
    case 'led':
      line([p(-0.35, -0.4), p(-0.35, 0.4), p(0.35, 0)], { closed: true, fill: '#888888', fillOpacity: 0.25 })
      line([p(0.35, -0.45), p(0.35, 0.45)], { width: 2.6 })
      if (kind === 'led') {
        line([p(-0.1, 0.55), p(0.3, 1)], { arrow: 'end', width: 1.6 })
        line([p(0.15, 0.4), p(0.55, 0.85)], { arrow: 'end', width: 1.6 })
      }
      break
    case 'lamp':
      line(circle(p, 0.55), { closed: true })
      line([p(-0.39, -0.39), p(0.39, 0.39)], { width: 2 })
      line([p(-0.39, 0.39), p(0.39, -0.39)], { width: 2 })
      break
    case 'voltmeter':
    case 'ammeter':
      line(circle(p, 0.55), { closed: true })
      out.push({ t: 'text', at: mid, text: kind === 'voltmeter' ? 'V' : 'A', size: 16, anchor: 'c', v: 'm', bold: true })
      break
    default:
      // A plain wire: the leads already join the ends.
      return [{ t: 'line', pts: [a, b], width: 2.2 }]
  }
  return out
}

function buildCircuit(p: ParsedPhysics): Scene {
  const prims: Prim[] = []
  const notes: string[] = []
  const dotAt = (c: Pt) => prims.push({ t: 'line', pts: circlePts(c, 0.1, 12), closed: true, fill: '#888888', fillOpacity: 1, width: 1, color: '#888888' })
  const ends = new Map<string, number>()
  const seen = (c: Pt) => ends.set(`${c[0]},${c[1]}`, (ends.get(`${c[0]},${c[1]}`) ?? 0) + 1)
  for (const s of p.statements) {
    const lab = label(s.tokens)
    const pts = s.tokens.filter(isPoint).map((t) => parsePoint(t, s.line))
    const rest = s.tokens.filter((t) => !isQuoted(t) && !isPoint(t))
    const head = s.head
    if (head === 'wire') {
      if (pts.length < 2) throw new DslError('wire needs two or more points: wire (0, 0) (0, 4) (6, 4)', s.line)
      prims.push({ t: 'line', pts, width: 2.2 })
      pts.forEach(seen)
    } else if (TWO_TERMINAL.has(head)) {
      if (pts.length !== 2) throw new DslError(`${head} goes between two points: ${head} R1 (0, 4) (4, 4) "text"`, s.line)
      const [a, b] = pts
      if (a[0] === b[0] && a[1] === b[1]) throw new DslError('The two ends are the same point.', s.line)
      prims.push(...symbol(head, a, b, { open: rest.includes('open') }))
      seen(a)
      seen(b)
      if (lab) {
        const d = unit([b[0] - a[0], b[1] - a[1]])
        const mid = mul(add(a, b), 0.5)
        const off: Pt = Math.abs(d[0]) >= Math.abs(d[1]) ? [0, 0.95] : [0.95, 0]
        prims.push({ t: 'text', at: add(mid, off), text: lab, size: 14, anchor: Math.abs(d[0]) >= Math.abs(d[1]) ? 'c' : 'l', v: 'm' })
      }
      const id = rest[0]
      if (id && !lab && /^[A-Za-z]\w*$/.test(id) && id.toLowerCase() !== 'open' && id.toLowerCase() !== 'closed') {
        const d = unit([b[0] - a[0], b[1] - a[1]])
        const mid = mul(add(a, b), 0.5)
        const off: Pt = Math.abs(d[0]) >= Math.abs(d[1]) ? [0, 0.95] : [0.95, 0]
        prims.push({ t: 'text', at: add(mid, off), text: id, size: 14, anchor: Math.abs(d[0]) >= Math.abs(d[1]) ? 'c' : 'l', v: 'm' })
      }
    } else if (head === 'ground') {
      if (pts.length !== 1) throw new DslError('ground goes at a point: ground (0, 0)', s.line)
      const [x, y] = pts[0]
      prims.push({ t: 'line', pts: [[x, y], [x, y - 0.4]], width: 2.2 }, { t: 'line', pts: [[x - 0.45, y - 0.4], [x + 0.45, y - 0.4]], width: 2.2 }, { t: 'line', pts: [[x - 0.3, y - 0.6], [x + 0.3, y - 0.6]], width: 2.2 }, { t: 'line', pts: [[x - 0.15, y - 0.8], [x + 0.15, y - 0.8]], width: 2.2 })
      seen(pts[0])
    } else if (head === 'node') {
      for (const pt of pts) dotAt(pt)
    } else if (head === 'opamp') {
      const at = pts[0]
      if (!at) throw new DslError('opamp goes at a point: opamp U1 (6, 3)  — its inputs are at x-1.5 (y±0.5) and its output at x+1.5', s.line)
      const [x, y] = at
      const flip = rest.includes('flip')
      prims.push({ t: 'line', pts: [[x - 1.5, y + 1.1], [x + 1.5, y], [x - 1.5, y - 1.1]], closed: true, width: 2.2 })
      prims.push({ t: 'line', pts: [[x - 2.5, y + 0.5], [x - 1.5, y + 0.5]], width: 2.2 }, { t: 'line', pts: [[x - 2.5, y - 0.5], [x - 1.5, y - 0.5]], width: 2.2 }, { t: 'line', pts: [[x + 1.5, y], [x + 2.5, y]], width: 2.2 })
      prims.push({ t: 'text', at: [x - 1.2, flip ? y - 0.5 : y + 0.5], text: '+', size: 15, anchor: 'c', v: 'm' }, { t: 'text', at: [x - 1.2, flip ? y + 0.5 : y - 0.5], text: '−', size: 15, anchor: 'c', v: 'm' })
      if (lab) prims.push({ t: 'text', at: [x, y + 1.5], text: lab, size: 14, anchor: 'c', v: 'm' })
      notes.push(`Op-amp terminals: inputs at (${x - 2.5}, ${y + 0.5}) and (${x - 2.5}, ${y - 0.5}), output at (${x + 2.5}, ${y}).`)
    } else if (head === 'current') {
      const [a, b] = pts
      if (!a || !b) throw new DslError('current marks a current: current (1, 4) (2, 4) "I"', s.line)
      prims.push({ t: 'line', pts: [a, b], arrow: 'end', width: 2.4, color: '#e34948' })
      if (lab) prims.push({ t: 'text', at: add(mul(add(a, b), 0.5), [0, 0.5]), text: lab, size: 14, anchor: 'c', v: 'm', color: '#e34948' })
    } else if (head === 'label') {
      if (!pts[0] || !lab) throw new DslError('label needs a point and a text: label (2, 5) "text"', s.line)
      prims.push({ t: 'text', at: pts[0], text: lab, size: 14, anchor: 'c', v: 'm' })
    } else if (head === 'note') {
      if (lab) notes.push(lab)
    } else {
      throw new DslError(`Unknown circuit statement "${head}". Parts: wire, resistor, capacitor, inductor, battery, acsource, currentsource, switch, diode, led, lamp, voltmeter, ammeter, ground, node, opamp, current, label.`, s.line)
    }
  }
  // A dot wherever three or more ends meet.
  for (const [k, n] of ends) if (n >= 3) dotAt(k.split(',').map(Number) as Pt)
  if (!prims.length) throw new DslError('Nothing to draw.', 1)
  return { title: p.title, prims, bbox: boundsOf(prims, 0.1), notes }
}

// ── wave ────────────────────────────────────────────────────────────────

function buildWave(p: ParsedPhysics): Scene {
  const prims: Prim[] = []
  const notes: string[] = []
  interface W {
    id: string
    A: number
    lambda: number
    f: number
    phase: number
    label?: string
    color: string
  }
  const waves: W[] = []
  const palette = ['#3987e5', '#e66767', '#1baf7a', '#e5832b']
  let xr: [number, number] | null = null
  let play = true
  let annotate = false
  const sums: { ids: string[]; label?: string }[] = []
  let standing: { n: number; L: number; A: number; f: number } | null = null
  for (const s of p.statements) {
    const { pos, opts } = splitOptions(s.tokens)
    const lab = label(s.tokens)
    switch (s.head) {
      case 'wave': {
        const lambda = num(opts.lambda ?? opts.wavelength, 2, s.line, 'wavelength')
        if (lambda <= 0) throw new DslError('lambda (wavelength) must be positive.', s.line)
        waves.push({ id: pos[0] && !isQuoted(pos[0]) ? pos[0] : `w${waves.length + 1}`, A: num(opts.a ?? opts.amplitude, 1, s.line, 'amplitude'), lambda, f: num(opts.f ?? opts.freq, 0.5, s.line, 'frequency'), phase: rad(parseAngle(opts.phase ?? '0', s.line)), label: lab, color: colorOf(s.tokens, s.line) ?? palette[waves.length % palette.length] })
        break
      }
      case 'sum': {
        const ids = pos.filter((t) => !isQuoted(t))
        if (ids.length < 2) throw new DslError('sum adds two or more waves: sum w1 w2 "resultant"', s.line)
        sums.push({ ids, label: lab })
        break
      }
      case 'standing': {
        standing = { n: Math.round(num(opts.n, 2, s.line, 'mode number n')), L: num(opts.l, 6, s.line, 'length L'), A: num(opts.a, 1, s.line, 'amplitude'), f: num(opts.f, 0.5, s.line, 'frequency') }
        if (standing.n < 1 || standing.n > 12) throw new DslError('n is the number of half-wavelengths, 1 to 12.', s.line)
        break
      }
      case 'xrange':
        xr = rangeOf(pos, s.line)
        break
      case 'animate':
        play = !/^(off|no|false)$/i.test(pos[0] ?? 'on')
        break
      case 'annotate':
        annotate = true
        break
      case 'note':
        if (lab) notes.push(lab)
        break
      default:
        throw new DslError(`Unknown wave statement "${s.head}". Use wave, sum, standing, xrange, animate, annotate or note.`, s.line)
    }
  }
  if (!waves.length && !standing) throw new DslError('Add a wave: wave w1 A=1 lambda=2 f=0.5', 1)
  const ref = waves[0]
  const range: [number, number] = xr ?? (standing ? [0, standing.L] : [0, ref.lambda * 3])
  const Aref = Math.max(...waves.map((w) => w.A), standing?.A ?? 0)
  const ymax = Aref * (sums.length ? waves.length : 1) * 1.2
  const y = (w: W) => (x: number, t: number) => w.A * Math.sin(((2 * Math.PI) / w.lambda) * x - 2 * Math.PI * w.f * t + w.phase)
  for (const w of waves) {
    prims.push({ t: 'fcurve', f: y(w), from: range[0], to: range[1], width: 2.4, color: w.color, samples: 400 })
    if (w.label) prims.push({ t: 'text', at: [range[1], w.A * 0.9], text: w.label, size: 13, anchor: 'r', v: 'b', color: w.color })
  }
  for (const sm of sums) {
    const parts = sm.ids.map((id) => {
      const w = waves.find((q) => q.id === id)
      if (!w) throw new DslError(`sum names "${id}", which is not a wave (waves: ${waves.map((q) => q.id).join(', ')}).`, 1)
      return y(w)
    })
    prims.push({ t: 'fcurve', f: (x, t) => parts.reduce((a, f) => a + f(x, t), 0), from: range[0], to: range[1], width: 3, color: '#333333', samples: 500 })
    if (sm.label) prims.push({ t: 'text', at: [range[1], ymax * 0.85], text: sm.label, size: 13, anchor: 'r', v: 'b' })
  }
  if (standing) {
    const st = standing
    const lam = (2 * st.L) / st.n
    prims.push({ t: 'fcurve', f: (x, t) => 2 * st.A * Math.sin(((2 * Math.PI) / lam) * x) * Math.cos(2 * Math.PI * st.f * t), from: 0, to: st.L, width: 2.8, color: '#3987e5', samples: 400 })
    prims.push({ t: 'fcurve', f: (x) => 2 * st.A * Math.sin(((2 * Math.PI) / lam) * x), from: 0, to: st.L, width: 1, color: '#888888', dash: 2, samples: 400 })
    prims.push({ t: 'fcurve', f: (x) => -2 * st.A * Math.sin(((2 * Math.PI) / lam) * x), from: 0, to: st.L, width: 1, color: '#888888', dash: 2, samples: 400 })
    for (let k = 0; k <= st.n; k++) prims.push({ t: 'line', pts: circlePts([(k * st.L) / st.n, 0], 0.07, 10), closed: true, fill: '#e34948', fillOpacity: 1, width: 1, color: '#e34948' })
    notes.push(`Standing wave, mode ${st.n}: wavelength ${lam.toFixed(2)}, ${st.n + 1} nodes (red) including the fixed ends.`)
  }
  if (annotate && ref) {
    // Wavelength between two crests and amplitude from the axis.
    const x0 = (ref.lambda / 4 + (ref.phase ? 0 : 0)) % ref.lambda
    prims.push({ t: 'line', pts: [[x0, -ref.A * 1.15], [x0 + ref.lambda, -ref.A * 1.15]], arrow: 'both', width: 1.4, color: '#888888' })
    prims.push({ t: 'text', at: [x0 + ref.lambda / 2, -ref.A * 1.2], text: `λ = ${ref.lambda}`, size: 13, anchor: 'c', v: 't' })
    prims.push({ t: 'line', pts: [[range[0] + 0.05, 0], [range[0] + 0.05, ref.A]], arrow: 'both', width: 1.4, color: '#888888' })
    prims.push({ t: 'text', at: [range[0] + 0.15, ref.A / 2], text: `A = ${ref.A}`, size: 13, anchor: 'l', v: 'm' })
    notes.push(`v = λf = ${(ref.lambda * ref.f).toPrecision(3)}, period T = 1/f = ${(1 / ref.f).toPrecision(3)}.`)
  }
  const period = standing ? 1 / standing.f : 1 / (ref?.f || 1)
  return { title: p.title, prims, bbox: [range[0] - 0.05 * (range[1] - range[0]), ymax, range[1] + 0.05 * (range[1] - range[0]), -ymax], axes: true, time: { period, unit: 's', play }, notes }
}

// ── energy levels ───────────────────────────────────────────────────────

function buildLevels(p: ParsedPhysics): Scene {
  const prims: Prim[] = []
  const notes: string[] = []
  interface Lv {
    key: string
    E: number
    label?: string
  }
  const levels: Lv[] = []
  let unitName = 'eV'
  const transitions: { from: string; to: string; label?: string; line: number }[] = []
  for (const s of p.statements) {
    const { pos, opts } = splitOptions(s.tokens)
    const lab = label(s.tokens)
    switch (s.head) {
      case 'unit':
        unitName = pos[0] ?? 'eV'
        break
      case 'level': {
        const E = parseNumber(opts.e ?? '', s.line, 'energy E')
        const key = opts.n ?? pos[0] ?? String(levels.length + 1)
        if (levels.some((l) => l.key === key)) throw new DslError(`Level "${key}" is defined twice.`, s.line)
        levels.push({ key, E, label: lab })
        break
      }
      case 'hydrogen': {
        const m = /^(\d+)\.\.(\d+)$/.exec(opts.n ?? '1..5')
        if (!m) throw new DslError('hydrogen takes a range of n: hydrogen n=1..5', s.line)
        for (let n = Number(m[1]); n <= Number(m[2]); n++) levels.push({ key: String(n), E: -13.6 / (n * n), label: `n = ${n}` })
        unitName = 'eV'
        break
      }
      case 'transition': {
        const arrow = s.tokens.findIndex((t) => t === '->' || t === '→')
        if (arrow < 1) throw new DslError('transition goes from one level to another: transition 3 -> 2 "Hα"', s.line)
        transitions.push({ from: s.tokens[arrow - 1], to: s.tokens[arrow + 1], label: lab, line: s.line })
        break
      }
      case 'note':
        if (lab) notes.push(lab)
        break
      default:
        throw new DslError(`Unknown levels statement "${s.head}". Use level, hydrogen, transition, unit or note.`, s.line)
    }
  }
  if (levels.length < 2) throw new DslError('Add two or more levels: level n=1 E=-13.6', 1)
  const Emin = Math.min(...levels.map((l) => l.E))
  const Emax = Math.max(...levels.map((l) => l.E))
  const span = Emax - Emin || 1
  const find = (k: string, line: number) => {
    const l = levels.find((x) => x.key === k)
    if (!l) throw new DslError(`There is no level "${k}" (levels: ${levels.map((x) => x.key).join(', ')}).`, line)
    return l
  }
  const W = 6
  // Levels crowd together near the limit, so label the energies in a column on the right.
  for (const l of levels) {
    prims.push({ t: 'line', pts: [[0, l.E], [W, l.E]], width: 2.4 })
    prims.push({ t: 'text', at: [-0.2, l.E], text: l.label ?? l.key, size: 14, anchor: 'r', v: 'm' })
    prims.push({ t: 'text', at: [W + 0.2, l.E], text: `${Number(l.E.toPrecision(4))} ${unitName}`, size: 13, anchor: 'l', v: 'm', color: '#888888' })
  }
  transitions.forEach((t, i) => {
    const a = find(t.from, t.line)
    const b = find(t.to, t.line)
    if (a.E === b.E) throw new DslError('A transition needs two different energies.', t.line)
    const x = ((i + 1) * W) / (transitions.length + 1)
    const emit = a.E > b.E
    const pts: Pt[] = []
    const n = 40
    // Photons are drawn as a wavy line; the arrow shows the electron's jump.
    for (let k = 0; k <= n; k++) {
      const y = a.E + ((b.E - a.E) * k) / n
      pts.push([x + (emit ? 0.12 * Math.sin((k / n) * Math.PI * 10) : 0), y])
    }
    prims.push({ t: 'line', pts: emit ? pts : [[x, a.E], [x, b.E]], arrow: 'end', width: 2.4, color: emit ? '#e66767' : '#3987e5' })
    const dE = Math.abs(a.E - b.E)
    const bits = [t.label, `ΔE = ${Number(dE.toPrecision(3))} ${unitName}`]
    if (unitName === 'eV') bits.push(`λ = ${(1239.84 / dE).toFixed(1)} nm`)
    const mid = (a.E + b.E) / 2
    prims.push({ t: 'text', at: [x + 0.2, mid], text: bits.filter(Boolean).join(' · '), size: 12, anchor: 'l', v: 'm', color: emit ? '#e66767' : '#3987e5' })
    notes.push(`${emit ? 'Emission' : 'Absorption'} ${t.from} → ${t.to}: ΔE = ${Number(dE.toPrecision(3))} ${unitName}${unitName === 'eV' ? `, λ = ${(1239.84 / dE).toFixed(1)} nm` : ''}.`)
  })
  return { title: p.title, prims, bbox: [-2.2, Emax + span * 0.1, W + 3.2, Emin - span * 0.08], notes }
}

/** Compile a ```physics script to the primitives a figure is drawn from. */
export function compilePhysics(src: string): Scene {
  const p = parsePhysics(src)
  let scene: Scene
  switch (p.kind) {
    case 'mechanics':
      scene = buildMechanics(p)
      break
    case 'optics':
      scene = buildOptics(p)
      break
    case 'field':
      scene = buildField(p)
      break
    case 'circuit':
      scene = buildCircuit(p)
      break
    case 'wave':
      scene = buildWave(p)
      break
    default:
      scene = buildLevels(p)
  }
  return scene
}

export type { Line }
