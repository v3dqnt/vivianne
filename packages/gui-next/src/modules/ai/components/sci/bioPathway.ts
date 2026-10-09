import type { PathwayEdge, PathwayNode, PathwaySpec } from './bioDsl'
import type { SciColors } from './dslCommon'
import { escapeHtml, plainLabel, svgText } from './symbols'

/**
 * Pathway diagrams in the style of SBGN process descriptions: gene, protein,
 * metabolite and complex nodes, compartments around them, small process
 * squares for conversions, and stimulation / inhibition / catalysis arcs.
 * elkjs lays it out; the drawing is plain SVG.
 */

interface Box {
  id: string
  x: number
  y: number
  w: number
  h: number
}
export interface PlacedNode extends Box {
  node?: PathwayNode
  process?: boolean
}
export interface PlacedEdge {
  kind: 'production' | 'consumption' | 'stimulation' | 'inhibition' | 'catalysis' | 'transport'
  points: [number, number][]
  /** An arrowhead at the start as well (reversible conversions). */
  both?: boolean
  label?: string
}
export interface PathwayLayout {
  width: number
  height: number
  nodes: PlacedNode[]
  compartments: (Box & { label: string })[]
  edges: PlacedEdge[]
}

const FONT = 12
const textW = (s: string) => plainLabel(s).length * FONT * 0.56

function sizeOf(n: PathwayNode): { w: number; h: number } {
  const tw = textW(n.label)
  switch (n.type) {
    case 'metabolite':
      return { w: Math.max(56, tw + 26), h: 36 }
    case 'complex':
      return { w: Math.max(84, tw + 34), h: 40 }
    case 'gene':
      return { w: Math.max(80, tw + 26), h: 34 }
    default:
      return { w: Math.max(80, tw + 26), h: 34 }
  }
}

interface ElkNode {
  id: string
  width?: number
  height?: number
  x?: number
  y?: number
  children?: ElkNode[]
  edges?: ElkEdge[]
  layoutOptions?: Record<string, string>
}
interface ElkEdge {
  id: string
  sources: string[]
  targets: string[]
  sections?: { startPoint: { x: number; y: number }; endPoint: { x: number; y: number }; bendPoints?: { x: number; y: number }[] }[]
}

/** Lay a pathway out with elkjs (loaded here, the first time one is drawn). */
export async function layoutPathway(spec: PathwaySpec): Promise<PathwayLayout> {
  const { default: ELK } = await import('elkjs/lib/elk.bundled.js')
  const elk = new ELK()
  const dir = spec.direction === 'right' ? 'RIGHT' : 'DOWN'
  const procs: { id: string; edge: PathwayEdge }[] = []
  const children = new Map<string, ElkNode>()
  const root: ElkNode = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': dir,
      'elk.hierarchyHandling': 'INCLUDE_CHILDREN',
      // Edge points relative to the whole drawing, not to their compartment.
      'org.eclipse.elk.json.edgeCoords': 'ROOT',
      'elk.spacing.nodeNode': '34',
      'elk.layered.spacing.nodeNodeBetweenLayers': '46',
      'elk.layered.spacing.edgeNodeBetweenLayers': '20',
      'elk.edgeRouting': 'ORTHOGONAL',
      'elk.padding': '[top=14,left=14,bottom=14,right=14]',
    },
    children: [],
    edges: [],
  }
  for (const c of spec.compartments) {
    const el: ElkNode = {
      id: c.id,
      children: [],
      layoutOptions: { 'elk.padding': '[top=34,left=18,bottom=18,right=18]', 'elk.spacing.nodeNode': '34' },
    }
    children.set(c.id, el)
    root.children!.push(el)
  }
  const container = (n: PathwayNode) => (n.compartment ? children.get(n.compartment)! : root)
  const byId = new Map(spec.nodes.map((n) => [n.id, n]))
  for (const n of spec.nodes) {
    const s = sizeOf(n)
    container(n).children!.push({ id: n.id, width: s.w, height: s.h })
  }
  const procHome = (e: PathwayEdge): ElkNode => {
    const home = byId.get(e.from[0])?.compartment ?? byId.get(e.to[0])?.compartment
    return home ? children.get(home)! : root
  }
  let k = 0
  const link = (a: string, b: string) => root.edges!.push({ id: `e${k++}`, sources: [a], targets: [b] })
  const meta: { kind: PlacedEdge['kind']; from: string; to: string; both?: boolean; label?: string }[] = []
  spec.edges.forEach((e, i) => {
    if (e.kind === 'convert' || e.kind === 'reversible') {
      const pid = `__p${i}`
      procHome(e).children!.push({ id: pid, width: 12, height: 12 })
      procs.push({ id: pid, edge: e })
      for (const f of e.from) {
        link(f, pid)
        meta.push({ kind: 'consumption', from: f, to: pid, both: e.kind === 'reversible' })
      }
      for (const t of e.to) {
        link(pid, t)
        meta.push({ kind: 'production', from: pid, to: t, label: e.label })
      }
      for (const cat of e.catalysts) {
        link(cat, pid)
        meta.push({ kind: 'catalysis', from: cat, to: pid })
      }
    } else {
      link(e.from[0], e.to[0])
      meta.push({ kind: e.kind === 'activates' ? 'stimulation' : e.kind === 'inhibits' ? 'inhibition' : 'transport', from: e.from[0], to: e.to[0], label: e.label })
    }
  })

  const res = (await elk.layout(root as never)) as unknown as ElkNode
  // Absolute positions: each node's x/y is relative to its parent.
  const boxes = new Map<string, Box>()
  const walk = (n: ElkNode, ox: number, oy: number) => {
    for (const c of n.children ?? []) {
      const b = { id: c.id, x: ox + (c.x ?? 0), y: oy + (c.y ?? 0), w: c.width ?? 0, h: c.height ?? 0 }
      boxes.set(c.id, b)
      walk(c, b.x, b.y)
    }
  }
  walk(res, 0, 0)
  const nodes: PlacedNode[] = []
  for (const n of spec.nodes) nodes.push({ ...boxes.get(n.id)!, node: n })
  for (const p of procs) nodes.push({ ...boxes.get(p.id)!, process: true })
  const compartments = spec.compartments.map((c) => ({ ...boxes.get(c.id)!, label: c.label }))
  const edges: PlacedEdge[] = (res.edges ?? []).map((e, i) => {
    const sec = e.sections?.[0]
    const pts: [number, number][] = sec ? [[sec.startPoint.x, sec.startPoint.y], ...(sec.bendPoints ?? []).map((b): [number, number] => [b.x, b.y]), [sec.endPoint.x, sec.endPoint.y]] : []
    const m = meta[i]
    return { kind: m.kind, points: pts, both: m.both, label: m.label }
  })
  return { width: Math.ceil(res.width ?? 400), height: Math.ceil(res.height ?? 300), nodes, compartments, edges }
}

const NODE_COLORS: Record<PathwayNode['type'], string> = { gene: '#7c6cf0', protein: '#3987e5', metabolite: '#1baf7a', complex: '#e5832b' }

function nodeShape(n: PlacedNode, c: SciColors): string {
  const { x, y, w, h } = n
  const type = n.node!.type
  const col = NODE_COLORS[type]
  const f = `fill="${col}" fill-opacity="${c.dark ? 0.22 : 0.13}" stroke="${col}" stroke-width="1.8"`
  if (type === 'metabolite') return `<ellipse cx="${x + w / 2}" cy="${y + h / 2}" rx="${w / 2}" ry="${h / 2}" ${f}/>`
  if (type === 'gene') return `<path d="M ${x} ${y} H ${x + w} V ${y + h - 10} Q ${x + w} ${y + h} ${x + w - 10} ${y + h} H ${x + 10} Q ${x} ${y + h} ${x} ${y + h - 10} Z" ${f}/>`
  if (type === 'complex') {
    const t = 9
    const pts = (i: number) => {
      const [l, r, tp, bt] = [x + i, x + w - i, y + i, y + h - i]
      return [[l + t, tp], [r - t, tp], [r, tp + t], [r, bt - t], [r - t, bt], [l + t, bt], [l, bt - t], [l, tp + t]].map((p) => p.join(',')).join(' ')
    }
    return `<polygon points="${pts(0)}" ${f}/><polygon points="${pts(3)}" fill="none" stroke="${col}" stroke-width="1" opacity=".6"/>`
  }
  return `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="12" ${f}/>`
}

function head(kind: PlacedEdge['kind'], tip: [number, number], from: [number, number], c: SciColors): string {
  const dx = tip[0] - from[0]
  const dy = tip[1] - from[1]
  const len = Math.hypot(dx, dy) || 1
  const ux = dx / len
  const uy = dy / len
  const nx = -uy
  const ny = ux
  const P = (a: number, b: number) => `${Math.round((tip[0] - ux * a + nx * b) * 10) / 10},${Math.round((tip[1] - uy * a + ny * b) * 10) / 10}`
  switch (kind) {
    case 'inhibition':
      return `<line x1="${P(0, -7).split(',')[0]}" y1="${P(0, -7).split(',')[1]}" x2="${P(0, 7).split(',')[0]}" y2="${P(0, 7).split(',')[1]}" stroke="${c.ink}" stroke-width="2.4"/>`
    case 'catalysis': {
      const cx = tip[0] - ux * 5
      const cy = tip[1] - uy * 5
      return `<circle cx="${cx}" cy="${cy}" r="5" fill="${c.paper}" stroke="${c.ink}" stroke-width="1.5"/>`
    }
    case 'stimulation':
      return `<polygon points="${P(0, 0)} ${P(10, 5.5)} ${P(10, -5.5)}" fill="${c.paper}" stroke="${c.ink}" stroke-width="1.5"/>`
    default:
      return `<polygon points="${P(0, 0)} ${P(10, 5)} ${P(10, -5)}" fill="${c.ink}"/>`
  }
}

export function pathwaySvg(spec: PathwaySpec, lay: PathwayLayout, c: SciColors): string {
  const titleH = spec.title ? 32 : 8
  const pad = 14
  const out: string[] = []
  for (const comp of lay.compartments) {
    out.push(`<rect x="${comp.x}" y="${comp.y}" width="${comp.w}" height="${comp.h}" rx="18" fill="${c.ink}" fill-opacity=".03" stroke="${c.muted}" stroke-width="1.4" stroke-dasharray="6 4"/>`)
    out.push(svgText(comp.x + 14, comp.y + 20, comp.label, { size: 12, weight: 600, anchor: 'start', fill: c.muted }))
  }
  for (const e of lay.edges) {
    if (e.points.length < 2) continue
    const pts = e.points
    const dash = e.kind === 'transport' ? ' stroke-dasharray="7 4"' : ''
    const last = pts[pts.length - 1]
    const prev = pts[pts.length - 2]
    // The line stops where the head starts, so a hollow head stays hollow.
    const inset = e.kind === 'inhibition' || e.kind === 'consumption' ? 0 : e.kind === 'catalysis' ? 10 : 9
    const trimmed = pts.map((p) => p)
    if (inset && e.kind !== 'consumption') {
      const dx = last[0] - prev[0]
      const dy = last[1] - prev[1]
      const len = Math.hypot(dx, dy) || 1
      trimmed[trimmed.length - 1] = [last[0] - (dx / len) * inset, last[1] - (dy / len) * inset]
    }
    out.push(`<polyline points="${trimmed.map((p) => `${p[0]},${p[1]}`).join(' ')}" fill="none" stroke="${c.ink}" stroke-width="1.5"${dash} stroke-linejoin="round"/>`)
    if (e.kind !== 'consumption') out.push(head(e.kind === 'transport' ? 'production' : e.kind, last, prev, c))
    if (e.both) out.push(head('production', pts[0], pts[1], c))
    if (e.label) {
      const mid = pts[Math.floor(pts.length / 2)]
      out.push(svgText(mid[0] + 6, mid[1] - 4, e.label, { size: 10.5, anchor: 'start', fill: c.muted }))
    }
  }
  for (const n of lay.nodes) {
    if (n.process) {
      out.push(`<rect x="${n.x}" y="${n.y}" width="${n.w}" height="${n.h}" fill="${c.paper}" stroke="${c.ink}" stroke-width="1.5"/>`)
      continue
    }
    out.push(`<g class="sci-node"><title>${escapeHtml(n.node!.type)}</title>${nodeShape(n, c)}</g>`)
    out.push(svgText(n.x + n.w / 2, n.y + n.h / 2 + 4, n.node!.label, { size: FONT, fill: c.ink, weight: 500 }))
  }
  const w = lay.width + pad * 2
  const h = lay.height + titleH + pad
  const title = spec.title ? svgText(w / 2, 22, spec.title, { size: 14, weight: 600, fill: c.ink }) : ''
  return `<svg xmlns="http://www.w3.org/2000/svg" class="sci-svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" font-family="system-ui, -apple-system, 'Segoe UI', sans-serif" color="${c.ink}">${title}<g transform="translate(${pad} ${titleH})">${out.join('')}</g></svg>`
}
