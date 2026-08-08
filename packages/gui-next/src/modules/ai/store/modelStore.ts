import { create } from 'zustand'
import { call } from '../../../ipc'
import type { DiscoveredModel, SessionStats, ThinkingLevel } from '../lib/types'

interface ModelStoreState {
  models: DiscoveredModel[]
  stats: SessionStats | null
  thinkingLevel: ThinkingLevel
  loading: boolean
  initialized: boolean
  init: () => Promise<void>
  selectModel: (model: DiscoveredModel) => Promise<void>
  setThinkingLevel: (level: ThinkingLevel) => Promise<void>
  refreshStats: () => Promise<void>
}

export const useModelStore = create<ModelStoreState>((set, get) => ({
  models: [],
  stats: null,
  thinkingLevel: 'low',
  loading: false,
  initialized: false,

  init: async () => {
    if (get().initialized) return
    set({ initialized: true, loading: true })
    try {
      const [models, stats] = await Promise.all([
        call<DiscoveredModel[]>('cmd_list_available_models'),
        call<SessionStats>('cmd_agent_session_stats'),
      ])
      set({ models: models ?? [], stats: stats ?? null })
    } finally {
      set({ loading: false })
    }
  },

  selectModel: async (model) => {
    await call('cmd_agent_set_model', {
      id: model.id,
      name: model.name,
      api: model.api,
      provider: model.provider,
      contextWindow: model.contextWindow,
      maxTokens: model.maxOutputTokens,
    })
    await get().refreshStats()
  },

  setThinkingLevel: async (level) => {
    set({ thinkingLevel: level })
    await call('cmd_agent_set_thinking_level', { level })
  },

  refreshStats: async () => {
    const stats = await call<SessionStats>('cmd_agent_session_stats')
    set({ stats: stats ?? null })
  },
}))
