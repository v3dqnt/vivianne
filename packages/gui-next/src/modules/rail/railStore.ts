import { create } from 'zustand'

/** What the project rail shows: its overview (project, goal, version control, context), or one of the working tabs. */
export type RailTab = 'overview' | 'files' | 'preview' | 'terminal' | 'debug'

/** The working tabs need room, so the rail widens while one is open; the overview keeps its compact card. */
export const isWide = (tab: RailTab) => tab !== 'overview'

const KEY = 'viv.rail.tab'
const read = (): RailTab => {
  try {
    const v = localStorage.getItem(KEY)
    return v === 'files' || v === 'terminal' ? v : 'overview'
  } catch {
    return 'overview'
  }
}

interface RailState {
  tab: RailTab
  /** The file the Preview tab shows. */
  previewPath: string | null
  setTab: (tab: RailTab) => void
  /** Open a file in the Preview tab. */
  preview: (path: string) => void
}

export const useRail = create<RailState>((set) => ({
  tab: typeof localStorage === 'undefined' ? 'overview' : read(),
  previewPath: null,
  setTab: (tab) => {
    try {
      // A preview depends on a file picked this session, so a relaunch comes back to Files instead.
      localStorage.setItem(KEY, tab === 'preview' ? 'files' : tab)
    } catch {
      /* not remembered */
    }
    set({ tab })
  },
  preview: (path) => set({ previewPath: path, tab: 'preview' }),
}))
