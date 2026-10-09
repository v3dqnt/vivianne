import type { CellKind, CellSpec } from './bioDsl'
import type { SciColors } from './dslCommon'
import { escapeHtml, svgText } from './symbols'

/**
 * Cell illustrations, drawn here from plain shapes: an animal, plant or
 * bacterial cell with its organelles, labelled on leader lines down both
 * sides. Nothing is fetched or embedded; every organelle is a few paths.
 */

interface Ctx {
  c: SciColors
  /** Whether this organelle is highlighted. */
  hl: boolean
}

interface Placed {
  id: string
  /** Where the label's leader line ends, in the 640-wide cell frame. */
  at: [number, number]
  body: string
  /** Radius of the highlight halo. */
  halo: number
}

const HUES = {
  nucleus: '#7c6cf0', mito: '#e5832b', er: '#2fa37a', golgi: '#d9529c', lyso: '#d94c4c', vac: '#4ea3e0', chloro: '#3aa655', perox: '#b58a2a', wall: '#8a7a55',
}

const n2 = (n: number) => Math.round(n * 10) / 10

function tint(color: string, a: number) {
  return `fill="${color}" fill-opacity="${a}" stroke="${color}"`
}

/** Dots: ribosomes, scattered on a path or free in the cytoplasm. */
const dots = (pts: [number, number][], r: number, color: string) => pts.map(([x, y]) => `<circle cx="${n2(x)}" cy="${n2(y)}" r="${r}" fill="${color}"/>`).join('')

function nucleus(x: number, y: number, r: number, k: Ctx, nucleolus: boolean): string {
  const pores = Array.from({ length: 10 }, (_, i) => {
    const a = (i / 10) * Math.PI * 2 + 0.2
    return `<circle cx="${n2(x + r * Math.cos(a))}" cy="${n2(y + r * Math.sin(a))}" r="2.6" fill="${k.c.paper}" stroke="${HUES.nucleus}" stroke-width="1"/>`
  }).join('')
  return (
    `<circle cx="${x}" cy="${y}" r="${r}" ${tint(HUES.nucleus, 0.13)} stroke-width="2"/>` +
    `<circle cx="${x}" cy="${y}" r="${r - 5}" fill="none" stroke="${HUES.nucleus}" stroke-width="1" stroke-dasharray="3 3" opacity=".6"/>${pores}` +
    (nucleolus ? `<circle cx="${n2(x + r * 0.2)}" cy="${n2(y + r * 0.12)}" r="${n2(r * 0.26)}" ${tint(HUES.nucleus, 0.5)} stroke-width="1"/>` : '') +
    // chromatin
    `<path d="M ${x - r * 0.5} ${y - r * 0.2} q ${r * 0.2} ${-r * 0.25} ${r * 0.35} 0 t ${r * 0.3} ${r * 0.15} M ${x - r * 0.3} ${y + r * 0.45} q ${r * 0.25} ${-r * 0.2} ${r * 0.4} 0` + `" fill="none" stroke="${HUES.nucleus}" stroke-width="1" opacity=".55"/>`
  )
}

function mitochondrion(x: number, y: number, rot: number, s: number): string {
  const w = 74 * s
  const h = 36 * s
  const cristae = Array.from({ length: 5 }, (_, i) => {
    const cx = -w / 2 + w * (0.2 + i * 0.15)
    const dir = i % 2 ? 1 : -1
    return `<path d="M ${n2(cx)} ${dir * (h / 2 - 1)} q ${n2(w * 0.04)} ${n2(-dir * h * 0.5)} ${n2(w * 0.02)} ${n2(-dir * h * 0.62)}" fill="none" stroke="${HUES.mito}" stroke-width="1.4"/>`
  }).join('')
  return (
    `<g transform="translate(${x} ${y}) rotate(${rot})">` +
    `<rect x="${-w / 2}" y="${-h / 2}" width="${w}" height="${h}" rx="${h / 2}" ${tint(HUES.mito, 0.16)} stroke-width="1.8"/>` +
    `<rect x="${-w / 2 + 4}" y="${-h / 2 + 4}" width="${w - 8}" height="${h - 8}" rx="${(h - 8) / 2}" fill="none" stroke="${HUES.mito}" stroke-width="1" opacity=".5"/>${cristae}</g>`
  )
}

function golgi(x: number, y: number, rot: number): string {
  const sacs = [0, 1, 2, 3, 4].map((i) => {
    const w = 70 - Math.abs(i - 2) * 9
    const yy = (i - 2) * 9
    return `<path d="M ${-w / 2} ${yy + 3} q ${w / 2} ${-9} ${w} 0" fill="none" stroke="${HUES.golgi}" stroke-width="4.6" stroke-linecap="round"/>`
  }).join('')
  return `<g transform="translate(${x} ${y}) rotate(${rot})">${sacs}</g>`
}

function vesicle(x: number, y: number, r: number, color = HUES.golgi): string {
  return `<circle cx="${x}" cy="${y}" r="${r}" ${tint(color, 0.18)} stroke-width="1.4"/>`
}

function roughER(cx: number, cy: number, r0: number, a0: number, a1: number, rows: number, c: SciColors): string {
  let out = ''
  for (let i = 0; i < rows; i++) {
    const r = r0 + i * 8
    const p0 = [cx + r * Math.cos(a0), cy + r * Math.sin(a0)]
    const p1 = [cx + r * Math.cos(a1), cy + r * Math.sin(a1)]
    out += `<path d="M ${n2(p0[0])} ${n2(p0[1])} A ${r} ${r} 0 0 1 ${n2(p1[0])} ${n2(p1[1])}" fill="none" stroke="${HUES.er}" stroke-width="4.5" stroke-linecap="round" opacity=".85"/>`
    const steps = Math.floor(((a1 - a0) * r) / 9)
    const pts: [number, number][] = []
    for (let s = 1; s < steps; s++) {
      const a = a0 + ((a1 - a0) * s) / steps
      pts.push([cx + (r + 3.6) * Math.cos(a), cy + (r + 3.6) * Math.sin(a)])
    }
    out += dots(pts, 1.5, c.ink)
  }
  return out
}

function smoothER(x: number, y: number): string {
  return (
    `<g transform="translate(${x} ${y})" fill="none" stroke="${HUES.er}" stroke-width="4.5" stroke-linecap="round" opacity=".85">` +
    `<path d="M -42 -6 q 14 -22 28 -4 t 28 2 t 26 -8"/><path d="M -30 12 q 12 -14 26 0 t 26 -2 t 22 6"/><path d="M -14 -4 q 4 12 -4 22"/><path d="M 20 -2 q 6 10 0 20"/></g>`
  )
}

function centrioles(x: number, y: number, c: SciColors): string {
  const tube = (rot: number, dx: number, dy: number) => {
    const lines = Array.from({ length: 4 }, (_, i) => `<line x1="${-9 + i * 6}" y1="-7" x2="${-9 + i * 6}" y2="7" stroke="${c.ink}" stroke-width="1" opacity=".6"/>`).join('')
    return `<g transform="translate(${x + dx} ${y + dy}) rotate(${rot})"><rect x="-11" y="-7" width="22" height="14" rx="5" fill="${c.ink}" fill-opacity=".08" stroke="${c.ink}" stroke-width="1.5"/>${lines}</g>`
  }
  return tube(0, -3, 6) + tube(90, 7, -3)
}

function lysosome(x: number, y: number): string {
  return `<circle cx="${x}" cy="${y}" r="15" ${tint(HUES.lyso, 0.2)} stroke-width="1.8"/>${dots([[x - 4, y - 3], [x + 4, y + 2], [x - 2, y + 6], [x + 5, y - 5]], 1.6, HUES.lyso)}`
}

function peroxisome(x: number, y: number): string {
  return `<circle cx="${x}" cy="${y}" r="12" ${tint(HUES.perox, 0.2)} stroke-width="1.6"/><rect x="${x - 4}" y="${y - 4}" width="8" height="8" fill="${HUES.perox}" opacity=".7"/>`
}

function chloroplast(x: number, y: number, rot: number): string {
  const stacks = [-18, 0, 18].map((dx) => [0, 1, 2, 3].map((i) => `<ellipse cx="${dx}" cy="${-8 + i * 5.5}" rx="7" ry="2.2" fill="${HUES.chloro}" fill-opacity=".55" stroke="${HUES.chloro}" stroke-width=".8"/>`).join('')).join('')
  return `<g transform="translate(${x} ${y}) rotate(${rot})"><ellipse rx="40" ry="23" ${tint(HUES.chloro, 0.14)} stroke-width="1.8"/>${stacks}</g>`
}

function cytoskeleton(paths: string[], c: SciColors): string {
  return paths.map((d) => `<path d="${d}" fill="none" stroke="${c.muted}" stroke-width="1.1" stroke-dasharray="1 4" stroke-linecap="round" opacity=".8"/>`).join('')
}

function layoutAnimal(show: Set<string>, k: (id: string) => Ctx): { shell: string; parts: Placed[] } {
  const c = k('').c
  const shell = `<path d="M 118 215 C 112 120 205 56 322 56 C 440 56 530 112 530 205 C 530 305 450 388 330 388 C 215 388 124 330 118 215 Z" fill="${c.ink}" fill-opacity=".035" stroke="${c.ink}" stroke-width="2.4"/>`
  const parts: Placed[] = []
  const add = (id: string, at: [number, number], body: string, halo: number) => {
    if (show.has(id)) parts.push({ id, at, body, halo })
  }
  add('cytoskeleton', [180, 330], cytoskeleton(['M 150 190 C 160 150 200 110 250 100', 'M 520 230 C 500 300 440 350 380 368', 'M 160 270 C 190 330 260 370 300 372'], c), 30)
  add('rough-er', [370, 150], roughER(290, 205, 70, -1.0, 0.2, 4, c), 55)
  add('nucleus', [260, 190], nucleus(290, 205, 58, k('nucleus'), show.has('nucleolus')), 66)
  add('nucleolus', [298, 214], '', 20)
  add('smooth-er', [200, 330], smoothER(220, 322), 48)
  add('golgi', [450, 290], golgi(450, 292, -12), 44)
  add('mitochondrion', [450, 130], mitochondrion(452, 128, -22, 1) + mitochondrion(190, 255, 40, 0.9), 44)
  add('lysosome', [205, 118], lysosome(205, 118), 24)
  add('centrioles', [335, 345], centrioles(335, 345, c), 24)
  add('vesicle', [395, 255], vesicle(395, 258, 7) + vesicle(412, 238, 5) + vesicle(385, 280, 5), 24)
  add('peroxisome', [500, 215], peroxisome(498, 215), 20)
  add('ribosome', [140, 205], dots([[142, 190], [150, 205], [136, 215], [158, 222], [146, 238], [250, 300], [262, 310], [420, 185], [432, 195], [405, 330], [372, 318]], 2.1, c.ink), 22)
  return { shell, parts }
}

function layoutPlant(show: Set<string>, k: (id: string) => Ctx): { shell: string; parts: Placed[] } {
  const c = k('').c
  const wall = show.has('cell-wall')
  const shell =
    (wall ? `<rect x="104" y="56" width="440" height="332" rx="34" fill="none" stroke="${HUES.wall}" stroke-width="9" opacity=".75"/>` : '') +
    `<rect x="116" y="68" width="416" height="308" rx="26" fill="${c.ink}" fill-opacity=".035" stroke="${c.ink}" stroke-width="2.2"/>`
  const parts: Placed[] = []
  const add = (id: string, at: [number, number], body: string, halo: number) => {
    if (show.has(id)) parts.push({ id, at, body, halo })
  }
  add('cell-wall', [110, 150], '', 18)
  add('vacuole', [330, 225], `<path d="M 235 215 C 232 150 290 140 335 142 C 400 144 440 170 432 230 C 426 290 380 312 325 308 C 262 304 238 268 235 215 Z" ${tint(HUES.vac, 0.16)} stroke-width="2"/>`, 70)
  add('nucleus', [190, 150], nucleus(190, 150, 40, k('nucleus'), show.has('nucleolus')), 50)
  add('nucleolus', [196, 156], '', 14)
  add('rough-er', [150, 225], roughER(190, 150, 48, 0.4, 2.0, 3, c), 40)
  add('chloroplast', [470, 120], chloroplast(470, 118, -15) + chloroplast(468, 328, 12) + chloroplast(160, 320, -6), 46)
  add('mitochondrion', [410, 335], mitochondrion(325, 345, 8, 0.8) + mitochondrion(175, 260, 70, 0.7), 36)
  add('golgi', [490, 225], golgi(492, 225, 90), 40)
  add('smooth-er', [190, 345], smoothER(225, 342), 40)
  add('vesicle', [440, 260], vesicle(450, 268, 6, HUES.vac) + vesicle(438, 250, 4, HUES.vac), 20)
  add('peroxisome', [260, 100], peroxisome(262, 100), 18)
  add('cytoskeleton', [150, 100], cytoskeleton(['M 140 130 C 170 90 230 90 280 96', 'M 520 150 C 515 250 500 300 470 355'], c), 24)
  add('ribosome', [300, 100], dots([[300, 100], [312, 96], [322, 104], [300, 112], [350, 95], [364, 100], [510, 300], [520, 312]], 2.1, c.ink), 20)
  return { shell, parts }
}

function layoutBacterium(show: Set<string>, k: (id: string) => Ctx): { shell: string; parts: Placed[] } {
  const c = k('').c
  const shell =
    (show.has('capsule') ? `<rect x="92" y="124" width="456" height="196" rx="98" fill="none" stroke="${HUES.wall}" stroke-width="5" opacity=".45"/>` : '') +
    (show.has('cell-wall') ? `<rect x="106" y="138" width="428" height="168" rx="84" fill="none" stroke="${HUES.wall}" stroke-width="5.5" opacity=".85"/>` : '') +
    `<rect x="114" y="146" width="412" height="152" rx="76" fill="${c.ink}" fill-opacity=".04" stroke="${c.ink}" stroke-width="1.8"/>`
  const parts: Placed[] = []
  const add = (id: string, at: [number, number], body: string, halo: number) => {
    if (show.has(id)) parts.push({ id, at, body, halo })
  }
  add('capsule', [100, 190], '', 14)
  add('cell-wall', [124, 258], '', 14)
  add('nucleoid', [300, 222], `<path d="M 255 225 C 250 190 300 180 320 200 C 345 220 300 245 280 232 C 265 222 290 205 310 215 C 330 226 345 205 335 192 C 322 180 295 192 300 210 C 305 230 345 240 355 220 C 365 200 340 180 322 176" fill="none" stroke="${HUES.nucleus}" stroke-width="2.6" stroke-linecap="round"/><ellipse cx="305" cy="214" rx="62" ry="38" fill="${HUES.nucleus}" fill-opacity=".1"/>`, 60)
  add('plasmid', [440, 215], `<circle cx="440" cy="215" r="14" fill="none" stroke="${HUES.chloro}" stroke-width="3"/>`, 22)
  add('ribosome', [210, 205], dots([[205, 195], [218, 215], [200, 228], [225, 248], [395, 190], [410, 235], [470, 250], [380, 262], [160, 215], [175, 245], [250, 270]], 2.6, c.ink), 22)
  add('flagellum', [590, 195], `<path d="M 526 222 C 548 198 560 246 584 220 C 606 196 616 238 636 214" fill="none" stroke="${c.ink}" stroke-width="3" stroke-linecap="round"/><circle cx="526" cy="222" r="4.5" fill="${c.ink}"/>`, 24)
  add('pili', [190, 150], [150, 190, 230, 270, 330].map((x) => `<line x1="${x}" y1="146" x2="${x - 6}" y2="118" stroke="${c.ink}" stroke-width="1.3"/>`).join('') + [210, 300, 400].map((x) => `<line x1="${x}" y1="298" x2="${x + 5}" y2="326" stroke="${c.ink}" stroke-width="1.3"/>`).join(''), 26)
  add('vesicle', [330, 270], vesicle(330, 272, 8, HUES.vac), 14)
  return { shell, parts }
}

const NAMES: Record<string, string> = {
  nucleus: 'Nucleus', nucleolus: 'Nucleolus', mitochondrion: 'Mitochondrion', 'rough-er': 'Rough ER', 'smooth-er': 'Smooth ER', golgi: 'Golgi apparatus',
  lysosome: 'Lysosome', centrioles: 'Centrioles', ribosome: 'Ribosomes', vesicle: 'Vesicles', peroxisome: 'Peroxisome', cytoskeleton: 'Cytoskeleton',
  'cell-wall': 'Cell wall', chloroplast: 'Chloroplast', vacuole: 'Central vacuole', capsule: 'Capsule', nucleoid: 'Nucleoid', plasmid: 'Plasmid',
  flagellum: 'Flagellum', pili: 'Pili',
}
const NOTES: Record<string, string> = {
  nucleus: 'stores the DNA', nucleolus: 'builds ribosomes', mitochondrion: 'makes ATP', 'rough-er': 'makes proteins', 'smooth-er': 'makes lipids',
  golgi: 'sorts and ships', lysosome: 'digests waste', centrioles: 'organise spindle', ribosome: 'translate mRNA', vesicle: 'carry cargo',
  peroxisome: 'breaks down fats', cytoskeleton: 'shape and transport', 'cell-wall': 'rigid support', chloroplast: 'photosynthesis', vacuole: 'stores water, keeps turgor',
  capsule: 'protective layer', nucleoid: 'circular chromosome', plasmid: 'extra genes', flagellum: 'swimming', pili: 'attach and exchange DNA',
}
export const organelleName = (id: string) => NAMES[id] ?? id

export function cellSvg(spec: CellSpec, c: SciColors): string {
  const OFF = 30
  const W = 700
  const H = 430
  const show = new Set(spec.show)
  const hl = new Set(spec.highlight)
  const k = (id: string): Ctx => ({ c, hl: hl.has(id) })
  const layout: Record<CellKind, typeof layoutAnimal> = { animal: layoutAnimal, plant: layoutPlant, bacterium: layoutBacterium }
  const { shell, parts } = layout[spec.cell](show, k)
  const accent = c.dark ? '#e5b84a' : '#eda100'
  const body = parts
    .map((p) => (hl.has(p.id) ? `<circle cx="${p.at[0]}" cy="${p.at[1]}" r="${p.halo}" fill="${accent}" opacity=".28"/>` : '') + p.body)
    .join('')
  const out: string[] = [`<g transform="translate(${OFF} 18)">${shell}${body}</g>`]
  if (spec.title) out.unshift(svgText(W / 2, 18, spec.title, { size: 14, weight: 600, fill: c.ink }))

  // Labels: left column for organelles left of centre, right column for the rest.
  if (spec.labels) {
    const sides: Record<'l' | 'r', (Placed & { y: number })[]> = { l: [], r: [] }
    for (const p of parts) {
      sides[p.at[0] + OFF < W / 2 ? 'l' : 'r'].push({ ...p, y: p.at[1] + 18 })
    }
    for (const side of ['l', 'r'] as const) {
      const list = sides[side].sort((a, b) => a.y - b.y)
      const gap = 33
      for (let i = 0; i < list.length; i++) list[i].y = Math.max(list[i].y, i ? list[i - 1].y + gap : 40)
      for (let i = list.length - 1; i >= 0; i--) {
        const limit = i === list.length - 1 ? H - 14 : list[i + 1].y - gap
        if (list[i].y > limit) list[i].y = limit
      }
      for (const p of list) {
        const x = side === 'l' ? 12 : W - 12
        const anchor = side === 'l' ? 'start' : 'end'
        const ex = side === 'l' ? 150 : W - 150
        const ax = p.at[0] + OFF
        const ay = p.at[1] + 18
        out.push(`<polyline points="${ax},${ay} ${side === 'l' ? Math.min(ex, ax - 6) : Math.max(ex, ax + 6)},${p.y - 4} ${side === 'l' ? 148 : W - 148},${p.y - 4}" fill="none" stroke="${c.muted}" stroke-width=".9" opacity=".75"/>`)
        out.push(`<circle cx="${ax}" cy="${ay}" r="2" fill="${c.ink}"/>`)
        const bold = hl.has(p.id)
        out.push(svgText(x, p.y, NAMES[p.id] ?? p.id, { size: 12, anchor, weight: bold ? 700 : 600, fill: c.ink }))
        const note = spec.notes[p.id] ?? NOTES[p.id]
        if (note) out.push(`<text x="${x}" y="${p.y + 12}" text-anchor="${anchor}" font-size="10" fill="${c.muted}">${escapeHtml(note)}</text>`)
      }
    }
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" class="sci-svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" font-family="system-ui, -apple-system, 'Segoe UI', sans-serif" color="${c.ink}">${out.join('')}</svg>`
}
