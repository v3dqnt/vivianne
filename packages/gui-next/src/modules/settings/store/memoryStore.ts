import { create } from 'zustand'
import { call } from '../../../ipc'
import type { HindsightConfig, MemoryEntry, MemoryState, MemoryStatus, SleepReport } from '../lib/memory'

interface MemoryStoreState {
  status: MemoryStatus | null
  recallResults: MemoryEntry[]
  loading: boolean
  refresh: () => Promise<void>
  toggleEnabled: () => Promise<void>
  toggleAutoConsolidate: () => Promise<void>
  toggleExtraction: () => Promise<void>
  purge: () => Promise<void>
  setBlockSecrets: (block: boolean) => Promise<void>
  setPages: (pages: { name: string; question: string }[]) => Promise<string | null>
  hindsight: HindsightConfig | null
  loadHindsight: () => Promise<void>
  saveHindsight: (config: HindsightConfig, apiKey?: string) => Promise<string | null>
  testHindsight: (config: HindsightConfig) => Promise<string>
  recall: (query: string) => Promise<void>
  /** Run a tidy-up pass now instead of waiting for the app to go idle. */
  sleepNow: () => Promise<SleepReport | null>
}

export const useMemoryStore = create<MemoryStoreState>((set, get) => ({
  status: null,
  recallResults: [],
  loading: false,
  hindsight: null,

  setBlockSecrets: async (block) => {
    const state = await call<MemoryState>('cmd_memory_configure_defense', { blockSecrets: block })
    if (state) set((s) => (s.status ? { status: { ...s.status, state } } : s))
  },

  /** Returns an error message, or null when saved. */
  setPages: async (pages) => {
    try {
      const state = await call<MemoryState>('cmd_memory_configure_pages', { pages })
      if (state) set((s) => (s.status ? { status: { ...s.status, state } } : s))
      return null
    } catch (e) {
      return String(e)
    }
  },

  loadHindsight: async () => {
    const h = await call<HindsightConfig>('cmd_hindsight_get')
    set({ hindsight: h ?? null })
  },

  saveHindsight: async (config, apiKey) => {
    try {
      const h = await call<HindsightConfig>('cmd_hindsight_set', { config: { enabled: config.enabled, url: config.url, bank: config.bank, retain: config.retain }, apiKey })
      set({ hindsight: h ?? null })
      return null
    } catch (e) {
      return String(e)
    }
  },

  testHindsight: async (config) => {
    try {
      return (await call<string>('cmd_hindsight_test', { config: { enabled: true, url: config.url, bank: config.bank, retain: config.retain } })) ?? 'No answer.'
    } catch (e) {
      return String(e)
    }
  },

  refresh: async () => {
    set({ loading: true })
    try {
      const status = await call<MemoryStatus>('cmd_memory_status')
      set({ status: status ?? null })
    } finally {
      set({ loading: false })
    }
  },

  toggleEnabled: async () => {
    const enabled = get().status?.state.enabled
    const state = await call<MemoryState>(enabled ? 'cmd_memory_disable' : 'cmd_memory_enable')
    if (state) set((s) => (s.status ? { status: { ...s.status, state } } : s))
    await get().refresh()
  },

  toggleAutoConsolidate: async () => {
    const next = !get().status?.state.autoConsolidate
    const state = await call<MemoryState>('cmd_memory_configure', { autoConsolidate: next })
    if (state) set((s) => (s.status ? { status: { ...s.status, state } } : s))
  },

  toggleExtraction: async () => {
    const next = !get().status?.state.extractionEnabled
    const state = await call<MemoryState>('cmd_memory_configure_extraction', { enabled: next })
    if (state) set((s) => (s.status ? { status: { ...s.status, state } } : s))
  },

  purge: async () => {
    await call('cmd_memory_purge')
    set({ recallResults: [] })
    await get().refresh()
  },

  recall: async (query) => {
    if (!query.trim()) {
      set({ recallResults: [] })
      return
    }
    const results = await call<MemoryEntry[]>('cmd_memory_recall', { query })
    set({ recallResults: results ?? [] })
  },

  sleepNow: async () => {
    const report = await call<SleepReport>('cmd_memory_sleep_now')
    await get().refresh()
    return report ?? null
  },
}))
