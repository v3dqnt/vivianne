import { create } from 'zustand'
import { call } from '../../../ipc'

/**
 * What used to open a side panel. There are no side panels any more, so a web
 * address or a local file opens in the system's own app instead. The method
 * names stay because the chat, the agent's events and a few components call
 * them.
 */
function openExternally(target: string) {
  void call('cmd_open_path', { path: target }).catch(() => {
    if (/^https?:\/\//i.test(target)) window.open(target, '_blank', 'noopener')
  })
}

interface PanelStoreState {
  /** Open a web address in the system browser. */
  openUrl: (url: string) => void
  /** Open a local file in its default app. */
  openDocument: (path: string) => void
  /** True while a resize drag is in flight (kept for the terminal code). */
  panelResizing: boolean
  setPanelResizing: (resizing: boolean) => void
}

export const usePanelStore = create<PanelStoreState>((set) => ({
  openUrl: (url) => openExternally(url),
  openDocument: (path) => openExternally(path),
  panelResizing: false,
  setPanelResizing: (panelResizing) => set({ panelResizing }),
}))
