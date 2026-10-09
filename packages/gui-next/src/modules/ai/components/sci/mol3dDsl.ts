import { DslError, keyValue, parseNumber, scriptLines, splitOptions, tokenize, isQuoted, unquote } from './dslCommon'

/**
 * The ```mol3d script: a 3D molecule or protein for 3Dmol.js.
 *
 *   pdb: 1CRN                       fetched from RCSB (files.rcsb.org)
 *   style: cartoon color=spectrum   cartoon | stick | sphere | line | cross
 *   style: stick resn=HEM           with a selection (chain, resi, resn, elem, hetero)
 *   surface: opacity=0.6 chain=A
 *   label: resi=10 "Cys10"
 *   spin: on
 *
 * Inline structures: `sdf:`, `xyz:`, `pdb:` or `mol2:` with nothing after the
 * colon, then the file's own lines to the end of the block.
 */

export type StyleKind = 'cartoon' | 'stick' | 'sphere' | 'line' | 'cross'

export interface Selection {
  chain?: string
  resi?: string
  resn?: string
  elem?: string
  hetero?: boolean
}

export interface Mol3DStyle {
  kind: StyleKind
  sel: Selection
  color?: string
  radius?: number
  opacity?: number
  line: number
}

export interface Mol3DSurface {
  sel: Selection
  opacity: number
  color?: string
  line: number
}

export interface Mol3DLabel {
  sel: Selection
  text: string
  line: number
}

export interface Mol3DSpec {
  title?: string
  /** Where the structure comes from: an RCSB id, or inline data. */
  source: { type: 'rcsb'; id: string } | { type: 'inline'; format: 'pdb' | 'sdf' | 'xyz' | 'mol2'; data: string }
  styles: Mol3DStyle[]
  surfaces: Mol3DSurface[]
  labels: Mol3DLabel[]
  spin: boolean
  /** `transparent` follows the theme. */
  background: string
}

const KEYS = new Set(['title', 'pdb', 'rcsb', 'sdf', 'xyz', 'mol2', 'style', 'cartoon', 'stick', 'sphere', 'line', 'surface', 'label', 'spin', 'background', 'bg'])
const STYLES = new Set<StyleKind>(['cartoon', 'stick', 'sphere', 'line', 'cross'])
const COLOR_SCHEMES = new Set(['spectrum', 'chain', 'ss', 'residue', 'element', 'jmol', 'amino', 'nucleic', 'bfactor'])

function selectionOf(opts: Record<string, string>, line: number): Selection {
  const sel: Selection = {}
  if (opts.chain) sel.chain = opts.chain.toUpperCase()
  if (opts.resi) {
    if (!/^[\d,\-]+$/.test(opts.resi)) throw new DslError(`resi is a residue number or a range like 10-25, got "${opts.resi}".`, line)
    sel.resi = opts.resi
  }
  if (opts.resn) sel.resn = opts.resn.toUpperCase()
  if (opts.elem) sel.elem = opts.elem
  if (opts.hetero !== undefined) sel.hetero = !/^(false|no|0)$/i.test(opts.hetero)
  return sel
}

export function parseMol3D(src: string): Mol3DSpec {
  const lines = scriptLines(src.replace(/\r\n?/g, '\n'))
  if (!lines.length) throw new DslError('Empty script: start with pdb: 1CRN or an inline sdf:/xyz:.', 1)
  const spec: Mol3DSpec = { source: { type: 'rcsb', id: '' }, styles: [], surfaces: [], labels: [], spin: false, background: 'transparent' }
  let haveSource = false
  const raw = src.replace(/\r\n?/g, '\n').split('\n')
  for (let li = 0; li < lines.length; li++) {
    const { n, text } = lines[li]
    const kv = keyValue(text, KEYS)
    if (!kv) throw new DslError(`Cannot read "${text.slice(0, 40)}". Lines look like "pdb: 1CRN", "style: cartoon color=spectrum", "label: resi=10 \\"Cys10\\"".`, n)
    const { key, value } = kv
    switch (key) {
      case 'title':
        spec.title = value
        break
      case 'pdb':
      case 'rcsb':
      case 'sdf':
      case 'xyz':
      case 'mol2': {
        if (haveSource) throw new DslError('A block shows one structure: remove the extra source line.', n)
        haveSource = true
        if (value === '') {
          // Inline data: everything after this line, as written.
          const format = key === 'rcsb' ? 'pdb' : key
          const data = raw.slice(n).join('\n').trim()
          if (!data) throw new DslError(`${key}: with nothing after it expects the ${key.toUpperCase()} file's lines underneath.`, n)
          spec.source = { type: 'inline', format: format as 'pdb' | 'sdf' | 'xyz' | 'mol2', data }
          li = lines.length
          break
        }
        if (key !== 'pdb' && key !== 'rcsb') throw new DslError(`${key}: takes the file's lines underneath it, not an id.`, n)
        if (!/^[0-9][A-Za-z0-9]{3}$/.test(value)) throw new DslError(`"${value}" is not a PDB id (a digit and three letters or digits, like 1CRN).`, n)
        spec.source = { type: 'rcsb', id: value.toUpperCase() }
        break
      }
      case 'style':
      case 'cartoon':
      case 'stick':
      case 'sphere':
      case 'line': {
        const tokens = tokenize(value, n)
        const { pos, opts } = splitOptions(tokens)
        const kind = (key === 'style' ? pos[0] : key)?.toLowerCase() as StyleKind
        if (!STYLES.has(kind)) throw new DslError(`Unknown style "${pos[0] ?? ''}". Styles are cartoon, stick, sphere, line, cross (surface has its own line).`, n)
        const style: Mol3DStyle = { kind, sel: selectionOf(opts, n), line: n }
        if (opts.color) {
          if (!/^#[0-9a-f]{3,8}$/i.test(opts.color) && !COLOR_SCHEMES.has(opts.color.toLowerCase()) && !/^[a-z]+$/i.test(opts.color)) throw new DslError(`color is spectrum, chain, ss, residue, element, a colour name or #rrggbb, got "${opts.color}".`, n)
          style.color = opts.color
        }
        if (opts.radius) style.radius = parseNumber(opts.radius, n, 'radius')
        if (opts.opacity) style.opacity = parseNumber(opts.opacity, n, 'opacity')
        spec.styles.push(style)
        break
      }
      case 'surface': {
        const { opts } = splitOptions(tokenize(value, n))
        const opacity = opts.opacity ? parseNumber(opts.opacity, n, 'opacity') : 0.6
        if (opacity < 0 || opacity > 1) throw new DslError('opacity is 0 to 1.', n)
        spec.surfaces.push({ sel: selectionOf(opts, n), opacity, color: opts.color, line: n })
        break
      }
      case 'label': {
        const tokens = tokenize(value, n)
        const { opts } = splitOptions(tokens.filter((t) => !isQuoted(t)))
        const q = tokens.find(isQuoted)
        if (!q) throw new DslError('label needs a quoted text: label: resi=10 "Cys10"', n)
        const sel = selectionOf(opts, n)
        if (!Object.keys(sel).length) throw new DslError('label needs a selection to put it on: resi=10, resn=HEM, chain=A…', n)
        spec.labels.push({ sel, text: unquote(q), line: n })
        break
      }
      case 'spin':
        spec.spin = !/^(off|false|no)$/i.test(value)
        break
      case 'background':
      case 'bg':
        spec.background = value || 'transparent'
        break
    }
  }
  if (!haveSource) throw new DslError('No structure: add "pdb: 1CRN" or an inline sdf:/xyz: block.', lines[0].n)
  if (!spec.styles.length) {
    // Proteins read best as a cartoon; small molecules as sticks.
    const protein = spec.source.type === 'rcsb' || spec.source.format === 'pdb'
    spec.styles.push({ kind: protein ? 'cartoon' : 'stick', sel: {}, color: protein ? 'spectrum' : undefined, line: 1 })
  }
  return spec
}
