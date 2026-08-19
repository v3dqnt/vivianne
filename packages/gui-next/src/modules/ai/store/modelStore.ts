import { create } from 'zustand'
import { call } from '../../../ipc'
import { preconnectProvider } from '../providers/shared'
import type { DiscoveredModel, SessionStats, ThinkingLevel } from '../lib/types'

/** `cmd_session_cost`'s return shape — total `$` attributed to the active session. */
export interface SessionCost {
  cost: number
  inputTokens: number
  outputTokens: number
  requests: number
}

/** A `DiscoveredModel` plus an optional base URL — every model the selector
 * shows is one of these; only custom-provider and local entries carry a
 * `baseUrl` (cloud providers resolve their own well-known endpoint
 * server-side, local resolves from the endpoint registry). */
export type SelectableModel = DiscoveredModel & { baseUrl?: string }

interface ModelStoreState {
  models: DiscoveredModel[]
  stats: SessionStats | null
  sessionCost: SessionCost | null
  thinkingLevel: ThinkingLevel
  loading: boolean
  initialized: boolean
  init: () => Promise<void>
  selectModel: (model: SelectableModel) => Promise<void>
  setThinkingLevel: (level: ThinkingLevel) => Promise<void>
  refreshStats: () => Promise<void>
}

async function fetchSessionCost(): Promise<SessionCost | null> {
  const sessionId = await call<string | null>('cmd_get_active_session_id')
  return call<SessionCost>('cmd_session_cost', { sessionId: sessionId ?? null })
}

export const useModelStore = create<ModelStoreState>((set, get) => ({
  models: [],
  stats: null,
  sessionCost: null,
  thinkingLevel: 'low',
  loading: false,
  initialized: false,

  init: async () => {
    if (get().initialized) return
    set({ initialized: true, loading: true })
    try {
      const [models, stats, sessionCost] = await Promise.all([
        call<DiscoveredModel[]>('cmd_list_available_models'),
        call<SessionStats>('cmd_agent_session_stats'),
        fetchSessionCost(),
      ])
      set({ models: models ?? [], stats: stats ?? null, sessionCost })
      if (stats?.provider) preconnectProvider(stats.provider)
    } finally {
      set({ loading: false })
    }
  },

  selectModel: async (model) => {
    preconnectProvider(model.provider)
    await call('cmd_agent_set_model', {
      id: model.id,
      name: model.name,
      api: model.api,
      provider: model.provider,
      baseUrl: model.baseUrl ?? null,
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
    const [stats, sessionCost] = await Promise.all([call<SessionStats>('cmd_agent_session_stats'), fetchSessionCost()])
    set({ stats: stats ?? null, sessionCost })
  },
}))
