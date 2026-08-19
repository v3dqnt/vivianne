import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'
import { disposeThreadStore } from '../../ai/store/threadRegistry'

interface PersistedThreads {
  openIds: string[]
  focusedId: string | null
  /** threadId → that thread's workspace folders. A thread is bound to the
   * workspace it was working in, and that binding survives restarts — the
   * dock panels (files/preview/git/terminal) follow whichever thread is
   * focused rather than one app-global workspace. */
  workspaces: Record<string, string[]>
}

interface ThreadsStoreState extends PersistedThreads {
  open: (id: string) => void
  close: (id: string) => void
  focus: (id: string) => void
  /** Remember this thread's workspace folders (called when a thread loads or
   * its workspace changes). */
  setThreadWorkspace: (id: string, folders: string[]) => void
  /** The focused thread's workspace, or null when no thread pane is focused
   * (in which case callers fall back to the app-global workspace). */
  focusedWorkspace: () => string[] | null
}

const STORAGE_KEY = 'viv.threads'

export const useThreadsStore = create<ThreadsStoreState>((set, get) => ({
  ...loadPersisted<PersistedThreads>(STORAGE_KEY, { openIds: [], focusedId: null, workspaces: {} }),

  open: (id) => {
    const { openIds, workspaces } = get()
    const next = openIds.includes(id) ? openIds : [...openIds, id]
    savePersisted(STORAGE_KEY, { openIds: next, focusedId: id, workspaces })
    set({ openIds: next, focusedId: id })
  },

  close: (id) => {
    const { openIds, focusedId, workspaces } = get()
    const next = openIds.filter((x) => x !== id)
    const nextFocused = focusedId === id ? (next[next.length - 1] ?? null) : focusedId
    disposeThreadStore(id)
    savePersisted(STORAGE_KEY, { openIds: next, focusedId: nextFocused, workspaces })
    set({ openIds: next, focusedId: nextFocused })
  },

  focus: (id) => {
    const { openIds, workspaces } = get()
    savePersisted(STORAGE_KEY, { openIds, focusedId: id, workspaces })
    set({ focusedId: id })
  },

  setThreadWorkspace: (id, folders) => {
    const { openIds, focusedId, workspaces } = get()
    if (JSON.stringify(workspaces[id]) === JSON.stringify(folders)) return
    const next = { ...workspaces, [id]: folders }
    savePersisted(STORAGE_KEY, { openIds, focusedId, workspaces: next })
    set({ workspaces: next })
  },

  focusedWorkspace: () => {
    const { focusedId, workspaces } = get()
    if (!focusedId) return null
    return workspaces[focusedId] ?? null
  },
}))
