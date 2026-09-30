import { create } from 'zustand'
import { call } from '../../../ipc'

export interface IdeTab {
  path: string
  /** What's actually on disk, as of the last read or save — the diff base
   * for `dirty` and what a "discard changes" reload falls back to. */
  savedContent: string
  /** The editor's live buffer. Separate from `savedContent` so typing never
   * needs a round-trip, and so `dirty` is a cheap reference/length check
   * rather than a re-read from the DOM. */
  draft: string
  loading: boolean
  /** Set when this tab failed to load (binary file, permission error, gone
   * from disk) — shown in place of the editor rather than an empty buffer
   * that would silently save as a truncation of nothing. */
  error: string | null
  /** The agent (or something else) wrote this file while it was open here.
   * Cleared by reloading or by saving over it (an explicit, informed choice
   * either way — never cleared by silently picking one for the user). */
  changedOnDisk: boolean
}

interface IdeStoreState {
  tabs: IdeTab[]
  activePath: string | null
  openFile: (path: string) => Promise<void>
  closeTab: (path: string) => void
  setActive: (path: string) => void
  setDraft: (path: string, draft: string) => void
  save: (path: string) => Promise<void>
  reload: (path: string) => Promise<void>
  markChangedOnDisk: (path: string) => void
}

function isDirty(tab: IdeTab): boolean {
  return tab.draft !== tab.savedContent
}

export const useIdeStore = create<IdeStoreState>((set, get) => ({
  tabs: [],
  activePath: null,

  openFile: async (path) => {
    const existing = get().tabs.find((t) => t.path === path)
    if (existing) {
      set({ activePath: path })
      return
    }
    // Inserted immediately in a loading state so clicking a file in the tree
    // opens its tab right away — the read happens underneath a spinner
    // instead of leaving the click feeling unresponsive.
    set((s) => ({
      tabs: [...s.tabs, { path, savedContent: '', draft: '', loading: true, error: null, changedOnDisk: false }],
      activePath: path,
    }))
    try {
      const content = await call<string>('cmd_read_file_raw', { path })
      set((s) => ({
        tabs: s.tabs.map((t) => (t.path === path ? { ...t, savedContent: content ?? '', draft: content ?? '', loading: false } : t)),
      }))
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      set((s) => ({ tabs: s.tabs.map((t) => (t.path === path ? { ...t, loading: false, error: msg } : t)) }))
    }
  },

  closeTab: (path) => {
    set((s) => {
      const tabs = s.tabs.filter((t) => t.path !== path)
      const activePath =
        s.activePath === path ? (tabs[tabs.length - 1]?.path ?? null) : s.activePath
      return { tabs, activePath }
    })
  },

  setActive: (path) => set({ activePath: path }),

  setDraft: (path, draft) => {
    set((s) => ({ tabs: s.tabs.map((t) => (t.path === path ? { ...t, draft } : t)) }))
  },

  save: async (path) => {
    const tab = get().tabs.find((t) => t.path === path)
    if (!tab || tab.loading) return
    await call('cmd_write_text_file', { path, content: tab.draft })
    set((s) => ({
      tabs: s.tabs.map((t) => (t.path === path ? { ...t, savedContent: t.draft, changedOnDisk: false } : t)),
    }))
  },

  reload: async (path) => {
    const existing = get().tabs.find((t) => t.path === path)
    if (!existing) return
    set((s) => ({ tabs: s.tabs.map((t) => (t.path === path ? { ...t, loading: true, error: null } : t)) }))
    try {
      const content = await call<string>('cmd_read_file_raw', { path })
      set((s) => ({
        tabs: s.tabs.map((t) =>
          t.path === path ? { ...t, savedContent: content ?? '', draft: content ?? '', loading: false, changedOnDisk: false } : t,
        ),
      }))
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      set((s) => ({ tabs: s.tabs.map((t) => (t.path === path ? { ...t, loading: false, error: msg } : t)) }))
    }
  },

  markChangedOnDisk: (path) => {
    set((s) => ({ tabs: s.tabs.map((t) => (t.path === path ? { ...t, changedOnDisk: true } : t)) }))
  },
}))

export { isDirty }
