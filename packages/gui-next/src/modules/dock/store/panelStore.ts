import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'

/** One kind of view a dock tab can host. `'preview'` merges what used to be
 * separate `'live'` and `'browser'` panels into one with an internal mode
 * switch (see `PreviewPanel.tsx`). `'custom'` renders whichever user-defined
 * panel the tab's `panelName` names (see `CustomPanel.tsx`) — unlike the
 * others it isn't a fixed singular view, so it's deliberately left out of
 * `VIEW_LABELS`; a custom tab labels itself with its panel's name. */
export type DockViewKind = 'review' | 'files' | 'preview' | 'tasks' | 'multiplayer' | 'terminal' | 'custom'

/** Kept as the old name/shape so `set_panel`'s existing values keep working. */
export type RightPanelKind = DockViewKind

export const VIEW_LABELS: Record<Exclude<DockViewKind, 'custom'>, string> = {
  review: 'Review',
  files: 'All files',
  preview: 'Preview',
  tasks: 'Background tasks',
  multiplayer: 'Multiplayer',
  terminal: 'Terminal',
}

/** Back-compat alias — `RIGHT_PANEL_LABELS` predates views being hostable in
 * either dock. */
export const RIGHT_PANEL_LABELS = VIEW_LABELS

export type DockId = 'right' | 'bottom'

export interface DockTab {
  /** Stable across renders and unique across both docks — a terminal tab
   * uses it as its PTY id, so reusing one would attach two views to the
   * same shell. */
  id: string
  kind: DockViewKind
  /** Only for `kind === 'custom'`: which installed panel playbook to render. */
  panelName?: string
}

interface Dock {
  open: boolean
  tabs: DockTab[]
  /** `null` only when `tabs` is empty. */
  activeId: string | null
  /** Width for the right dock, height for the bottom one. */
  size: number
}

/** Which of the two modes the merged Preview panel is showing. */
export type PreviewMode = 'dev' | 'browser'

interface PanelStoreState {
  right: Dock
  bottom: Dock
  previewMode: PreviewMode
  /** True while either dock's `ResizeHandle` drag is active. Transient —
   * never persisted. Lets a mounted `TerminalPanel` skip its
   * ResizeObserver-driven xterm `fit()` + resize IPC while the panel's actual
   * DOM size is changing every frame of the drag, then run one `fit()` when
   * the drag ends. */
  panelResizing: boolean

  toggleDock: (dock: DockId) => void
  openDock: (dock: DockId) => void
  closeDock: (dock: DockId) => void
  setDockSize: (dock: DockId, size: number) => void
  /** Adds a tab and focuses it. A non-terminal view is single-instance per
   * dock — asking for one that's already there just focuses it, since two
   * identical file trees side by side is never what was meant. Terminals are
   * the exception: each is its own shell, so a second one is a real request. */
  addTab: (dock: DockId, kind: DockViewKind, panelName?: string) => void
  closeTab: (dock: DockId, tabId: string) => void
  setActiveTab: (dock: DockId, tabId: string) => void

  /** Focus (creating if needed) a view in the right dock and open it. Used by
   * the agent's `set_panel` tool, which has no concept of tabs. */
  openRight: (kind: DockViewKind) => void
  closeRight: () => void
  /** Focus (creating if needed) a tab for the named custom panel playbook. */
  openCustomPanel: (name: string) => void
  openBottom: () => void
  closeBottom: () => void

  setPreviewMode: (mode: PreviewMode) => void
  setPanelResizing: (resizing: boolean) => void
}

const STORAGE_KEY = 'viv.docks'
/** The pre-tabs key. Read once to carry a previous layout over rather than
 * dumping everyone back to defaults on upgrade. */
const LEGACY_KEY = 'viv.panels'

let tabSeq = 0
function newTabId(kind: DockViewKind): string {
  tabSeq += 1
  return `${kind}-${Date.now().toString(36)}-${tabSeq}`
}

interface PersistedDock {
  open: boolean
  tabs: DockTab[]
  activeId: string | null
  size: number
}
interface Persisted {
  right: PersistedDock
  bottom: PersistedDock
  previewMode: PreviewMode
}

/** The old single-view-per-dock shape, for one-time migration. */
interface LegacyPersisted {
  rightOpen?: boolean
  rightKind?: string
  customPanelId?: string | null
  rightWidth?: number
  bottomOpen?: boolean
  bottomHeight?: number
  previewMode?: PreviewMode
}

function emptyDock(size: number): PersistedDock {
  return { open: false, tabs: [], activeId: null, size }
}

/** Builds the initial two docks: the new persisted shape if present,
 * otherwise the pre-tabs layout converted into a single tab per dock,
 * otherwise defaults. */
function initialState(): Persisted {
  const stored = loadPersisted<Persisted | null>(STORAGE_KEY, null as unknown as Persisted)
  if (stored && stored.right && stored.bottom) return stored

  const legacy = loadPersisted<LegacyPersisted>(LEGACY_KEY, {})
  const right = emptyDock(legacy.rightWidth ?? 420)
  const bottom = emptyDock(legacy.bottomHeight ?? 260)
  let previewMode: PreviewMode = legacy.previewMode ?? 'dev'

  // `'live'`/`'browser'` predate Live Preview and Browser becoming one
  // `'preview'` panel with a mode switch.
  let kind = legacy.rightKind ?? 'review'
  if (kind === 'live') {
    kind = 'preview'
    previewMode = 'dev'
  } else if (kind === 'browser') {
    kind = 'preview'
    previewMode = 'browser'
  }
  const rightTab: DockTab = {
    id: newTabId(kind as DockViewKind),
    kind: kind as DockViewKind,
    panelName: kind === 'custom' ? (legacy.customPanelId ?? undefined) : undefined,
  }
  right.tabs = [rightTab]
  right.activeId = rightTab.id
  right.open = legacy.rightOpen ?? false

  const bottomTab: DockTab = { id: newTabId('terminal'), kind: 'terminal' }
  bottom.tabs = [bottomTab]
  bottom.activeId = bottomTab.id
  bottom.open = legacy.bottomOpen ?? false

  return { right, bottom, previewMode }
}

/** Right and bottom docks: each holds any number of tabbed views, and every
 * view kind can live in either one. Replaces the old one-view-per-dock model
 * (a single `rightKind` plus a hardcoded bottom terminal), which meant
 * switching to the file tree threw away whatever you were looking at. */
export const usePanelStore = create<PanelStoreState>((set, get) => {
  const persist = () => {
    const { right, bottom, previewMode } = get()
    savePersisted(STORAGE_KEY, { right, bottom, previewMode })
  }

  /** Every mutation goes through this so persistence can't be forgotten on a
   * new action. */
  const update = (dock: DockId, fn: (d: Dock) => Dock) => {
    set((s) => ({ [dock]: fn(s[dock]) }) as unknown as Partial<PanelStoreState>)
    persist()
  }

  /** Focus an existing tab matching `kind` (and `panelName`, for custom
   * panels), or create one. Shared by the tab UI and the agent's `set_panel`. */
  const focusOrCreate = (dock: DockId, kind: DockViewKind, panelName?: string, forceNew = false) => {
    update(dock, (d) => {
      const existing = forceNew
        ? undefined
        : d.tabs.find((t) => t.kind === kind && (kind !== 'custom' || t.panelName === panelName))
      if (existing) return { ...d, open: true, activeId: existing.id }
      const tab: DockTab = { id: newTabId(kind), kind, panelName }
      return { ...d, open: true, tabs: [...d.tabs, tab], activeId: tab.id }
    })
  }

  return {
    ...initialState(),
    panelResizing: false,

    toggleDock: (dock) =>
      update(dock, (d) => {
        // Reopening an empty dock would show a blank panel with no way to
        // add anything, so seed it with a sensible default view.
        if (!d.open && d.tabs.length === 0) {
          const tab: DockTab = {
            id: newTabId(dock === 'bottom' ? 'terminal' : 'files'),
            kind: dock === 'bottom' ? 'terminal' : 'files',
          }
          return { ...d, open: true, tabs: [tab], activeId: tab.id }
        }
        return { ...d, open: !d.open }
      }),

    openDock: (dock) => update(dock, (d) => ({ ...d, open: true })),
    closeDock: (dock) => update(dock, (d) => ({ ...d, open: false })),
    setDockSize: (dock, size) => update(dock, (d) => ({ ...d, size })),

    addTab: (dock, kind, panelName) => focusOrCreate(dock, kind, panelName, kind === 'terminal'),

    closeTab: (dock, tabId) =>
      update(dock, (d) => {
        const idx = d.tabs.findIndex((t) => t.id === tabId)
        if (idx === -1) return d
        const tabs = d.tabs.filter((t) => t.id !== tabId)
        // Closing the active tab focuses its neighbour rather than jumping to
        // the first one — the tab beside it is where attention already is.
        const activeId =
          d.activeId === tabId ? (tabs[Math.min(idx, tabs.length - 1)]?.id ?? null) : d.activeId
        return { ...d, tabs, activeId, open: tabs.length > 0 && d.open }
      }),

    setActiveTab: (dock, tabId) => update(dock, (d) => ({ ...d, activeId: tabId, open: true })),

    openRight: (kind) => focusOrCreate('right', kind),
    closeRight: () => update('right', (d) => ({ ...d, open: false })),
    openCustomPanel: (name) => focusOrCreate('right', 'custom', name),
    openBottom: () => focusOrCreate('bottom', 'terminal'),
    closeBottom: () => update('bottom', (d) => ({ ...d, open: false })),

    setPreviewMode: (previewMode) => {
      set({ previewMode })
      persist()
    },
    setPanelResizing: (panelResizing) => set({ panelResizing }),
  }
})
