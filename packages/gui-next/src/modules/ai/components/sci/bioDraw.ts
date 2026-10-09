import type { BioSpec } from './bioDsl'
import { cellSvg } from './bioCell'
import { plasmidSvg, sequenceSvg, treeSvg } from './bioRender'
import type { SciColors } from './dslCommon'

/** Draw a parsed biology script as an SVG string (the pathway layout is async). */
export async function drawBio(spec: BioSpec, colors: SciColors): Promise<string> {
  switch (spec.kind) {
    case 'sequence':
      return sequenceSvg(spec, colors)
    case 'plasmid':
      return plasmidSvg(spec, colors)
    case 'tree':
      return treeSvg(spec, colors)
    case 'cell':
      return cellSvg(spec, colors)
    case 'pathway': {
      const { layoutPathway, pathwaySvg } = await import('./bioPathway')
      return pathwaySvg(spec, await layoutPathway(spec), colors)
    }
  }
}
