import type { SciColors } from './dslCommon'
import type { Mol3DSpec, Selection } from './mol3dDsl'

/**
 * Draws a ```mol3d script with 3Dmol.js (loaded the first time one appears).
 * The structure comes from RCSB (`files.rcsb.org`, allowed by the app's
 * content-security policy) or from the script itself.
 */

export interface Mol3DHandle {
  png(): string
  destroy(): void
}

const cache = new Map<string, Promise<string>>()

/** A PDB entry's file, fetched once per session. */
export function fetchPdb(id: string): Promise<string> {
  let p = cache.get(id)
  if (!p) {
    p = fetch(`https://files.rcsb.org/download/${id}.pdb`).then(async (r) => {
      if (r.status === 404) throw new Error(`RCSB has no entry "${id}". Check the PDB id.`)
      if (!r.ok) throw new Error(`RCSB answered ${r.status} for ${id}.`)
      return r.text()
    })
    // A failure is not cached: the next try may be online.
    p.catch(() => cache.delete(id))
    cache.set(id, p)
  }
  return p
}

function colorArgs(color: string | undefined): Record<string, unknown> {
  if (!color) return {}
  switch (color.toLowerCase()) {
    case 'spectrum':
      return { color: 'spectrum' }
    case 'chain':
      return { colorscheme: 'chain' }
    case 'ss':
      return { colorscheme: 'ssPyMol' }
    case 'residue':
    case 'amino':
      return { colorscheme: 'amino' }
    case 'nucleic':
      return { colorscheme: 'nucleic' }
    case 'element':
    case 'jmol':
      return { colorscheme: 'Jmol' }
    case 'bfactor':
      return { colorscheme: { prop: 'b', gradient: 'rwb', min: 90, max: 30 } }
    default:
      return { color }
  }
}

const sel = (s: Selection): Record<string, unknown> => ({ ...s })

export async function drawMol3D(root: HTMLElement, spec: Mol3DSpec, c: SciColors, data: string): Promise<Mol3DHandle> {
  const $3Dmol = await import('3dmol')
  const bg = spec.background === 'transparent' ? null : spec.background
  const viewer = $3Dmol.createViewer(root, { backgroundColor: bg ?? undefined, backgroundAlpha: bg ? 1 : 0, antialias: true })
  const format = spec.source.type === 'rcsb' ? 'pdb' : spec.source.format
  viewer.addModel(data, format)
  viewer.setStyle({}, {})
  for (const s of spec.styles) {
    // Opacity belongs to the representation; width and radius vary by kind.
    const common: Record<string, unknown> = { ...colorArgs(s.color), ...(s.opacity !== undefined ? { opacity: s.opacity } : {}) }
    const style: Record<string, unknown> = s.kind === 'stick' ? { radius: s.radius ?? 0.15 } : s.kind === 'sphere' ? { scale: s.radius ?? 0.3 } : s.kind === 'line' ? { linewidth: s.radius ?? 1.5 } : {}
    viewer.addStyle(sel(s.sel), { [s.kind]: { ...style, ...common } })
  }
  for (const s of spec.surfaces) {
    viewer.addSurface($3Dmol.SurfaceType.VDW, { opacity: s.opacity, ...(s.color ? colorArgs(s.color) : { colorscheme: 'whiteCarbon' }) }, sel(s.sel))
  }
  for (const l of spec.labels) {
    viewer.addLabel(
      l.text,
      { fontSize: 13, fontColor: c.ink, backgroundColor: c.paper, backgroundOpacity: 0.85, borderColor: c.line, borderThickness: 1, showBackground: true, alignment: 'center' },
      sel(l.sel),
    )
  }
  viewer.zoomTo()
  viewer.render()
  if (spec.spin && !window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) viewer.spin('y', 0.6)
  return {
    png: () => viewer.pngURI(),
    destroy() {
      try {
        viewer.spin(false)
        viewer.clear()
        // Release the WebGL context now rather than at garbage collection: the browser allows only a few at once.
        const canvas = viewer.getCanvas()
        const gl = (canvas?.getContext('webgl2') ?? canvas?.getContext('webgl')) as WebGLRenderingContext | null
        gl?.getExtension('WEBGL_lose_context')?.loseContext()
      } catch {
        // Already torn down.
      }
      root.remove()
    },
  }
}
