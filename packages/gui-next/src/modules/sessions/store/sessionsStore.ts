import { create } from 'zustand'
import { call } from '../../../ipc'
import { useAgentStore } from '../../ai/store/agentStore'
import type { AgentMessage } from '../../ai/lib/types'
import type { SessionInfo } from '../lib/types'

interface SessionsStoreState {
  sessions: SessionInfo[]
  activeSessionId: string | null
  loading: boolean

  refresh: () => Promise<void>
  refreshActiveId: () => Promise<void>
  newChat: () => Promise<void>
  loadSession: (id: string) => Promise<void>
  deleteSession: (id: string) => Promise<void>
  renameSession: (id: string, name: string) => Promise<void>
}

export const useSessionsStore = create<SessionsStoreState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  loading: false,

  refresh: async () => {
    set({ loading: true })
    try {
      const list = await call<SessionInfo[]>('cmd_get_sessions')
      set({ sessions: list ?? [] })
    } finally {
      set({ loading: false })
    }
  },

  refreshActiveId: async () => {
    const id = await call<string | null>('cmd_get_active_session_id')
    set({ activeSessionId: id ?? null })
  },

  newChat: async () => {
    await call('cmd_agent_reset')
    set({ activeSessionId: null })
    // The agent store owns the live transcript; a fresh thread starts empty
    // rather than carrying the previous conversation's messages forward.
    useAgentStore.setState({ messages: [], streamingIndex: null })
  },

  loadSession: async (id: string) => {
    await call('cmd_load_session', { id })
    set({ activeSessionId: id })
    // `cmd_load_session` only sets the backend's live `AgentState` — the
    // frontend has to separately fetch the messages it just loaded, same as
    // the old app's `loadMessages` step after a session switch.
    const messages = await call<AgentMessage[]>('cmd_agent_messages')
    useAgentStore.setState({ messages: messages ?? [], streamingIndex: null })
  },

  deleteSession: async (id: string) => {
    await call('cmd_delete_session', { id })
    if (get().activeSessionId === id) {
      set({ activeSessionId: null })
      useAgentStore.setState({ messages: [], streamingIndex: null })
    }
    await get().refresh()
  },

  renameSession: async (id: string, name: string) => {
    await call('cmd_rename_session', { id, name })
    await get().refresh()
  },
}))
