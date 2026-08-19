import { create } from 'zustand'
import { call } from '../../../ipc'

interface WebPreviewResult {
  screenshotBase64: string | null
}

interface WebPreviewStoreState {
  open: boolean
  url: string
  screenshotBase64: string | null
  error: string | null
  loading: boolean
  /** Set by `agentStore` when the `web_preview` tool fires — opens the panel. */
  requested: (url: string, screenshotBase64: string | null, error: string | null) => void
  close: () => void
  navigate: (url: string) => Promise<void>
  back: () => Promise<void>
  forward: () => Promise<void>
  refresh: () => Promise<void>
}

export const useWebPreviewStore = create<WebPreviewStoreState>((set, get) => ({
  open: false,
  url: '',
  screenshotBase64: null,
  error: null,
  loading: false,

  requested: (url, screenshotBase64, error) => {
    set({ open: true, url, screenshotBase64, error })
  },

  close: () => set({ open: false }),

  navigate: async (url) => {
    set({ loading: true, error: null })
    try {
      const result = await call<WebPreviewResult>('cmd_web_preview_navigate', { url })
      set({ url, screenshotBase64: result?.screenshotBase64 ?? get().screenshotBase64 })
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
    } finally {
      set({ loading: false })
    }
  },

  back: async () => {
    set({ loading: true })
    try {
      const result = await call<WebPreviewResult>('cmd_web_preview_back')
      if (result?.screenshotBase64) set({ screenshotBase64: result.screenshotBase64 })
    } finally {
      set({ loading: false })
    }
  },

  forward: async () => {
    set({ loading: true })
    try {
      const result = await call<WebPreviewResult>('cmd_web_preview_forward')
      if (result?.screenshotBase64) set({ screenshotBase64: result.screenshotBase64 })
    } finally {
      set({ loading: false })
    }
  },

  refresh: async () => {
    set({ loading: true })
    try {
      const result = await call<WebPreviewResult>('cmd_web_preview_refresh')
      if (result?.screenshotBase64) set({ screenshotBase64: result.screenshotBase64 })
    } finally {
      set({ loading: false })
    }
  },
}))
