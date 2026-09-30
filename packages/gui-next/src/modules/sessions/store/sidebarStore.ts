import { create } from 'zustand'

const KEY = 'viv.sidebarCollapsed'

function load(): boolean {
  try {
    return localStorage.getItem(KEY) === '1'
  } catch {
    return false
  }
}

interface SidebarState {
  /** The project sidebar is folded away, leaving the rail and the workspace. */
  collapsed: boolean
  toggle: () => void
}

export const useSidebarStore = create<SidebarState>((set, get) => ({
  collapsed: load(),
  toggle: () => {
    const collapsed = !get().collapsed
    try {
      localStorage.setItem(KEY, collapsed ? '1' : '0')
    } catch {
      // Storage refused: the choice lasts for this session only.
    }
    set({ collapsed })
  },
}))
