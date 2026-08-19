import { create } from 'zustand'
import { call } from '../../../ipc'
import type { PlaybookDef } from '../lib/playbooks'

interface PlaybookStoreState {
  playbooks: PlaybookDef[]
  refresh: () => Promise<void>
  pickPlaybookFile: () => Promise<string | null>
  addFromFile: (path: string) => Promise<void>
  remove: (name: string) => Promise<void>
}

export const usePlaybookStore = create<PlaybookStoreState>((set, get) => ({
  playbooks: [],

  refresh: async () => {
    const list = (await call<PlaybookDef[]>('cmd_list_playbooks')) ?? []
    set({ playbooks: list })
  },

  pickPlaybookFile: async () => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({ multiple: false, filters: [{ name: 'Playbook', extensions: ['json'] }] })
    return !selected || Array.isArray(selected) ? null : selected
  },

  addFromFile: async (path) => {
    await call('cmd_add_playbook_file', { path })
    await get().refresh()
  },

  remove: async (name) => {
    await call('cmd_remove_playbook', { name })
    set({ playbooks: get().playbooks.filter((p) => p.name !== name) })
  },
}))
