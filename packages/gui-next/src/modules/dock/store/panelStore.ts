import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'
import { closePanel, openPanel } from '../../workspace/layout'

/**
 * Panel commands for the rest of the app — now a thin facade over the
 * docking workspace (`modules/workspace/layout.ts`).
 *
 * This used to own two fixed docks, right and bottom, each a strip of tabs.
 * Panels now live wherever the user docks them, so "open the right dock" has
 * no meaning; "open the preview" does. The old method names stay because the
 * agent's `set_panel` tool and several components call them, and each maps to
 * the panel-level operation it always actually meant.
 */

/** One kind of view a panel can host. `'custom'` renders a user-defined panel
 * playbook named by `panelName`. */
export type DockViewKind = 'review' | 'files' | 'ide' | 'preview' | 'documents' | 'tasks' | 'multiplayer' | 'terminal' | 'agents' | 'plan' | 'document' | 'custom'

/** Kept as the old name so `set_panel`'s values keep type-checking. */
export type RightPanelKind = DockViewKind

/** Which mode the merged Preview panel is showing. 'live' is a real
 * embedded webview; 'browser' is the agent's camofox screenshot feed;
 * 'documents' is the document viewer (PDF, Word, Excel, PowerPoint). */
export type PreviewMode = 'dev' | 'live' | 'browser' | 'documents'

interface PanelStoreState {
  previewMode: PreviewMode
  /** Documents open in Preview's Documents mode, as tabs. */
  openDocs: string[]
  activeDoc: string | null
  /** Show a document in Preview (opening the panel and its Documents mode). */
  openDocument: (path: string) => void
  closeDocument: (path: string) => void
  /** True while a panel resize drag is in flight. Transient. Lets a terminal
   * skip its per-frame `fit()` + resize IPC and fit once at the end. */
  panelResizing: boolean

  /** Open (or focus) a panel. Formerly "open it in the right dock". */
  openRight: (kind: DockViewKind) => void
  /** Close the side panels the agent opens. Formerly "collapse the right dock". */
  closeRight: (kind?: DockViewKind) => void
  openCustomPanel: (name: string) => void
  /** Open a terminal. */
  openBottom: () => void
  /** Deliberately does nothing — see the note on the implementation. */
  closeBottom: () => void

  setPreviewMode: (mode: PreviewMode) => void
  setPanelResizing: (resizing: boolean) => void
}

const STORAGE_KEY = 'viv.previewMode'
const DOCS_KEY = 'viv.previewDocs'
const MAX_OPEN_DOCS = 12

function saveDocs(openDocs: string[], activeDoc: string | null) {
  savePersisted(DOCS_KEY, { openDocs, activeDoc })
}

/** The side panels `set_panel` can open — what "close the right panel" closes
 * when the agent doesn't name one. */
const AGENT_SIDE_PANELS: DockViewKind[] = ['review', 'files', 'preview', 'tasks']

export const usePanelStore = create<PanelStoreState>((set, get) => ({
  ...loadPersisted<{ openDocs: string[]; activeDoc: string | null }>(DOCS_KEY, { openDocs: [], activeDoc: null }),
  openDocument: (path) => {
    const docs = get().openDocs.filter((d) => d !== path)
    const openDocs = [...docs, path].slice(-MAX_OPEN_DOCS)
    saveDocs(openDocs, path)
    savePersisted(STORAGE_KEY, 'documents')
    set({ openDocs, activeDoc: path, previewMode: 'documents' })
    openPanel('preview')
  },
  closeDocument: (path) => {
    const { openDocs, activeDoc } = get()
    const i = openDocs.indexOf(path)
    const next = openDocs.filter((d) => d !== path)
    const active = activeDoc === path ? (next[Math.min(i, next.length - 1)] ?? null) : activeDoc
    saveDocs(next, active)
    set({ openDocs: next, activeDoc: active })
  },
  // Read from the old docks key too, so the preview keeps the mode it was in.
  previewMode: loadPersisted<PreviewMode | null>(STORAGE_KEY, null) ?? loadPersisted<{ previewMode?: PreviewMode }>('viv.docks', {}).previewMode ?? 'dev',
  panelResizing: false,

  openRight: (kind) => openPanel(kind),
  closeRight: (kind) => {
    for (const k of kind ? [kind] : AGENT_SIDE_PANELS) closePanel(k)
  },
  openCustomPanel: (name) => openPanel('custom', { panelName: name }),
  openBottom: () => openPanel('terminal'),
  // The old bottom dock only ever *hid* its terminals; their shells kept
  // running. Closing a terminal panel now kills its shell, so honouring
  // "close" here would let the agent end a session the user may be typing in.
  // The panel stays; the user closes it if they want to.
  closeBottom: () => {},

  setPreviewMode: (previewMode) => {
    savePersisted(STORAGE_KEY, previewMode)
    set({ previewMode })
  },
  setPanelResizing: (panelResizing) => set({ panelResizing }),
}))
