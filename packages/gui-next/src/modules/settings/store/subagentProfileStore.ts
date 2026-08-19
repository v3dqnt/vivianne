import { create } from 'zustand'
import { call } from '../../../ipc'
import type { SubagentModelOverride, SubagentProfile } from '../lib/subagentProfiles'

export interface SaveSubagentProfileInput {
  originalId: string | null
  name: string
  color: string | null
  model: SubagentModelOverride | null
  description: string
  allowedTools: string[] | null
  systemPrompt: string
  injectAgentsMd: boolean
}

interface SubagentProfileStoreState {
  builtin: SubagentProfile[]
  custom: SubagentProfile[]
  loading: boolean
  refresh: () => Promise<void>
  saveProfile: (input: SaveSubagentProfileInput) => Promise<string>
  removeProfile: (id: string) => Promise<void>
}

export const useSubagentProfileStore = create<SubagentProfileStoreState>((set, get) => ({
  builtin: [],
  custom: [],
  loading: false,

  refresh: async () => {
    set({ loading: true })
    try {
      const result = await call<{ builtin: SubagentProfile[]; custom: SubagentProfile[] }>('cmd_list_subagent_profiles')
      set({ builtin: result?.builtin ?? [], custom: result?.custom ?? [] })
    } finally {
      set({ loading: false })
    }
  },

  saveProfile: async (input) => {
    const id = await call<string>('cmd_save_subagent_profile', { ...input })
    await get().refresh()
    return id ?? ''
  },

  removeProfile: async (id) => {
    await call('cmd_delete_subagent_profile', { id })
    set((s) => ({ custom: s.custom.filter((p) => p.id !== id) }))
  },
}))
