import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'

/** Which panel the right-hand side is showing. `'live'` and `'browser'` were
 * merged into a single `'preview'` panel with an internal mode switch (dev
 * server vs. browser) — see `PreviewPanel.tsx`. */
export type RightPanelKind = 'review' | 'files' | 'preview' | 'tasks'

export const RIGHT_PANEL_LABELS: Record<RightPanelKind, string> = {
  review: 'Review',
  files: 'All files',
  preview: 'Preview',
  tasks: 'Background tasks',
}

/** Which of the two modes the merged Preview panel is showing. */
export type PreviewMode = 'dev' | 'browser'

interface PanelStoreState {
  rightOpen: boolean
  rightKind: RightPanelKind
  rightWidth: number
  bottomOpen: boolean
  bottomHeight: number
  previewMode: PreviewMode
  /** Open that panel, or close it if it's already the open one. */
  toggleRight: (kind: RightPanelKind) => void
  /** Always opens (never closes) — for callers that want a deterministic
   * "make sure this is showing" rather than toggleRight's flip-if-already-open
   * behavior. Used by the agent's set_panel tool. */
  openRight: (kind: RightPanelKind) => void
  closeRight: () => void
  setRightWidth: (width: number) => void
  toggleBottom: () => void
  /** Always opens (never closes) — same rationale as openRight. */
  openBottom: () => void
  closeBottom: () => void
  setBottomHeight: (height: number) => void
  setPreviewMode: (mode: PreviewMode) => void
  /** True while either `ResizeHandle` drag (right panel or bottom terminal)
   * is active. Transient — never persisted. Lets a mounted `TerminalPanel`
   * skip its ResizeObserver-driven xterm `fit()` + resize IPC while the
   * panel's actual DOM size is changing every frame of the drag, then run
   * one `fit()` when the drag ends. */
  panelResizing: boolean
  setPanelResizing: (resizing: boolean) => void
}

const STORAGE_KEY = 'viv.panels'
type Persisted = Pick<PanelStoreState, 'rightOpen' | 'rightKind' | 'rightWidth' | 'bottomOpen' | 'bottomHeight' | 'previewMode'>

/** A pre-merge `rightKind` of `'live'`/`'browser'` (from localStorage written
 * before Live Preview and Browser became one `'preview'` panel) maps onto the
 * merged panel, in the matching mode. Anything else passes through as-is. */
function migrateRightKind(kind: string): { rightKind: RightPanelKind; previewMode?: PreviewMode } {
  if (kind === 'live') return { rightKind: 'preview', previewMode: 'dev' }
  if (kind === 'browser') return { rightKind: 'preview', previewMode: 'browser' }
  return { rightKind: kind as RightPanelKind }
}

/** Right-side panel + bottom terminal layout. Replaces the old always-on
 * vertical icon rail: the toggles now live in the title bar, so the panels
 * are pure content with nothing permanently occupying screen edge. */
export const usePanelStore = create<PanelStoreState>((set, get) => {
  const persist = () => {
    const { rightOpen, rightKind, rightWidth, bottomOpen, bottomHeight, previewMode } = get()
    savePersisted(STORAGE_KEY, { rightOpen, rightKind, rightWidth, bottomOpen, bottomHeight, previewMode })
  }

  const loaded = loadPersisted<Omit<Persisted, 'previewMode'> & { previewMode?: PreviewMode }>(STORAGE_KEY, {
    rightOpen: false,
    rightKind: 'review',
    rightWidth: 420,
    bottomOpen: false,
    bottomHeight: 260,
    previewMode: 'dev',
  })
  const migrated = migrateRightKind(loaded.rightKind)

  return {
    ...loaded,
    rightKind: migrated.rightKind,
    previewMode: migrated.previewMode ?? loaded.previewMode ?? 'dev',
    panelResizing: false,

    toggleRight: (kind) => {
      const { rightOpen, rightKind } = get()
      if (rightOpen && rightKind === kind) set({ rightOpen: false })
      else set({ rightOpen: true, rightKind: kind })
      persist()
    },
    openRight: (kind) => {
      set({ rightOpen: true, rightKind: kind })
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
    openBottom: () => {
      set({ bottomOpen: true })
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
    setPreviewMode: (previewMode) => {
      set({ previewMode })
      persist()
    },
    setPanelResizing: (panelResizing) => {
      set({ panelResizing })
    },
  }
})
