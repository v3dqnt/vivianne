import { create } from 'zustand'
import { call } from '../../../ipc'

/** Mirrors `agent::master::MasterInfo` (`rename_all = "camelCase"`). */
export interface MasterInfo {
  name: string
  charter: string
  charterPath: string
  created: string | null
}

interface MasterState {
  /** Project folder → its master, `null` when it has none. Absent = not
   * looked up yet, which the sidebar must not mistake for "has no master". */
  byRoot: Record<string, MasterInfo | null>
  load: (root: string) => Promise<MasterInfo | null>
}

export const useMasterStore = create<MasterState>((set) => ({
  byRoot: {},
  load: async (root) => {
    const info = (await call<MasterInfo | null>('cmd_get_master', { root }).catch(() => null)) ?? null
    set((s) => ({ byRoot: { ...s.byRoot, [root]: info } }))
    return info
  },
}))
