import { create } from 'zustand'
import { call } from '../../../ipc'

export type WorkspaceAccessMode = 'full' | 'readOnly'

interface WorkspaceGetResult {
  folders: string[]
  accessMode: WorkspaceAccessMode
}

interface WorkspaceStoreState {
  folders: string[]
  accessMode: WorkspaceAccessMode
  /** Git branch of the primary (first) folder, or null if it isn't a repo. */
  branch: string | null
  loading: boolean
  refresh: () => Promise<void>
  setWorkspace: (folders: string[], accessMode: WorkspaceAccessMode) => Promise<void>
  addExistingFolder: () => Promise<void>
  createFolder: (name: string) => Promise<void>
  removeFolder: (path: string) => Promise<void>
  toggleAccessMode: () => Promise<void>
}

async function refreshBranch(primaryFolder: string | undefined, set: (partial: Partial<WorkspaceStoreState>) => void) {
  if (!primaryFolder) {
    set({ branch: null })
    return
  }
  const branch = await call<string | null>('cmd_git_current_branch', { path: primaryFolder })
  set({ branch: branch ?? null })
}

export const useWorkspaceStore = create<WorkspaceStoreState>((set, get) => ({
  folders: [],
  accessMode: 'full',
  branch: null,
  loading: false,

  refresh: async () => {
    const res = await call<WorkspaceGetResult>('cmd_agent_get_workspace')
    if (res) set({ folders: res.folders, accessMode: res.accessMode })
    void refreshBranch(res?.folders[0], set)
  },

  setWorkspace: async (folders, accessMode) => {
    set({ loading: true })
    try {
      await call('cmd_agent_set_workspace', { folders, accessMode })
      set({ folders, accessMode })
      void refreshBranch(folders[0], set)
    } finally {
      set({ loading: false })
    }
  },

  addExistingFolder: async () => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({ directory: true, multiple: false })
    if (!selected || Array.isArray(selected)) return
    const { folders, accessMode } = get()
    if (folders.includes(selected)) return
    await get().setWorkspace([...folders, selected], accessMode)
  },

  createFolder: async (name) => {
    const trimmed = name.trim()
    if (!trimmed) return
    const { open } = await import('@tauri-apps/plugin-dialog')
    const parent = await open({ directory: true, multiple: false })
    if (!parent || Array.isArray(parent)) return
    const created = await call<string>('cmd_create_workspace_folder', { parent, name: trimmed })
    if (!created) return
    const { folders, accessMode } = get()
    await get().setWorkspace([...folders, created], accessMode)
  },

  removeFolder: async (path) => {
    const { folders, accessMode } = get()
    await get().setWorkspace(
      folders.filter((f) => f !== path),
      accessMode,
    )
  },

  toggleAccessMode: async () => {
    const { folders, accessMode } = get()
    await get().setWorkspace(folders, accessMode === 'full' ? 'readOnly' : 'full')
  },
}))
