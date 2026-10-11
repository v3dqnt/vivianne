import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'

/** Where the fixed chrome sits (Settings → Layout & shortcuts). */
interface LayoutPrefs {
  sidebarSide: 'left' | 'right'
  showRail: boolean
  update: (patch: Partial<Pick<LayoutPrefs, 'sidebarSide' | 'showRail'>>) => void
}

const KEY = 'viv.layoutPrefs'
const saved = loadPersisted<Partial<LayoutPrefs>>(KEY, {})

export const useLayoutPrefs = create<LayoutPrefs>((set, get) => ({
  sidebarSide: saved.sidebarSide ?? 'left',
  showRail: saved.showRail ?? true,
  update: (patch) => {
    set(patch)
    const { sidebarSide, showRail } = { ...get(), ...patch }
    savePersisted(KEY, { sidebarSide, showRail })
  },
}))
