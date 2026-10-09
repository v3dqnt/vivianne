import { create } from 'zustand'
import { call } from '../../../ipc'
import { useTeamStore } from './teamStore'

/**
 * The General tab's agents live in a folder of their own
 * (`bots::general_root`), which everything else treats as their project:
 * agents, chats, memory, routines, rules, email and phone.
 */
interface GeneralAgentsState {
  root: string | null
  ensure: () => Promise<string | null>
}

export const useGeneralAgents = create<GeneralAgentsState>((set, get) => ({
  root: null,
  ensure: async () => {
    const known = get().root
    if (known) return known
    const root = (await call<string>('cmd_general_agents_root').catch(() => null)) ?? null
    if (root) {
      set({ root })
      void useTeamStore.getState().load(root)
    }
    return root
  },
}))
