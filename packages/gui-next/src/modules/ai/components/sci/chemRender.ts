import type { RDKit } from './rdkit'
import { DslError } from './dslCommon'
import { explainSmiles, splitReaction, type ChemEntry, type ChemHighlight, type ChemSpec } from './chemDsl'
import { escapeHtml, formulaText, plainLabel, svgText } from './symbols'

/**
 * Chemistry figures: RDKit draws each molecule; this file lays the molecules
 * out (a grid of structures, or a scheme of reactants → products with the
 * conditions on the arrows) and composes everything into one SVG, so the
 * whole figure can be copied or exported in a single piece.
 */

export interface ChemColors {
  ink: string
  muted: string
  dark: boolean
  /** Highlight colours, in order. */
  accents: string[]
}

export interface MolInfo {
  smiles: string
  formula: string
  mw: number
}

export interface ChemResult {
  svg: string
  width: number
  height: number
  /** One entry per drawn molecule, in reading order. */
  molecules: MolInfo[]
}

const BOND = 28
const FONT = 12

const DARK_PALETTE: Record<number, [number, number, number]> = {
  1: [0.8, 0.8, 0.8], 6: [0.92, 0.92, 0.92], 7: [0.45, 0.62, 1], 8: [1, 0.42, 0.42], 9: [0.45, 0.85, 0.5], 15: [1, 0.65, 0.25],
  16: [0.92, 0.78, 0.2], 17: [0.4, 0.8, 0.45], 35: [0.85, 0.5, 0.3], 53: [0.72, 0.5, 0.9],
}

function rgb(hex: string): [number, number, number] {
  const h = hex.replace('#', '')
  const f = h.length === 3 ? [...h].map((c) => c + c).join('') : h.slice(0, 6)
  const n = parseInt(f, 16)
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255]
}

type Mol = NonNullable<ReturnType<RDKit['get_mol']>>

/** Parse one structure, or throw a DslError that says what is wrong with it. */
export function parseMol(rd: RDKit, source: string, line: number, molfile = false): Mol {
  const log = rd.set_log_capture('rdApp.*')
  let mol: Mol | null = null
  try {
    mol = rd.get_mol(source)
    if (mol && !mol.is_valid()) {
      mol.delete()
      mol = null
    }
    if (!mol) {
      const raw = log?.get_buffer() ?? ''
      const why = molfile ? raw.trim().split('\n').pop()?.replace(/^\[[^\]]*\]\s*/, '') || 'Invalid molfile.' : explainSmiles(source, raw)
      throw new DslError(molfile ? why : `"${source.length > 60 ? `${source.slice(0, 57)}…` : source}": ${why}`, line)
    }
  } finally {
    log?.delete()
  }
  if (molfile && mol.has_coords() !== 2) mol.set_new_coords(true)
  return mol
}

/** Hill-order formula from the molecule's JSON (explicit and implicit hydrogens counted). */
export function formulaOf(mol: Mol): string {
  const j = JSON.parse(mol.get_json()) as { molecules: { atoms: { z?: number; impHs?: number; chg?: number }[] }[] }
  const counts = new Map<string, number>()
  let charge = 0
  for (const m of j.molecules) {
    for (const a of m.atoms) {
      const sym = SYMBOLS[a.z ?? 6] ?? `#${a.z}`
      counts.set(sym, (counts.get(sym) ?? 0) + 1)
      if (a.impHs) counts.set('H', (counts.get('H') ?? 0) + a.impHs)
      charge += a.chg ?? 0
    }
  }
  const keys = [...counts.keys()]
  const ordered = counts.has('C') ? ['C', ...(counts.has('H') ? ['H'] : []), ...keys.filter((k) => k !== 'C' && k !== 'H').sort()] : keys.sort()
  const sub = (n: number) => (n === 1 ? '' : [...String(n)].map((d) => '₀₁₂₃₄₅₆₇₈₉'[Number(d)]).join(''))
  let f = ordered.map((k) => k + sub(counts.get(k) ?? 0)).join('')
  if (charge) f += (Math.abs(charge) === 1 ? '' : [...String(Math.abs(charge))].map((d) => '⁰¹²³⁴⁵⁶⁷⁸⁹'[Number(d)]).join('')) + (charge > 0 ? '⁺' : '⁻')
  return f
}

const SYMBOLS: Record<number, string> = {
  1: 'H', 2: 'He', 3: 'Li', 4: 'Be', 5: 'B', 6: 'C', 7: 'N', 8: 'O', 9: 'F', 10: 'Ne', 11: 'Na', 12: 'Mg', 13: 'Al', 14: 'Si', 15: 'P', 16: 'S',
  17: 'Cl', 18: 'Ar', 19: 'K', 20: 'Ca', 22: 'Ti', 24: 'Cr', 25: 'Mn', 26: 'Fe', 27: 'Co', 28: 'Ni', 29: 'Cu', 30: 'Zn', 33: 'As', 34: 'Se',
  35: 'Br', 47: 'Ag', 50: 'Sn', 53: 'I', 78: 'Pt', 79: 'Au', 80: 'Hg', 82: 'Pb',
}

export function infoOf(mol: Mol): MolInfo {
  const d = JSON.parse(mol.get_descriptors()) as { amw?: number }
  return { smiles: mol.get_smiles(), formula: formulaOf(mol), mw: d.amw ?? 0 }
}

interface Drawn {
  inner: string
  w: number
  h: number
  info: MolInfo
}

function resolveHighlights(rd: RDKit, mol: Mol, hs: ChemHighlight[], accents: string[], dark: boolean) {
  const atoms: number[] = []
  const bonds: number[] = []
  const atomColors: Record<number, [number, number, number]> = {}
  const bondColors: Record<number, [number, number, number]> = {}
  const n = mol.get_num_atoms()
  hs.forEach((h, i) => {
    const color = rgb(h.color ?? accents[i % accents.length])
    const found: { atoms: number[]; bonds: number[] }[] = []
    if (h.atoms) {
      const bad = h.atoms.find((a) => a >= n)
      if (bad !== undefined) throw new DslError(`highlight atom ${bad} doesn't exist: this molecule has atoms 0 to ${n - 1} (turn on \`labels\` to see them).`, h.line)
      found.push({ atoms: h.atoms, bonds: [] })
    } else if (h.smarts) {
      const q = rd.get_qmol(h.smarts)
      if (!q || !q.is_valid()) {
        q?.delete()
        throw new DslError(`"${h.smarts}" is not valid SMARTS.`, h.line)
      }
      try {
        const m = JSON.parse(mol.get_substruct_matches(q) || '[]') as { atoms: number[]; bonds: number[] }[]
        found.push(...m)
      } finally {
        q.delete()
      }
    }
    for (const f of found) {
      for (const a of f.atoms) {
        atoms.push(a)
        atomColors[a] = color
      }
      for (const b of f.bonds) {
        bonds.push(b)
        bondColors[b] = color
      }
      // Explicit atom lists highlight the bonds between them too.
      if (h.atoms) {
        const set = new Set(f.atoms)
        const mb = mol.get_molblock().split('\n')
        const counts = parseInt(mb[3].slice(0, 3), 10)
        const nb = parseInt(mb[3].slice(3, 6), 10)
        for (let b = 0; b < nb; b++) {
          const row = mb[4 + counts + b]
          const a1 = parseInt(row.slice(0, 3), 10) - 1
          const a2 = parseInt(row.slice(3, 6), 10) - 1
          if (set.has(a1) && set.has(a2)) {
            bonds.push(b)
            bondColors[b] = color
          }
        }
      }
    }
  })
  return { atoms, bonds, atomColors, bondColors, alpha: dark ? 0.55 : 0.5 }
}

function draw(rd: RDKit, mol: Mol, spec: ChemSpec, colors: ChemColors, highlights: ChemHighlight[]): Drawn {
  const hl = resolveHighlights(rd, mol, highlights, colors.accents, colors.dark)
  const palette: Record<number, [number, number, number]> = colors.dark ? { ...DARK_PALETTE } : { 6: rgb(colors.ink) }
  palette[-1] = rgb(colors.ink)
  const details: Record<string, unknown> = {
    width: -1,
    height: -1,
    fixedBondLength: BOND,
    padding: 0.04,
    bondLineWidth: 1.6,
    clearBackground: false,
    atomColourPalette: palette,
    symbolColour: rgb(colors.ink),
    addAtomIndices: spec.labels,
    addStereoAnnotation: spec.cip,
    explicitMethyl: false,
  }
  if (hl.atoms.length || hl.bonds.length) {
    Object.assign(details, {
      atoms: hl.atoms,
      bonds: hl.bonds,
      highlightAtomColors: Object.fromEntries(Object.entries(hl.atomColors).map(([k, v]) => [k, [...v, hl.alpha]])),
      highlightBondColors: Object.fromEntries(Object.entries(hl.bondColors).map(([k, v]) => [k, [...v, hl.alpha]])),
      highlightRadius: 0.35,
    })
  }
  let target = mol
  let owned: Mol | null = null
  if (spec.hs) {
    const withHs = rd.get_mol(mol.add_hs())
    if (withHs) {
      target = withHs
      owned = withHs
    }
  }
  try {
    const svg = target.get_svg_with_highlights(JSON.stringify(details))
    const vb = /viewBox='0 0 ([\d.]+) ([\d.]+)'/.exec(svg)
    const start = svg.indexOf('<!-- END OF HEADER -->')
    const inner = svg.slice(start + 22, svg.lastIndexOf('</svg>')).replace(/<rect[^>]*>\s*<\/rect>|<rect[^>]*\/>/g, '')
    return { inner, w: Number(vb?.[1] ?? 100), h: Number(vb?.[2] ?? 100), info: infoOf(mol) }
  } finally {
    owned?.delete()
  }
}

/** A molecule's drawing placed in a box: scaled down to fit, centred. */
function place(d: Drawn, x: number, y: number, boxW: number, boxH: number, label?: string, grow = 1): string {
  const k = Math.min(grow, boxW / d.w, boxH / d.h)
  const w = d.w * k
  const h = d.h * k
  const ox = x + (boxW - w) / 2
  const oy = y + (boxH - h) / 2
  const tip = `${label ? `${label} — ` : ''}${d.info.formula} · ${d.info.mw.toFixed(2)} g/mol`
  return (
    `<g class="sci-mol"><title>${escapeHtml(tip)}</title>` +
    `<rect x="${ox}" y="${oy}" width="${w}" height="${h}" fill="transparent"/>` +
    `<svg x="${ox}" y="${oy}" width="${w}" height="${h}" viewBox="0 0 ${d.w} ${d.h}">${d.inner}</svg></g>`
  )
}

const arrowSvg = (x1: number, x2: number, y: number, ink: string) =>
  `<line x1="${x1}" y1="${y}" x2="${x2 - 3}" y2="${y}" stroke="${ink}" stroke-width="1.6"/><path d="M${x2} ${y} L${x2 - 9} ${y - 4.5} L${x2 - 9} ${y + 4.5} Z" fill="${ink}"/>`

/** Reagent formulas in a condition line get their subscripts: `H2SO4, 80 °C` → H₂SO₄, 80 °C. */
const reagents = (s: string) => s.replace(/[A-Z][A-Za-z0-9()]*\d[A-Za-z0-9()]*/g, (w) => formulaText(w))

const textWidth = (s: string, size = FONT) => plainLabel(s).length * size * 0.56

interface Item {
  w: number
  h: number
  /** `x`, `top`: where to put it. */
  render: (x: number, top: number, rowH: number) => string
  /** An arrow may start a row on its own. */
  arrow?: boolean
}

/** One reaction, or a chain of them, as a row of structures, plus signs and arrows. */
function scheme(rd: RDKit, entry: ChemEntry, spec: ChemSpec, colors: ChemColors, maxW: number, infos: MolInfo[]): { body: string; w: number; h: number } {
  let parts: ReturnType<typeof splitReaction>
  try {
    parts = splitReaction(entry.source)
  } catch (e) {
    throw new DslError((e as Error).message, entry.line)
  }
  const items: Item[] = []
  const boxW = 190
  const boxH = 150
  parts.species.forEach((group, si) => {
    group.forEach((smi, gi) => {
      const mol = parseMol(rd, smi, entry.line)
      try {
        const d = draw(rd, mol, spec, colors, entry.highlights.filter((h) => h.smarts))
        infos.push(d.info)
        const k = Math.min(1, boxW / d.w, boxH / d.h)
        items.push({ w: d.w * k, h: d.h * k, render: (x, top, rowH) => place(d, x, top + (rowH - d.h * k) / 2, d.w * k, d.h * k) })
      } finally {
        mol.delete()
      }
      if (gi < group.length - 1) items.push({ w: 26, h: 20, render: (x, top, rowH) => svgText(x + 13, top + rowH / 2 + 6, '+', { size: 18, fill: colors.muted }) })
    })
    if (si < parts.agents.length) {
      const agentText = parts.agents[si].map((a) => {
        const m = rd.get_mol(a)
        if (!m || !m.is_valid()) return a
        try {
          return formulaOf(m)
        } finally {
          m.delete()
        }
      })
      const cond = entry.conditions[si] ?? ''
      const [above, below] = reagents(cond).split(/\s\/\s/)
      const top = [...agentText, ...(above ? [above] : [])].join(', ')
      const w = Math.max(70, textWidth(top, 11) + 16, textWidth(below ?? '', 11) + 16)
      items.push({
        w,
        h: 24,
        arrow: true,
        render: (x, y, rowH) => {
          const cy = y + rowH / 2
          return arrowSvg(x + 2, x + w - 2, cy, colors.ink) + (top ? svgText(x + w / 2, cy - 7, top, { size: 11, fill: colors.ink }) : '') + (below ? svgText(x + w / 2, cy + 17, below, { size: 11, fill: colors.muted }) : '')
        },
      })
    }
  })

  // Flow into rows; a row that runs out of room breaks before an arrow.
  const rows: Item[][] = [[]]
  let used = 0
  for (const it of items) {
    if (used + it.w > maxW && rows[rows.length - 1].length && it.arrow) {
      rows.push([])
      used = 0
    }
    rows[rows.length - 1].push(it)
    used += it.w + 6
  }
  let y = 0
  let width = 0
  const out: string[] = []
  for (const row of rows) {
    const rowH = Math.max(...row.map((i) => i.h)) + 28
    let x = 0
    for (const it of row) {
      out.push(it.render(x, y, rowH))
      x += it.w + 6
    }
    width = Math.max(width, x - 6)
    y += rowH
  }
  return { body: out.join(''), w: width, h: y }
}

/** Draw a whole ```chem figure as one SVG. */
export function renderChem(rd: RDKit, spec: ChemSpec, colors: ChemColors): ChemResult {
  const infos: MolInfo[] = []
  const blocks: { y: number; h: number; body: string; w: number }[] = []
  const pad = 12
  const titleH = spec.title ? 28 : 0
  const singles = spec.entries.filter((e) => e.kind !== 'reaction')
  const cols = Math.min(spec.grid || (singles.length <= 1 ? 1 : singles.length === 2 ? 2 : 3), Math.max(1, singles.length))
  const cell = spec.size
  const cellH = Math.round(cell * 0.8)
  const legendH = 34
  const maxW = Math.max(cols * cell, 560)

  // Stand-alone molecules share a grid; reactions each take a full-width row.
  let cursorY = 0
  const grid: string[] = []
  let gridW = 0
  const pendingCells: string[] = []
  const flushGrid = () => {
    if (!pendingCells.length) return
    const rows = Math.ceil(pendingCells.length / cols)
    pendingCells.forEach((c, i) => {
      const cx = (i % cols) * cell
      const cy = cursorY + Math.floor(i / cols) * (cellH + legendH)
      grid.push(`<g transform="translate(${cx} ${cy})">${c}</g>`)
    })
    gridW = Math.max(gridW, Math.min(pendingCells.length, cols) * cell)
    blocks.push({ y: cursorY, h: rows * (cellH + legendH), body: '', w: gridW })
    cursorY += rows * (cellH + legendH)
    pendingCells.length = 0
  }
  const rxnBodies: string[] = []
  let rxnW = 0
  for (const e of spec.entries) {
    if (e.kind === 'reaction') {
      flushGrid()
      const s = scheme(rd, e, spec, colors, maxW, infos)
      rxnBodies.push(`<g transform="translate(0 ${cursorY})">${s.body}</g>`)
      if (e.name) rxnBodies.push(svgText(s.w / 2, cursorY + s.h + 4, e.name, { size: 12, fill: colors.muted }))
      const h = s.h + (e.name ? 22 : 8)
      blocks.push({ y: cursorY, h, body: '', w: s.w })
      rxnW = Math.max(rxnW, s.w)
      cursorY += h
      continue
    }
    const mol = parseMol(rd, e.source, e.line, e.kind === 'molfile')
    try {
      for (const h of e.highlights) if (h.atoms && h.atoms.some((a) => a >= mol.get_num_atoms())) resolveHighlights(rd, mol, [h], colors.accents, colors.dark)
      const d = draw(rd, mol, spec, colors, e.highlights)
      infos.push(d.info)
      const legend = e.name ?? d.info.formula
      pendingCells.push(
        place(d, 8, 4, cell - 16, cellH - 8, e.name, 1.5) +
          svgText(cell / 2, cellH + 14, legend, { size: FONT, fill: colors.ink, weight: 500 }) +
          (e.name ? svgText(cell / 2, cellH + 29, `${d.info.formula} · ${d.info.mw.toFixed(2)}`, { size: 10.5, fill: colors.muted }) : ''),
      )
    } finally {
      mol.delete()
    }
  }
  flushGrid()

  const width = Math.max(gridW, rxnW, 80)
  const height = cursorY + titleH
  const title = spec.title ? svgText(width / 2, 18, spec.title, { size: 13.5, weight: 600, fill: colors.ink }) : ''
  const content = `<g transform="translate(0 ${titleH})">${grid.join('')}${rxnBodies.join('')}</g>`
  const w = width + pad * 2
  const h = height + pad * 2
  // Reaction rows centre under the widest block.
  const shift = (w - width) / 2
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" class="sci-svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" font-family="system-ui, -apple-system, 'Segoe UI', sans-serif" color="${colors.ink}">` +
    `<g transform="translate(${shift} ${pad})">${title}${content}</g></svg>`
  return { svg, width: w, height: h, molecules: infos }
}

/** The summary `check_diagram` gives the model for a drawn figure. */
export function describeChem(r: ChemResult): string {
  return r.molecules.map((m, i) => `${i + 1}. ${m.smiles}  ${m.formula}  ${m.mw.toFixed(2)} g/mol`).join('\n')
}
