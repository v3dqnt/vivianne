import { create } from 'zustand'

interface FilesState {
  /** Folder paths whose children are showing. */
  expanded: Set<string>
  /** The one file expanded inline under its row. */
  openFile: string | null
  /** A row to scroll to and flash after `revealFile`. */
  flash: string | null
  toggleFolder: (path: string) => void
  expandFolders: (paths: string[]) => void
  setOpenFile: (path: string | null) => void
  flashFile: (path: string | null) => void
}

export const useFilesStore = create<FilesState>((set) => ({
  expanded: new Set(),
  openFile: null,
  flash: null,
  toggleFolder: (path) =>
    set((s) => {
      const next = new Set(s.expanded)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return { expanded: next }
    }),
  expandFolders: (paths) =>
    set((s) => {
      const next = new Set(s.expanded)
      for (const p of paths) next.add(p)
      return { expanded: next }
    }),
  setOpenFile: (openFile) => set({ openFile }),
  flashFile: (flash) => set({ flash }),
}))
