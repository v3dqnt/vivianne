import { DslError, keyValue, parseRanges, scriptLines, tokenize, isQuoted, unquote } from './dslCommon'

/**
 * The ```chem script: structures as SMILES, reaction SMILES (`A.B>>C`) or a
 * molfile, with a few option lines. Options written under a structure belong
 * to it; options with nothing above them belong to the structure below.
 * `title`, `grid`, `size`, `labels`, `cip` and `hs` are for the whole figure.
 */

export interface ChemHighlight {
  atoms?: number[]
  smarts?: string
  color?: string
  line: number
}

export interface ChemEntry {
  line: number
  kind: 'molecule' | 'reaction' | 'molfile'
  /** SMILES, reaction SMILES, or the molfile text. */
  source: string
  name?: string
  /** Text for each arrow of a reaction, in order; `above / below`. */
  conditions: string[]
  highlights: ChemHighlight[]
}

export interface ChemSpec {
  title?: string
  entries: ChemEntry[]
  /** Columns for stand-alone molecules; 0 chooses from the count. */
  grid: number
  /** Width of one molecule's cell, in px. */
  size: number
  /** Show RDKit's atom numbers. */
  labels: boolean
  /** Annotate stereocentres with R/S and double bonds with E/Z. */
  cip: boolean
  /** Draw hydrogens explicitly. */
  hs: boolean
}

const GLOBAL_KEYS = new Set(['title', 'grid', 'size', 'labels', 'cip', 'hs'])
const ENTRY_KEYS = new Set(['name', 'conditions', 'highlight'])
const SOURCE_KEYS = new Set(['smiles', 'rxn', 'reaction', 'molfile'])
const KNOWN = new Set([...GLOBAL_KEYS, ...ENTRY_KEYS, ...SOURCE_KEYS])

const flag = (v: string) => !/^(off|false|no|none|0)$/i.test(v.trim())

function parseHighlight(value: string, line: number): ChemHighlight {
  const color = /\s(#[0-9a-f]{3,8})\s*$/i.exec(value)
  const body = color ? value.slice(0, color.index).trim() : value.trim()
  const h: ChemHighlight = { color: color?.[1], line }
  const atoms = /^atoms?\s+(.+)$/i.exec(body)
  const smarts = /^smarts\s+(\S+)$/i.exec(body)
  if (atoms) h.atoms = parseRanges(atoms[1], line)
  else if (smarts) h.smarts = smarts[1]
  else throw new DslError('highlight needs "atoms 0-3" (numbers as shown by `labels`) or "smarts C(=O)O".', line)
  return h
}

/** Quoted strings in order, or the whole text split on `|`. */
function parseConditions(value: string, line: number): string[] {
  if (value.includes('"')) return tokenize(value, line).filter(isQuoted).map(unquote)
  return value.split('|').map((s) => s.trim()).filter(Boolean)
}

export function parseChem(src: string): ChemSpec {
  const spec: ChemSpec = { entries: [], grid: 0, size: 240, labels: false, cip: false, hs: false }
  const text = src.replace(/\r\n?/g, '\n')

  // A molfile is recognised by its "M  END" line; everything from the start of
  // the block (or from a `molfile` line) through that line is the structure.
  const raw = text.split('\n')
  const endAt = raw.findIndex((l) => /^M\s+END\s*$/.test(l.trim()))
  let molfile: { from: number; to: number } | null = null
  if (endAt >= 0) {
    const marker = raw.findIndex((l, i) => i < endAt && /^molfile\s*:?\s*$/i.test(l.trim()))
    molfile = { from: marker >= 0 ? marker + 1 : 0, to: endAt }
  }

  type Key = 'name' | 'conditions' | 'highlight'
  const pending: Record<Key, { value: string; line: number }[]> = { name: [], conditions: [], highlight: [] }
  const give = (target: ChemEntry, key: Key, value: string, line: number) => {
    if (key === 'name') target.name = value
    else if (key === 'conditions') target.conditions = parseConditions(value, line)
    else target.highlights.push(parseHighlight(value, line))
  }
  const attach = (e: ChemEntry | undefined, key: Key, value: string, line: number) => {
    // Highlights stack; a name or a conditions line fills a structure only once.
    const free = e && (key === 'highlight' || (key === 'name' ? e.name === undefined : e.conditions.length === 0))
    if (e && free) give(e, key, value, line)
    else pending[key].push({ value, line })
  }
  const add = (entry: ChemEntry) => {
    for (const key of ['name', 'conditions', 'highlight'] as const) {
      for (const p of pending[key]) give(entry, key, p.value, p.line)
      pending[key] = []
    }
    spec.entries.push(entry)
  }
  const newEntry = (line: number, kind: ChemEntry['kind'], source: string, name?: string): ChemEntry => ({ line, kind, source, name, conditions: [], highlights: [] })
  const addStructure = (line: number, smiles: string, name?: string) => {
    const s = smiles.trim()
    if (!s) throw new DslError('Nothing after the colon: expected a SMILES.', line)
    add(newEntry(line, s.includes('>') ? 'reaction' : 'molecule', s, name))
  }

  let molfileDone = false
  for (const { n, text: line } of scriptLines(text)) {
    if (molfile && n - 1 <= molfile.to && n - 1 >= molfile.from) {
      if (!molfileDone) {
        molfileDone = true
        add(newEntry(n, 'molfile', raw.slice(molfile.from, molfile.to + 1).join('\n')))
      }
      continue
    }
    if (/^molfile\s*:?\s*$/i.test(line)) continue
    // `labels`, `cip` and `hs` stand alone: they are switches, not values.
    if (/^(labels|cip|hs)$/i.test(line)) {
      spec[line.toLowerCase() as 'labels' | 'cip' | 'hs'] = true
      continue
    }
    const kv = keyValue(line, KNOWN)
    if (!kv) {
      // A bare line is a structure: `SMILES` or `SMILES name of the molecule`.
      const sp = line.search(/\s/)
      const smiles = sp < 0 ? line : line.slice(0, sp)
      const name = sp < 0 ? undefined : line.slice(sp + 1).trim() || undefined
      const word = /^([A-Za-z][A-Za-z-]{2,})\s*:\s/.exec(line)
      if (word) throw new DslError(`Unknown option "${word[1]}". Options are: ${[...GLOBAL_KEYS, ...ENTRY_KEYS, 'smiles'].join(', ')}.`, n)
      addStructure(n, smiles, name)
      continue
    }
    const { key, value } = kv
    switch (key) {
      case 'smiles':
      case 'rxn':
      case 'reaction':
        addStructure(n, value)
        break
      case 'title':
        spec.title = value
        break
      case 'grid': {
        const g = Number(value)
        if (!Number.isInteger(g) || g < 1 || g > 8) throw new DslError('grid is a column count from 1 to 8.', n)
        spec.grid = g
        break
      }
      case 'size': {
        const s = Number(value)
        if (!Number.isFinite(s) || s < 120 || s > 600) throw new DslError('size is the width of one molecule in px, 120 to 600.', n)
        spec.size = s
        break
      }
      case 'labels':
        spec.labels = value === '' ? true : flag(value)
        break
      case 'cip':
        spec.cip = value === '' ? true : flag(value)
        break
      case 'hs':
        spec.hs = value === '' ? true : flag(value)
        break
      case 'name':
      case 'conditions':
      case 'highlight':
        attach(spec.entries[spec.entries.length - 1], key, value, n)
        break
    }
  }
  if (!spec.entries.length) throw new DslError('No structure: write a SMILES, a reaction SMILES (A.B>>C) or a molfile.', 1)
  return spec
}

/** Split reaction SMILES `R>A>P` or a chain `A>>B>>C` into species and agent groups. */
export function splitReaction(source: string): { species: string[][]; agents: string[][] } {
  const parts = source.split('>')
  if (parts.length < 3 || parts.length % 2 === 0) throw new Error('A reaction has the form reactants>agents>products, e.g. CCO.CC(=O)O>>CC(=O)OCC.O')
  const species: string[][] = []
  const agents: string[][] = []
  parts.forEach((p, i) => {
    const group = p.split('.').map((s) => s.trim()).filter(Boolean)
    if (i % 2 === 0) {
      if (!group.length) throw new Error('A reaction needs structures on both sides of each arrow.')
      species.push(group)
    } else {
      agents.push(group)
    }
  })
  return { species, agents }
}

/** Short, plain reasons for the SMILES mistakes models make most, used when RDKit gives only its raw log. */
export function explainSmiles(smiles: string, raw: string): string {
  const unclosed = /unclosed ring/i.test(raw)
  if (unclosed) {
    const digits = new Map<string, number>()
    for (const m of smiles.replace(/\[[^\]]*\]/g, 'X').matchAll(/%\d\d|\d/g)) digits.set(m[0], (digits.get(m[0]) ?? 0) + 1)
    const open = [...digits].filter(([, c]) => c % 2 === 1).map(([d]) => d)
    return `Unclosed ring: ring bond ${open.length ? open.join(', ') : 'digit'} is opened but never closed. Every ring digit must appear twice (C1CCCCC1).`
  }
  if (/extra open parentheses/i.test(raw)) return 'Unbalanced parentheses: a "(" is never closed.'
  if (/extra close parentheses/i.test(raw)) return 'Unbalanced parentheses: a ")" has no matching "(".'
  const pos = /position (\d+)/.exec(raw)
  const syntax = /syntax error/i.test(raw)
  if (/Failed parsing/.test(raw) || syntax) {
    return `Not valid SMILES${pos ? ` near character ${Number(pos[1]) + 1} ("${smiles.slice(Math.max(0, Number(pos[1]) - 1), Number(pos[1]) + 3)}")` : ''}. Check element symbols (inside [] for anything unusual), brackets and bond symbols.`
  }
  if (/valence|explicit valence/i.test(raw)) return `An atom has more bonds than its valence allows (${raw.split('\n').find((l) => /valence/i.test(l))?.replace(/^\[[^\]]*\]\s*/, '') ?? ''}).`
  if (/kekulize/i.test(raw)) return 'An aromatic ring can’t be kekulized: check which atoms are lowercase (aromatic) and whether they form a valid ring.'
  return raw.trim().split('\n').pop()?.replace(/^\[[^\]]*\]\s*/, '') || 'Invalid structure.'
}
