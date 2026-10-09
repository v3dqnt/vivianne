import { drawBio } from './bioDraw'
import { parseBio } from './bioDsl'
import { parseChem } from './chemDsl'
import { describeChem, renderChem } from './chemRender'
import { DslError, type SciColors } from './dslCommon'
import { compilePhysics } from './physicsDsl'
import { parseMol3D } from './mol3dDsl'
import { parsePlot } from './plotDsl'
import type { RDKit } from './rdkit'

/**
 * Validates a science script without drawing it on screen: the same parsers
 * and the same RDKit calls the chat blocks use, reduced to a short verdict
 * the agent can read. `check_diagram` answers with this.
 */

export type DiagramKind = 'chem' | 'mol3d' | 'bio' | 'plot' | 'physics'

const COLORS: SciColors = { ink: '#111111', muted: '#666666', line: '#dddddd', paper: '#ffffff', dark: false, accents: ['#eda100', '#e34948', '#2a78d6'] }

function invalid(code: string, e: unknown): string {
  if (e instanceof DslError) {
    const text = e.line ? code.replace(/\r\n?/g, '\n').split('\n')[e.line - 1] : undefined
    return `INVALID${e.line ? ` (line ${e.line})` : ''}: ${e.message}${text !== undefined ? `\n  ${e.line} | ${text}` : ''}`
  }
  return `INVALID: ${e instanceof Error ? e.message : String(e)}`
}

/** `rd` is the loaded RDKit, needed only for `chem`. */
export async function checkScript(kind: DiagramKind, code: string, rd?: RDKit): Promise<string> {
  try {
    switch (kind) {
      case 'chem': {
        const spec = parseChem(code)
        if (!rd) throw new Error('RDKit is not loaded.')
        const out = renderChem(rd, spec, COLORS)
        return `OK: ${out.molecules.length} structure${out.molecules.length === 1 ? '' : 's'} drawn (canonical SMILES, formula, molecular weight):\n${describeChem(out)}`
      }
      case 'bio': {
        const spec = parseBio(code)
        await drawBio(spec, COLORS)
        const what =
          spec.kind === 'sequence'
            ? `${spec.type} sequence of ${spec.seq.length} ${spec.type === 'protein' ? 'residues' : 'bases'}, ${spec.features.length} feature(s)`
            : spec.kind === 'plasmid'
              ? `plasmid ${spec.title}, ${spec.size} bp, ${spec.features.length} feature(s), ${spec.sites.length} site(s)`
              : spec.kind === 'tree'
                ? `${spec.layout} tree`
                : spec.kind === 'pathway'
                  ? `pathway with ${spec.nodes.length} node(s) and ${spec.edges.length} edge(s)`
                  : `${spec.cell} cell showing ${spec.show.join(', ')}`
        return `OK: ${what}.`
      }
      case 'plot': {
        const spec = parsePlot(code)
        const warnings: string[] = []
        const values = Object.fromEntries(spec.params.map((p) => [p.name, p.value]))
        const [x0, x1] = spec.x.domain ?? [-10, 10]
        for (const s of spec.series) {
          if (s.kind !== 'fn') continue
          let finite = 0
          for (let i = 0; i <= 40; i++) if (Number.isFinite(s.fn({ ...values, x: x0 + ((x1 - x0) * i) / 40 }))) finite++
          if (!finite) warnings.push(`line ${s.line}: "${s.src}" is not a number anywhere on x = ${x0}..${x1} (check the domain, log of a negative, division by zero).`)
        }
        return `OK: ${spec.series.length} series, ${spec.params.length} slider(s).${warnings.length ? `\nWARNING ${warnings.join('\nWARNING ')}` : ''}`
      }
      case 'physics': {
        const scene = compilePhysics(code)
        return `OK: ${scene.prims.length} drawing elements.${scene.notes.length ? `\n${scene.notes.join('\n')}` : ''}`
      }
      case 'mol3d': {
        const spec = parseMol3D(code)
        return `OK: ${spec.source.type === 'rcsb' ? `PDB ${spec.source.id} (fetched from RCSB when drawn; not checked offline)` : `inline ${spec.source.format.toUpperCase()}`}, ${spec.styles.length} style(s), ${spec.surfaces.length} surface(s), ${spec.labels.length} label(s).`
      }
    }
  } catch (e) {
    return invalid(code, e)
  }
}
