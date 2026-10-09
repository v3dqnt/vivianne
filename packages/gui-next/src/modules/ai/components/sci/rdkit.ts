import type { MainModule } from '@rdkit/rdkit'

/**
 * RDKit (the C++ chemistry toolkit, built to WebAssembly) behind one lazy
 * loader. The 7MB module is fetched the first time a chemistry block or a
 * `check_diagram` call needs it and shared after that; nothing in the app
 * loads it otherwise. It ships inside the app — no CDN at runtime.
 */
export type RDKit = MainModule

let loading: Promise<RDKit> | null = null

export function loadRDKit(): Promise<RDKit> {
  if (!loading) {
    loading = (async () => {
      const [{ default: init }, wasm] = await Promise.all([import('@rdkit/rdkit'), import('@rdkit/rdkit/RDKit_minimal.wasm?url')])
      const rd = await init({ locateFile: () => wasm.default })
      // CoordGen lays rings out the way a chemist would draw them.
      rd.prefer_coordgen(true)
      return rd
    })().catch((e) => {
      loading = null
      throw e
    })
  }
  return loading
}
