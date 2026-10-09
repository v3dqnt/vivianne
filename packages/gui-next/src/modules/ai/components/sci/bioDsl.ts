import { DslError, isQuoted, keyValue, parseColor, parseNumber, scriptLines, tokenize, unquote, type Line } from './dslCommon'

/**
 * The ```bio script. The first line names what to draw, and the rest says how:
 *
 *   sequence | plasmid | tree | pathway | cell   [title]
 *
 * Every parser here reports the line of the script it stopped at.
 */

export interface Feature {
  start: number
  end: number
  label: string
  color?: string
  /** Plasmids: which way the arrow points. */
  dir?: 'cw' | 'ccw' | 'none'
  line: number
}

export interface SequenceSpec {
  kind: 'sequence'
  title?: string
  seq: string
  type: 'dna' | 'rna' | 'protein'
  features: Feature[]
  /** Reading frames to translate: 1..3, or -1..-3 for the reverse strand. */
  translate: number[]
  color: 'base' | 'none'
  width: number
  complement: boolean
  numbers: boolean
}

export interface PlasmidSpec {
  kind: 'plasmid'
  title: string
  size: number
  features: Feature[]
  sites: { name: string; pos: number; line: number }[]
}

export interface NewickNode {
  name?: string
  length?: number
  children: NewickNode[]
}

export interface TreeSpec {
  kind: 'tree'
  title?: string
  root: NewickNode
  layout: 'rectangular' | 'circular' | 'cladogram'
  scale: boolean
  highlights: { taxa: string[]; label: string; color?: string; line: number }[]
}

export type PathwayType = 'gene' | 'protein' | 'metabolite' | 'complex'
export interface PathwayNode {
  id: string
  type: PathwayType
  label: string
  compartment?: string
  line: number
}
export interface PathwayCompartment {
  id: string
  label: string
  line: number
}
export interface PathwayEdge {
  kind: 'convert' | 'reversible' | 'activates' | 'inhibits' | 'transports'
  from: string[]
  to: string[]
  /** Enzymes or other nodes that catalyse a conversion. */
  catalysts: string[]
  label?: string
  line: number
}
export interface PathwaySpec {
  kind: 'pathway'
  title?: string
  direction: 'down' | 'right'
  compartments: PathwayCompartment[]
  nodes: PathwayNode[]
  edges: PathwayEdge[]
}

export type CellKind = 'animal' | 'plant' | 'bacterium'
export interface CellSpec {
  kind: 'cell'
  title?: string
  cell: CellKind
  show: string[]
  highlight: string[]
  notes: Record<string, string>
  labels: boolean
}

export type BioSpec = SequenceSpec | PlasmidSpec | TreeSpec | PathwaySpec | CellSpec

const KINDS = ['sequence', 'plasmid', 'tree', 'pathway', 'cell']
const flag = (v: string) => !/^(off|false|no|none|0)$/i.test(v.trim())

/** Every organelle each cell type can show; the first group is the default. */
export const ORGANELLES: Record<CellKind, { default: string[]; all: string[] }> = {
  animal: {
    default: ['nucleus', 'nucleolus', 'mitochondrion', 'rough-er', 'smooth-er', 'golgi', 'lysosome', 'centrioles', 'ribosome', 'vesicle'],
    all: ['nucleus', 'nucleolus', 'mitochondrion', 'rough-er', 'smooth-er', 'golgi', 'lysosome', 'centrioles', 'ribosome', 'vesicle', 'peroxisome', 'cytoskeleton'],
  },
  plant: {
    default: ['cell-wall', 'nucleus', 'nucleolus', 'chloroplast', 'mitochondrion', 'vacuole', 'rough-er', 'golgi', 'ribosome'],
    all: ['cell-wall', 'nucleus', 'nucleolus', 'chloroplast', 'mitochondrion', 'vacuole', 'rough-er', 'smooth-er', 'golgi', 'ribosome', 'vesicle', 'peroxisome', 'cytoskeleton'],
  },
  bacterium: {
    default: ['capsule', 'cell-wall', 'nucleoid', 'plasmid', 'ribosome', 'flagellum', 'pili'],
    all: ['capsule', 'cell-wall', 'nucleoid', 'plasmid', 'ribosome', 'flagellum', 'pili', 'vesicle'],
  },
}

const ORGANELLE_ALIASES: Record<string, string> = {
  mitochondria: 'mitochondrion', mito: 'mitochondrion', er: 'rough-er', rer: 'rough-er', ser: 'smooth-er', 'rough-endoplasmic-reticulum': 'rough-er',
  'smooth-endoplasmic-reticulum': 'smooth-er', 'golgi-apparatus': 'golgi', 'golgi-body': 'golgi', lysosomes: 'lysosome', ribosomes: 'ribosome',
  centriole: 'centrioles', centrosome: 'centrioles', chloroplasts: 'chloroplast', 'central-vacuole': 'vacuole', cellwall: 'cell-wall', wall: 'cell-wall',
  vesicles: 'vesicle', peroxisomes: 'peroxisome', flagella: 'flagellum', pilus: 'pili', fimbriae: 'pili', 'dna-ring': 'nucleoid', plasmids: 'plasmid',
}
export const organelleId = (s: string) => {
  const k = s.trim().toLowerCase().replace(/[\s_]+/g, '-')
  return ORGANELLE_ALIASES[k] ?? k
}

// ── Newick ──────────────────────────────────────────────────────────────

/** Parse a Newick string; `at` offsets errors into the script's lines. */
export function parseNewick(text: string, startLine: number): NewickNode {
  let i = 0
  const lineAt = () => startLine + (text.slice(0, i).match(/\n/g)?.length ?? 0)
  const fail = (msg: string): never => {
    throw new DslError(msg, lineAt())
  }
  const ws = () => {
    while (i < text.length && /\s/.test(text[i])) i++
  }
  const label = (): string | undefined => {
    ws()
    if (text[i] === "'") {
      const end = text.indexOf("'", i + 1)
      if (end < 0) fail('A quoted taxon name is missing its closing quote.')
      const out = text.slice(i + 1, end)
      i = end + 1
      return out
    }
    const m = /^[^\s():;,\[\]]+/.exec(text.slice(i))
    if (!m) return undefined
    i += m[0].length
    return m[0].replace(/_/g, ' ')
  }
  const length = (): number | undefined => {
    ws()
    if (text[i] !== ':') return undefined
    i++
    ws()
    const m = /^-?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/.exec(text.slice(i))
    if (!m) return fail('A branch length after ":" must be a number.')
    i += m[0].length
    return Number(m[0])
  }
  const node = (): NewickNode => {
    ws()
    const n: NewickNode = { children: [] }
    if (text[i] === '(') {
      i++
      for (;;) {
        n.children.push(node())
        ws()
        if (text[i] === ',') {
          i++
          continue
        }
        if (text[i] === ')') {
          i++
          break
        }
        fail(i >= text.length ? 'The tree ends before every "(" is closed.' : `Expected "," or ")" but found "${text[i]}".`)
      }
    }
    n.name = label()
    n.length = length()
    if (!n.children.length && !n.name) fail('A taxon needs a name.')
    return n
  }
  const root = node()
  ws()
  if (text[i] === ';') i++
  ws()
  if (i < text.length) fail(`Unexpected "${text[i]}" after the end of the tree.`)
  return root
}

export function leavesOf(n: NewickNode): NewickNode[] {
  return n.children.length ? n.children.flatMap(leavesOf) : [n]
}

// ── Parsing ─────────────────────────────────────────────────────────────

const SEQ_KEYS = new Set(['title', 'name', 'seq', 'sequence', 'type', 'feature', 'translate', 'color', 'colour', 'width', 'complement', 'numbers'])
const PLASMID_KEYS = new Set(['title', 'name', 'size', 'feature', 'site'])
const TREE_KEYS = new Set(['title', 'name', 'newick', 'layout', 'scale', 'highlight'])
const PATHWAY_KEYS = new Set(['title', 'name', 'direction'])
const CELL_KEYS = new Set(['title', 'name', 'show', 'hide', 'highlight', 'label', 'labels'])

function parseRange(tok: string, line: number): [number, number] {
  const m = /^(\d+)\s*(?:-|\.\.|–)\s*(\d+)$/.exec(tok)
  if (!m) throw new DslError(`Expected a range like 10-45 or 10..45, got "${tok}".`, line)
  return [Number(m[1]), Number(m[2])]
}

function parseFeature(value: string, line: number, plasmid: boolean): Feature {
  const t = tokenize(value, line)
  if (!t.length) throw new DslError('feature needs a range and a label: feature: 10-45 "Promoter" #3987e5', line)
  const [start, end] = parseRange(t[0], line)
  const f: Feature = { start, end, label: '', line }
  const words: string[] = []
  for (const tok of t.slice(1)) {
    if (isQuoted(tok)) f.label = unquote(tok)
    else if (/^#[0-9a-f]{3,8}$/i.test(tok)) f.color = tok
    else if (plasmid && /^(cw|ccw|none)$/i.test(tok)) f.dir = tok.toLowerCase() as Feature['dir']
    else if (/^(red|blue|green|orange|yellow|purple|pink|teal|gray|grey|brown|black)$/i.test(tok)) f.color = parseColor(tok, line)
    else words.push(tok)
  }
  if (!f.label) f.label = words.join(' ')
  if (!f.label) throw new DslError('This feature has no label.', line)
  return f
}

const NAMED: Record<string, string> = {
  red: '#e34948', blue: '#3987e5', green: '#1baf7a', orange: '#e87a2b', yellow: '#eda100', purple: '#7c6cf0', pink: '#e87ba4',
  teal: '#14a3a3', gray: '#8a8a8a', grey: '#8a8a8a', brown: '#9a6b3f', black: '#333333',
}
/** A feature or highlight colour as a CSS value. */
export const resolveColor = (c: string | undefined, fallback: string) => (c ? (NAMED[c.toLowerCase()] ?? c) : fallback)

function parseSequence(lines: Line[], header: Line, title: string | undefined): SequenceSpec {
  const spec: SequenceSpec = { kind: 'sequence', title, seq: '', type: 'dna', features: [], translate: [], color: 'base', width: 0, complement: false, numbers: true }
  let typeSet: string | undefined
  let seqLine = header.n
  for (const { n, text } of lines) {
    const kv = keyValue(text, SEQ_KEYS)
    if (!kv) {
      // A bare line continues the sequence (FASTA headers are skipped).
      if (text.startsWith('>')) continue
      if (!/^[A-Za-z*\-\s\d]+$/.test(text)) throw new DslError(`Unexpected line "${text.slice(0, 30)}": a sequence has letters only; options look like "key: value".`, n)
      spec.seq += text.replace(/[\s\d]/g, '').toUpperCase()
      seqLine = n
      continue
    }
    const { key, value } = kv
    switch (key) {
      case 'title':
      case 'name':
        spec.title = value
        break
      case 'seq':
      case 'sequence':
        if (!/^[A-Za-z*\-\s\d]*$/.test(value)) throw new DslError('A sequence has letters only.', n)
        spec.seq += value.replace(/[\s\d]/g, '').toUpperCase()
        seqLine = n
        break
      case 'type':
        if (!/^(dna|rna|protein|auto)$/i.test(value)) throw new DslError('type is dna, rna or protein.', n)
        typeSet = value.toLowerCase()
        break
      case 'feature':
        spec.features.push(parseFeature(value, n, false))
        break
      case 'translate': {
        spec.translate = value.split(/[\s,]+/).filter(Boolean).map((v) => {
          const f = Number(v)
          if (![1, 2, 3, -1, -2, -3].includes(f)) throw new DslError('translate takes reading frames 1, 2, 3 (or -1, -2, -3 for the reverse strand).', n)
          return f
        })
        break
      }
      case 'color':
      case 'colour':
        spec.color = /^(none|off|plain)$/i.test(value) ? 'none' : 'base'
        break
      case 'width': {
        const w = parseNumber(value, n, 'number of residues per row')
        if (!Number.isInteger(w) || w < 10 || w > 200) throw new DslError('width is residues per row, 10 to 200.', n)
        spec.width = w
        break
      }
      case 'complement':
        spec.complement = flag(value)
        break
      case 'numbers':
        spec.numbers = flag(value)
        break
    }
  }
  if (!spec.seq) throw new DslError('No sequence: add "seq: ATGGCC…".', header.n)
  if (/[^A-Z*\-]/.test(spec.seq)) throw new DslError('The sequence contains characters other than letters.', seqLine)
  const nuc = /^[ACGTUNRYKMSWBDHV\-]+$/.test(spec.seq) && (spec.seq.match(/[ACGTUN]/g)?.length ?? 0) / spec.seq.length > 0.85
  const detected: SequenceSpec['type'] = nuc ? (spec.seq.includes('U') && !spec.seq.includes('T') ? 'rna' : 'dna') : 'protein'
  spec.type = typeSet && typeSet !== 'auto' ? (typeSet as SequenceSpec['type']) : detected
  if (spec.type !== 'protein' && /[^ACGTUNRYKMSWBDHV\-]/.test(spec.seq)) {
    const bad = spec.seq.match(/[^ACGTUNRYKMSWBDHV\-]/)?.[0]
    throw new DslError(`"${bad}" is not a nucleotide code. If this is a protein, say "type: protein".`, seqLine)
  }
  if (spec.type === 'protein' && spec.translate.length) throw new DslError('A protein sequence cannot be translated.', header.n)
  if (spec.type === 'protein' && /[^ACDEFGHIKLMNPQRSTVWYXBZUO*\-]/.test(spec.seq)) {
    const bad = spec.seq.match(/[^ACDEFGHIKLMNPQRSTVWYXBZUO*\-]/)?.[0]
    throw new DslError(`"${bad}" is not an amino-acid code.`, seqLine)
  }
  if (!spec.width) spec.width = spec.type === 'protein' ? 50 : 60
  for (const f of spec.features) {
    if (f.start < 1 || f.end < f.start || f.end > spec.seq.length) throw new DslError(`Feature "${f.label}" runs ${f.start}-${f.end}, but the sequence is ${spec.seq.length} long (positions start at 1).`, f.line)
  }
  return spec
}

function parsePlasmid(lines: Line[], header: Line, title: string | undefined): PlasmidSpec {
  const spec: PlasmidSpec = { kind: 'plasmid', title: title ?? 'Plasmid', size: 0, features: [], sites: [] }
  for (const { n, text } of lines) {
    const kv = keyValue(text, PLASMID_KEYS)
    if (!kv) throw new DslError(`Unexpected line "${text.slice(0, 30)}": options look like "key: value" (size, feature, site).`, n)
    const { key, value } = kv
    if (key === 'title' || key === 'name') spec.title = value
    else if (key === 'size') {
      spec.size = parseNumber(value, n, 'size in bp')
      if (!Number.isInteger(spec.size) || spec.size < 10) throw new DslError('size is the length in base pairs.', n)
    } else if (key === 'feature') spec.features.push(parseFeature(value, n, true))
    else if (key === 'site') {
      const t = tokenize(value, n)
      if (t.length < 2) throw new DslError('site needs a name and a position: site: EcoRI 396', n)
      spec.sites.push({ name: unquote(t[0]), pos: parseNumber(t[1], n, 'position'), line: n })
    }
  }
  if (!spec.size) throw new DslError('A plasmid needs its length: "size: 2686".', header.n)
  for (const f of spec.features) {
    if (f.start < 1 || f.end < 1 || f.start > spec.size || f.end > spec.size) throw new DslError(`Feature "${f.label}" (${f.start}..${f.end}) lies outside the ${spec.size} bp plasmid.`, f.line)
  }
  for (const s of spec.sites) if (s.pos < 1 || s.pos > spec.size) throw new DslError(`Site ${s.name} at ${s.pos} lies outside the ${spec.size} bp plasmid.`, s.line)
  return spec
}

function parseTree(lines: Line[], header: Line, title: string | undefined): TreeSpec {
  const spec: TreeSpec = { kind: 'tree', title, root: { children: [] }, layout: 'rectangular', scale: true, highlights: [] }
  let newick = ''
  let newickLine = 0
  let inNewick = false
  for (const { n, text } of lines) {
    const kv = inNewick ? null : keyValue(text, TREE_KEYS)
    if (!kv) {
      // Newick may be given bare, or run over several lines after "newick:".
      if (!inNewick && !/^[('\[]/.test(text)) throw new DslError(`Unexpected line "${text.slice(0, 30)}": give the tree as Newick, e.g. ((A:1,B:1):1,C:2);`, n)
      if (!newickLine) newickLine = n
      newick += `${text}\n`
      inNewick = !text.includes(';')
      continue
    }
    const { key, value } = kv
    if (key === 'title' || key === 'name') spec.title = value
    else if (key === 'newick') {
      newickLine = n
      newick += `${value}\n`
      inNewick = !value.includes(';')
    } else if (key === 'layout') {
      if (!/^(rectangular|circular|cladogram)$/i.test(value)) throw new DslError('layout is rectangular, circular or cladogram.', n)
      spec.layout = value.toLowerCase() as TreeSpec['layout']
    } else if (key === 'scale') spec.scale = flag(value)
    else if (key === 'highlight') {
      const t = tokenize(value, n)
      const color = t.find((x) => /^#[0-9a-f]{3,8}$/i.test(x))
      const quoted = t.find(isQuoted)
      const taxa = t.filter((x) => x !== color && x !== quoted).map((x) => x.replace(/_/g, ' '))
      if (!taxa.length) throw new DslError('highlight needs taxa: highlight: Human, Chimp "Apes" #e34948', n)
      spec.highlights.push({ taxa, label: quoted ? unquote(quoted) : '', color, line: n })
    }
  }
  if (!newick.trim()) throw new DslError('No tree: add "newick: ((A:1,B:1):1,C:2);".', header.n)
  spec.root = parseNewick(newick.trim(), newickLine)
  const names = new Set(leavesOf(spec.root).map((l) => l.name))
  for (const h of spec.highlights) {
    const missing = h.taxa.find((t) => !names.has(t))
    if (missing) throw new DslError(`highlight names "${missing}", which is not a taxon in the tree (taxa: ${[...names].slice(0, 6).join(', ')}${names.size > 6 ? ', …' : ''}).`, h.line)
  }
  return spec
}

const PATHWAY_TYPES = new Set(['gene', 'protein', 'metabolite', 'complex'])

function parsePathway(lines: Line[], header: Line, title: string | undefined): PathwaySpec {
  const spec: PathwaySpec = { kind: 'pathway', title, direction: 'down', compartments: [], nodes: [], edges: [] }
  const ids = new Set<string>()
  const compartmentIds = new Set<string>()
  const ref = (id: string, n: number) => {
    if (!ids.has(id)) throw new DslError(`"${id}" is not a declared node. Declare it first: node ${id} metabolite "Label".`, n)
    return id
  }
  const list = (s: string, n: number) => s.split('+').map((x) => ref(x.trim(), n))
  for (const { n, text } of lines) {
    const kv = keyValue(text, PATHWAY_KEYS)
    if (kv) {
      if (kv.key === 'title' || kv.key === 'name') spec.title = kv.value
      else if (kv.key === 'direction') {
        if (!/^(down|right)$/i.test(kv.value)) throw new DslError('direction is down or right.', n)
        spec.direction = kv.value.toLowerCase() as 'down' | 'right'
      }
      continue
    }
    const t = tokenize(text, n)
    const head = t[0].toLowerCase()
    if (head === 'compartment') {
      if (t.length < 2) throw new DslError('compartment needs an id: compartment cyt "Cytosol"', n)
      const id = t[1]
      if (ids.has(id)) throw new DslError(`"${id}" is already used.`, n)
      ids.add(id)
      compartmentIds.add(id)
      spec.compartments.push({ id, label: t[2] && isQuoted(t[2]) ? unquote(t[2]) : id, line: n })
    } else if (head === 'node') {
      const id = t[1]
      const type = t[2]?.toLowerCase()
      if (!id || !type) throw new DslError('node needs an id and a type: node HK protein "Hexokinase" in cyt', n)
      if (!PATHWAY_TYPES.has(type)) throw new DslError(`Unknown node type "${t[2]}". Types are gene, protein, metabolite, complex (compartments use "compartment").`, n)
      if (ids.has(id)) throw new DslError(`"${id}" is already used.`, n)
      ids.add(id)
      const lab = t.slice(3).find(isQuoted)
      const inAt = t.findIndex((x, i) => i >= 3 && x.toLowerCase() === 'in')
      const comp = inAt >= 0 ? t[inAt + 1] : undefined
      if (comp && !compartmentIds.has(comp)) throw new DslError(`Unknown compartment "${comp}": declare it first with compartment ${comp} "Label".`, n)
      spec.nodes.push({ id, type: type as PathwayType, label: lab ? unquote(lab) : id, compartment: comp, line: n })
    } else {
      const label = t.find(isQuoted)
      const body = text.replace(/"[^"]*"/g, ' ').replace(/\s+/g, ' ').trim()
      let m: RegExpExecArray | null
      if ((m = /^(\S+)\s+catalyses\s+(.+?)\s*(<->|->)\s*(.+)$/i.exec(body))) {
        spec.edges.push({ kind: m[3] === '<->' ? 'reversible' : 'convert', from: list(m[2], n), to: list(m[4], n), catalysts: list(m[1], n), label: label && unquote(label), line: n })
      } else if ((m = /^(.+?)\s*(<->|->)\s*(.+?)(?:\s+by\s+(.+))?$/i.exec(body))) {
        spec.edges.push({ kind: m[2] === '<->' ? 'reversible' : 'convert', from: list(m[1], n), to: list(m[3], n), catalysts: m[4] ? list(m[4], n) : [], label: label && unquote(label), line: n })
      } else if ((m = /^(\S+)\s+(activates|inhibits|transports)\s+(\S+)$/i.exec(body))) {
        spec.edges.push({ kind: m[2].toLowerCase() as PathwayEdge['kind'], from: [ref(m[1], n)], to: [ref(m[3], n)], catalysts: [], label: label && unquote(label), line: n })
      } else {
        throw new DslError(`Cannot read "${text}". Use "A -> B", "A + B -> C by E", "E catalyses A -> B", "A activates B", "A inhibits B" or "A transports B".`, n)
      }
    }
  }
  if (!spec.nodes.length) throw new DslError('A pathway needs nodes: node glc metabolite "Glucose".', header.n)
  return spec
}

function parseCell(lines: Line[], header: Line, title: string | undefined, kind: CellKind): CellSpec {
  const spec: CellSpec = { kind: 'cell', title, cell: kind, show: [...ORGANELLES[kind].default], highlight: [], notes: {}, labels: true }
  const check = (id: string, n: number) => {
    if (!ORGANELLES[kind].all.includes(id)) throw new DslError(`"${id}" is not drawn for a ${kind} cell. Choose from: ${ORGANELLES[kind].all.join(', ')}.`, n)
    return id
  }
  for (const { n, text } of lines) {
    const kv = keyValue(text, CELL_KEYS)
    if (!kv) throw new DslError(`Unexpected line "${text.slice(0, 30)}": options look like "key: value" (show, hide, highlight, label).`, n)
    const { key, value } = kv
    if (key === 'title' || key === 'name') spec.title = value
    else if (key === 'show') spec.show = value.split(/[\s,]+/).filter(Boolean).map((v) => check(organelleId(v), n))
    else if (key === 'hide') spec.show = spec.show.filter((s) => !value.split(/[\s,]+/).filter(Boolean).map(organelleId).includes(s))
    else if (key === 'highlight') spec.highlight = value.split(/[\s,]+/).filter(Boolean).map((v) => check(organelleId(v), n))
    else if (key === 'labels') spec.labels = flag(value)
    else if (key === 'label') {
      const t = tokenize(value, n)
      if (t.length < 2 || !isQuoted(t[1])) throw new DslError('label needs an organelle and a quoted note: label: nucleus "Holds the DNA"', n)
      spec.notes[check(organelleId(t[0]), n)] = unquote(t[1])
    }
  }
  if (!spec.show.length) throw new DslError('Nothing to show.', header.n)
  return spec
}

export function parseBio(src: string): BioSpec {
  const lines = scriptLines(src)
  if (!lines.length) throw new DslError('Empty script: start with sequence, plasmid, tree, pathway or cell.', 1)
  const header = lines[0]
  const m = /^(\w+)(?:\s+(.*))?$/.exec(header.text)
  const kind = m?.[1].toLowerCase()
  if (!m || !kind || !KINDS.includes(kind)) {
    throw new DslError(`The first line must name what to draw: ${KINDS.join(', ')} (got "${header.text.slice(0, 30)}").`, header.n)
  }
  const rest = m[2]?.trim()
  const title = rest ? unquote(rest.startsWith('"') ? tokenize(rest, header.n)[0] : rest) : undefined
  const body = lines.slice(1)
  switch (kind) {
    case 'sequence':
      return parseSequence(body, header, title)
    case 'plasmid':
      return parsePlasmid(body, header, title)
    case 'tree':
      return parseTree(body, header, title)
    case 'pathway':
      return parsePathway(body, header, title)
    default: {
      const t = (rest ?? '').split(/\s+/)[0]?.toLowerCase()
      const cell = (['animal', 'plant', 'bacterium'] as const).find((c) => c === t || (c === 'bacterium' && /^bacteri/.test(t ?? '')))
      if (!cell) throw new DslError('A cell is animal, plant or bacterium: "cell animal".', header.n)
      const after = rest?.slice(t!.length).trim()
      return parseCell(body, header, after ? unquote(after.startsWith('"') ? tokenize(after, header.n)[0] : after) : undefined, cell)
    }
  }
}
