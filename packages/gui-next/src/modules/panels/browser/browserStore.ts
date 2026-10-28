import { create } from 'zustand'
import { loadPersisted, savePersisted } from '../../../lib/persist'
import { closeTab, fileTab, HOME_URL, webTab, type BrowserTab } from './tabs'

const KEY = 'viv.browser.v1'

interface Persisted {
  tabs: BrowserTab[]
  activeId: string | null
}

interface BrowserState {
  tabs: BrowserTab[]
  activeId: string | null
  /** Open a web tab, or focus the one already showing that address. */
  openUrl: (url: string) => void
  /** Open a local file as a tab, or focus it if it is already open. */
  openFile: (path: string, title: string) => void
  /** A new tab at the home page. */
  newTab: () => void
  close: (id: string) => void
  activate: (id: string) => void
  setUrl: (id: string, url: string) => void
  setTitle: (id: string, title: string) => void
}

const saved = loadPersisted<Persisted>(KEY, { tabs: [], activeId: null })

function persist(tabs: BrowserTab[], activeId: string | null) {
  savePersisted(KEY, { tabs, activeId })
}

export const useBrowserStore = create<BrowserState>((set, get) => {
  const commit = (tabs: BrowserTab[], activeId: string | null) => {
    persist(tabs, activeId)
    set({ tabs, activeId })
  }
  return {
    tabs: saved.tabs ?? [],
    activeId: saved.activeId ?? null,

    openUrl: (url) => {
      const { tabs } = get()
      const existing = tabs.find((t) => t.kind === 'web' && t.url === url)
      if (existing) return get().activate(existing.id)
      const tab = webTab(url)
      commit([...tabs, tab], tab.id)
    },
    openFile: (path, title) => {
      const { tabs } = get()
      const existing = tabs.find((t) => t.kind === 'file' && t.path === path)
      if (existing) return get().activate(existing.id)
      const tab = fileTab(path, title)
      commit([...tabs, tab], tab.id)
    },
    newTab: () => {
      const tab = webTab(HOME_URL)
      commit([...get().tabs, tab], tab.id)
    },
    close: (id) => {
      const { tabs, activeId } = get()
      const next = closeTab(tabs, activeId, id)
      commit(next.tabs, next.activeId)
    },
    activate: (id) => {
      const { tabs } = get()
      commit(tabs, id)
    },
    // Both are polled every second, so an unchanged value writes nothing.
    setUrl: (id, url) => {
      const { tabs, activeId } = get()
      const current = tabs.find((t) => t.id === id)
      if (current?.kind !== 'web' || current.url === url) return
      commit(tabs.map((t) => (t.id === id && t.kind === 'web' ? { ...t, url } : t)), activeId)
    },
    setTitle: (id, title) => {
      const { tabs } = get()
      const current = tabs.find((t) => t.id === id)
      if (!current || current.title === title) return
      // Titles change on every navigation; keep the state update cheap and
      // skip the write, since the title is re-read from the page on launch.
      set({ tabs: tabs.map((t) => (t.id === id ? { ...t, title } : t)) })
    },
  }
})
