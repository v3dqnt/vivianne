import { create } from 'zustand'
import { call } from '../../../ipc'
import { useAgentStore } from '../../ai/store/agentStore'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'
import { contentToText, type AgentMessage } from '../../ai/lib/types'
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
  /** Duplicates a session under a new id and switches to it — mirrors
   * `loadSession`'s tail (backend `cmd_fork_session` already makes the fork
   * the live session server-side; the frontend just has to catch up). */
  forkSession: (id: string) => Promise<void>
  /** Build markdown of a session's transcript. Only the active session's
   * messages are in memory, so export reads those (client-side). */
  exportMarkdown: () => string
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
    // Persists the empty thread and returns its id, so the row shows up in
    // the sidebar right away (as "New chat") instead of only existing once
    // the first turn completes. It renames itself from the first message on
    // the next autosave.
    const id = await call<string>('cmd_new_session')
    set({ activeSessionId: id ?? null })
    // The agent store owns the live transcript; a fresh thread starts empty
    // rather than carrying the previous conversation's messages forward.
    useAgentStore.setState({ messages: [], streamingIndex: null })
    await get().refresh()
    // `cmd_new_session` resets the agent, which can also reset the backend's
    // workspace — the workspace selector otherwise keeps showing whatever
    // the previous thread had.
    void useWorkspaceStore.getState().refresh()
  },

  loadSession: async (id: string) => {
    await call('cmd_load_session', { id })
    set({ activeSessionId: id })
    // `cmd_load_session` only sets the backend's live `AgentState` — the
    // frontend has to separately fetch the messages it just loaded, same as
    // the old app's `loadMessages` step after a session switch.
    const messages = await call<AgentMessage[]>('cmd_agent_messages')
    useAgentStore.setState({ messages: messages ?? [], streamingIndex: null })
    // Same reason: the workspace selector reads a separately-cached store,
    // which `cmd_load_session` doesn't update on its own — without this it
    // keeps showing whichever thread's workspace was active before the
    // switch.
    void useWorkspaceStore.getState().refresh()
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

  forkSession: async (id: string) => {
    const newId = await call<string>('cmd_fork_session', { id })
    if (!newId) return
    set({ activeSessionId: newId })
    const messages = await call<AgentMessage[]>('cmd_agent_messages')
    useAgentStore.setState({ messages: messages ?? [], streamingIndex: null })
    await get().refresh()
  },

  exportMarkdown: () => {
    const session = get().sessions.find((s) => s.id === get().activeSessionId)
    const title = session?.name || 'Vivianne session'
    const messages = useAgentStore.getState().messages
    const lines: string[] = [`# ${title}`, '']
    for (const m of messages) {
      if (m.type === 'user') {
        lines.push('## You', '', contentToText(m.content), '')
      } else if (m.type === 'assistant') {
        lines.push('## Vivianne', '', contentToText(m.content), '')
      } else if (m.type === 'toolResult') {
        const toolText = contentToText(m.content)
        lines.push(`_tool: ${m.toolName}${m.isError ? ' (error)' : ''}_`, toolText ? `\n\`\`\`\n${toolText}\n\`\`\`` : '', '')
      }
    }
    return lines.join('\n')
  },
}))
