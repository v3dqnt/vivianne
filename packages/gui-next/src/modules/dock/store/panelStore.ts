import { create } from 'zustand'
import { openPanel } from '../../workspace/layout'
import type { PanelKind } from '../../workspace/registry'

/**
 * Panel commands for the rest of the app — a thin facade over the docking
 * workspace (`modules/workspace/layout.ts`).
 *
 * The old method names stay because the agent's `set_panel` tool and several
 * components call them, and each maps to the panel-level operation it always
 * actually meant.
 */

/** A tab the Browser panel should open the next time it renders. The panel
 * takes it and clears it, so one request never opens twice. */
export type PendingOpen = { kind: 'url'; url: string } | { kind: 'file'; path: string }

interface PanelStoreState {
  pending: PendingOpen | null
  /** Open a web address in the Browser panel. */
  openUrl: (url: string) => void
  /** Open a local file in the Browser panel. */
  openDocument: (path: string) => void
  /** Take the pending request, if any. */
  takePending: () => PendingOpen | null
  /** True while a panel resize drag is in flight. Transient. Lets a terminal
   * skip its per-frame `fit()` + resize IPC and fit once at the end. */
  panelResizing: boolean
  /** Open (or focus) a panel. */
  openRight: (kind: PanelKind) => void
  /** Open the terminal. */
  openBottom: () => void
  /** Deliberately does nothing — see the note on the implementation. */
  closeBottom: () => void
  setPanelResizing: (resizing: boolean) => void
}

export const usePanelStore = create<PanelStoreState>((set, get) => ({
  pending: null,
  openUrl: (url) => {
    set({ pending: { kind: 'url', url } })
    openPanel('browser')
  },
  openDocument: (path) => {
    set({ pending: { kind: 'file', path } })
    openPanel('browser')
  },
  takePending: () => {
    const p = get().pending
    if (p) set({ pending: null })
    return p
  },
  panelResizing: false,
  openRight: (kind) => openPanel(kind),
  openBottom: () => openPanel('terminal'),
  // Closing a terminal panel kills its shell, so honouring "close" here would
  // let the agent end a session the user may be typing in. The panel stays;
  // the user closes it if they want to.
  closeBottom: () => {},
  setPanelResizing: (panelResizing) => set({ panelResizing }),
}))
