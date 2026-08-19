import { create } from 'zustand'
import { call } from '../../../ipc'
import type { AgentMessage } from '../lib/types'

/** Mirrors `coding_agent::commands::SubagentSummary` — `#[serde(rename_all = "camelCase")]`. */
export interface SubagentSummary {
  id: string
  subagentType: string
  description: string
  prompt: string
  cwd: string
  status: string
  startedAtEpochMs: number
  elapsedMs: number
  turns: number
  toolCalls: number
  output: string | null
  error: string | null
}

interface SubagentStoreState {
  subagents: SubagentSummary[]
  refresh: () => Promise<void>
  kill: (id: string) => Promise<void>
  /** A subagent's own transcript — in-memory on the backend, so this reads
   * fresh each call rather than being cached in this store (cheap, and the
   * transcript view polls it live while the subagent is still running). */
  getMessages: (id: string) => Promise<AgentMessage[]>
}

let pollHandle: ReturnType<typeof setInterval> | null = null

export const useSubagentStore = create<SubagentStoreState>((set, get) => ({
  subagents: [],

  refresh: async () => {
    const list = (await call<SubagentSummary[]>('cmd_coding_list_subagents')) ?? []
    set({ subagents: list })
    // Poll while any are still running, same pattern as `llamaStore` —
    // catches completion/failure without waiting on the next tool event.
    const active = list.some((a) => a.status === 'running')
    if (active && !pollHandle) pollHandle = setInterval(() => void get().refresh(), 2000)
    else if (!active && pollHandle) {
      clearInterval(pollHandle)
      pollHandle = null
    }
  },

  kill: async (id) => {
    await call('cmd_coding_kill_subagent', { id })
    await get().refresh()
  },

  getMessages: async (id) => {
    return (await call<AgentMessage[]>('cmd_coding_get_subagent_messages', { id })) ?? []
  },
}))
