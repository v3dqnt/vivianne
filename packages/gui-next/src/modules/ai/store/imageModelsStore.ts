import { create } from 'zustand'
import { call, on } from '../../../ipc'

export interface CatalogModel {
  id: string
  name: string
  file: string
  url: string
  gb: number
  note: string
}

interface Status {
  installed: boolean
  path: string | null
  models: { file: string; bytes: number }[]
  catalog: CatalogModel[]
}

interface ImageModelsState extends Status {
  loaded: boolean
  /** Percent, while the sd program is being downloaded. */
  installing: number | null
  /** Percent per model file being downloaded. */
  downloading: Record<string, number>
  error: string | null
  refresh: () => Promise<void>
  install: () => Promise<void>
  download: (id: string) => Promise<void>
  remove: (file: string) => Promise<void>
}

let listening = false
const pct = (p: { received: number; total: number }) => (p.total ? Math.round((p.received / p.total) * 100) : 0)
const message = (e: unknown) => (e instanceof Error ? e.message : String(e))

/** Local image generation: the stable-diffusion.cpp program, and the models it runs. */
export const useImageModels = create<ImageModelsState>((set, get) => ({
  installed: false,
  path: null,
  models: [],
  catalog: [],
  loaded: false,
  installing: null,
  downloading: {},
  error: null,

  refresh: async () => {
    if (!listening) {
      listening = true
      void on<{ received: number; total: number }>('local-images:sd', (p) => set({ installing: pct(p) }))
      void on<{ file: string; received: number; total: number }>('local-images:progress', (p) => set((s) => ({ downloading: { ...s.downloading, [p.file]: pct(p) } })))
    }
    const status = await call<Status>('cmd_sd_status')
    if (status) set({ ...status, loaded: true })
  },

  install: async () => {
    set({ installing: 0, error: null })
    try {
      await call('cmd_sd_install')
    } catch (e) {
      set({ error: `Couldn't install stable-diffusion.cpp: ${message(e)}` })
    } finally {
      set({ installing: null })
      await get().refresh()
    }
  },

  download: async (id) => {
    const m = get().catalog.find((c) => c.id === id)
    if (!m) return
    set((s) => ({ downloading: { ...s.downloading, [m.file]: 0 }, error: null }))
    try {
      await call('cmd_sd_model_download', { id })
    } catch (e) {
      if (message(e) !== 'Cancelled') set({ error: `Download failed: ${message(e)}` })
    } finally {
      set((s) => {
        const { [m.file]: _done, ...rest } = s.downloading
        return { downloading: rest }
      })
      await get().refresh()
    }
  },

  remove: async (file) => {
    try {
      await call('cmd_sd_model_delete', { file })
    } catch (e) {
      set({ error: message(e) })
    }
    await get().refresh()
  },
}))
