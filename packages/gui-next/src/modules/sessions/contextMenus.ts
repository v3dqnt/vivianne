import { call } from '../../ipc'
import { registerMenu } from '../../lib/contextMenu'
import { useArchivedThreads } from '../../lib/archivedThreads'
import { useDaysStore } from './store/daysStore'

/** A thread row, as the menu sees it. */
export interface ThreadTarget {
  id: string
  title: string
}

/** A file or folder, absolute, optionally at a line. */
export interface FileTarget {
  path: string
  line?: number
}

interface OpenApp {
  id: string
  label: string
}

// The installed editors don't change while the app runs, so ask once.
let apps: Promise<OpenApp[]> | null = null
const installedApps = () => (apps ??= call<OpenApp[]>('cmd_list_open_apps').then((a) => a ?? []).catch(() => []))

/** What can be done with a thread, and with a file: registered once at startup. */
export function registerContextMenus(): void {
  registerMenu<ThreadTarget>('thread', (t) => {
    const archived = useArchivedThreads.getState().ids.includes(t.id)
    return [
      { id: 'fork', label: 'Fork into a new thread', group: 'thread', run: () => useDaysStore.getState().forkThread(t.id) },
      { id: 'archive', label: archived ? 'Restore from archive' : 'Archive', group: 'thread', run: () => useArchivedThreads.getState().set(t.id, !archived) },
      { id: 'delete', label: 'Delete…', group: 'danger', danger: true, run: () => useDaysStore.getState().deleteThread(t.id) },
    ]
  })

  registerMenu<FileTarget>('file', async (f) => {
    const found = await installedApps()
    const open = found.filter((a) => a.id !== 'default' && a.id !== 'reveal').map((a) => ({ id: `open-${a.id}`, label: `Open in ${a.label}`, group: 'open', run: async () => void (await call('cmd_open_in_app', { path: f.path, app: a.id, line: f.line ?? null })) }))
    return [
      ...open,
      { id: 'open-default', label: 'Open with the default app', group: 'open', run: async () => void (await call('cmd_open_in_app', { path: f.path, app: 'default', line: null })) },
      { id: 'reveal', label: 'Show in folder', group: 'open', run: async () => void (await call('cmd_open_in_app', { path: f.path, app: 'reveal', line: null })) },
      { id: 'copy-path', label: 'Copy path', group: 'copy', run: () => navigator.clipboard.writeText(f.path) },
    ]
  })
}
