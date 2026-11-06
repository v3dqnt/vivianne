import type React from 'react'
import { usePanelStore } from '../../dock/store/panelStore'

/** A web link clicked in the chat: opened in Preview's browser, beside the
 * conversation. Ctrl/⌘-click (or a middle click) still goes to the system
 * browser, as a link normally would. */
export function onWebLinkClick(e: React.MouseEvent, url: string | undefined) {
  if (!url || !/^https?:\/\//i.test(url)) return
  if (e.ctrlKey || e.metaKey || e.shiftKey || e.button === 1) return
  e.preventDefault()
  usePanelStore.getState().openUrl(url)
}
