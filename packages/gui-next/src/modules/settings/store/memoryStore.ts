import { create } from 'zustand'
import { call } from '../../../ipc'
import type { MemoryEntry, MemoryState, MemoryStatus } from '../lib/memory'

interface MemoryStoreState {
  status: MemoryStatus | null
  recallResults: MemoryEntry[]
  loading: boolean
  refresh: () => Promise<void>
  toggleEnabled: () => Promise<void>
  toggleAutoConsolidate: () => Promise<void>
  toggleExtraction: () => Promise<void>
  purge: () => Promise<void>
  recall: (query: string) => Promise<void>
}

export const useMemoryStore = create<MemoryStoreState>((set, get) => ({
  status: null,
  recallResults: [],
  loading: false,

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
}))
