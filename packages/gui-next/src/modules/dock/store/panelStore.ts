import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'

/** Which panel the right-hand side is showing. */
export type RightPanelKind = 'review' | 'files' | 'live' | 'browser' | 'tasks'

export const RIGHT_PANEL_LABELS: Record<RightPanelKind, string> = {
  review: 'Review',
  files: 'All files',
  live: 'Live preview',
  browser: 'Browser',
  tasks: 'Background tasks',
}

interface PanelStoreState {
  rightOpen: boolean
  rightKind: RightPanelKind
  rightWidth: number
  bottomOpen: boolean
  bottomHeight: number
  /** Open that panel, or close it if it's already the open one. */
  toggleRight: (kind: RightPanelKind) => void
  closeRight: () => void
  setRightWidth: (width: number) => void
  toggleBottom: () => void
  closeBottom: () => void
  setBottomHeight: (height: number) => void
}

const STORAGE_KEY = 'viv.panels'
type Persisted = Pick<PanelStoreState, 'rightOpen' | 'rightKind' | 'rightWidth' | 'bottomOpen' | 'bottomHeight'>

/** Right-side panel + bottom terminal layout. Replaces the old always-on
 * vertical icon rail: the toggles now live in the title bar, so the panels
 * are pure content with nothing permanently occupying screen edge. */
export const usePanelStore = create<PanelStoreState>((set, get) => {
  const persist = () => {
    const { rightOpen, rightKind, rightWidth, bottomOpen, bottomHeight } = get()
    savePersisted(STORAGE_KEY, { rightOpen, rightKind, rightWidth, bottomOpen, bottomHeight })
  }

  return {
    ...loadPersisted<Persisted>(STORAGE_KEY, {
      rightOpen: false,
      rightKind: 'review',
      rightWidth: 420,
      bottomOpen: false,
      bottomHeight: 260,
    }),

    toggleRight: (kind) => {
      const { rightOpen, rightKind } = get()
      if (rightOpen && rightKind === kind) set({ rightOpen: false })
      else set({ rightOpen: true, rightKind: kind })
      persist()
    },
    closeRight: () => {
      set({ rightOpen: false })
      persist()
    },
    setRightWidth: (rightWidth) => {
      set({ rightWidth })
      persist()
    },
    toggleBottom: () => {
      set({ bottomOpen: !get().bottomOpen })
      persist()
    },
    closeBottom: () => {
      set({ bottomOpen: false })
      persist()
    },
    setBottomHeight: (bottomHeight) => {
      set({ bottomHeight })
      persist()
    },
  }
})
