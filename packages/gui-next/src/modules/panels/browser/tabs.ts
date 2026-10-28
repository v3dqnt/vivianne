export const HOME_URL = 'https://duckduckgo.com'

export type BrowserTab =
  | { id: string; kind: 'web'; url: string; title: string; webviewLabel: string }
  | { id: string; kind: 'file'; path: string; title: string }

export function newId(): string {
  return crypto.randomUUID().slice(0, 8)
}

export function webTab(url: string, id = newId()): BrowserTab {
  return { id, kind: 'web', url, title: hostOf(url), webviewLabel: `live_browser_${id}` }
}

export function fileTab(path: string, title: string, id = newId()): BrowserTab {
  return { id, kind: 'file', path, title }
}

/** Close a tab: the neighbour on the right takes over, else the one on the
 * left, else nothing. */
export function closeTab(tabs: BrowserTab[], activeId: string | null, id: string): { tabs: BrowserTab[]; activeId: string | null } {
  const i = tabs.findIndex((t) => t.id === id)
  const next = tabs.filter((t) => t.id !== id)
  if (activeId !== id) return { tabs: next, activeId }
  return { tabs: next, activeId: next[Math.min(i, next.length - 1)]?.id ?? null }
}

export function hostOf(url: string): string {
  try {
    return new URL(url).host || url
  } catch {
    return url
  }
}

/** What a tab's label reads. A web tab follows the page: its `document.title`
 * when the page sets one, the host otherwise. */
export function titleOf(tab: BrowserTab, url: string | null, pageTitle: string | null): string {
  if (tab.kind === 'file') return tab.title
  if (pageTitle && pageTitle.trim()) return pageTitle.trim()
  return hostOf(url ?? tab.url) || tab.title
}
