import { create } from 'zustand'
import { call } from '../../../ipc'

export interface ChangelogSection {
  title: string
  items: string[]
}

export interface ChangelogEntry {
  version: string
  date: string | null
  sections: ChangelogSection[]
}

export interface UpdateInfo {
  version: string
  currentVersion: string
  notes: string | null
  date: string | null
}

export interface Announcement {
  id: string
  title: string
  body: string
  level: string | null
  date: string | null
  url: string | null
  appVersion: string | null
}

export type CheckStatus = 'idle' | 'checking' | 'up-to-date' | 'available' | 'error'
export type InstallStatus = 'idle' | 'installing' | 'error'

interface UpdateStoreState {
  version: string
  changelog: ChangelogEntry[]
  announcements: Announcement[]
  update: UpdateInfo | null
  checkStatus: CheckStatus
  installStatus: InstallStatus
  error: string | null
  /** Bytes downloaded / total, while installing. `total` is null until the
   * server reports a content length. */
  progress: { downloaded: number; total: number | null } | null
  /** True when this build's notes haven't been shown yet — drives the
   * "what's new" dialog. */
  showWhatsNew: boolean

  init: () => Promise<void>
  check: (opts?: { silent?: boolean }) => Promise<void>
  install: () => Promise<void>
  dismissAnnouncement: (id: string) => Promise<void>
  dismissWhatsNew: () => Promise<void>
  setProgress: (p: { downloaded: number; total: number | null }) => void
}

export const useUpdateStore = create<UpdateStoreState>((set, get) => ({
  version: '',
  changelog: [],
  announcements: [],
  update: null,
  checkStatus: 'idle',
  installStatus: 'idle',
  error: null,
  progress: null,
  showWhatsNew: false,

  init: async () => {
    const [version, changelog, state] = await Promise.all([
      call<string>('cmd_app_version'),
      call<ChangelogEntry[]>('cmd_get_changelog'),
      call<{ lastSeenVersion: string | null }>('cmd_get_update_state'),
    ])
    // Fires once per upgrade, and never on a first-ever launch — someone
    // opening the app for the first time doesn't need a changelog for a
    // version they've never not been on.
    const showWhatsNew = !!version && !!state?.lastSeenVersion && state.lastSeenVersion !== version
    set({ version: version ?? '', changelog: changelog ?? [], showWhatsNew })

    // A first launch still records the version, so the NEXT upgrade is what
    // triggers the dialog rather than this one.
    if (version && !state?.lastSeenVersion) await call('cmd_mark_version_seen')

    // Announcements are fetched separately and failure-tolerantly: the
    // backend returns [] rather than erroring when the feed is unreachable,
    // so a missing network never blocks startup state.
    const announcements = await call<Announcement[]>('cmd_get_announcements')
    set({ announcements: announcements ?? [] })
  },

  check: async ({ silent = false } = {}) => {
    if (!silent) set({ checkStatus: 'checking', error: null })
    try {
      const update = await call<UpdateInfo | null>('cmd_check_for_update')
      set({ update: update ?? null, checkStatus: update ? 'available' : 'up-to-date' })
    } catch (e) {
      // A silent background check that fails stays silent — an unreachable
      // update endpoint is not worth a visible error on every launch.
      if (silent) set({ checkStatus: 'idle' })
      else set({ checkStatus: 'error', error: e instanceof Error ? e.message : String(e) })
    }
  },

  install: async () => {
    set({ installStatus: 'installing', error: null, progress: { downloaded: 0, total: null } })
    try {
      // Doesn't resolve on success: the backend restarts into the new build.
      await call('cmd_install_update')
    } catch (e) {
      set({ installStatus: 'error', progress: null, error: e instanceof Error ? e.message : String(e) })
    }
  },

  dismissAnnouncement: async (id) => {
    set({ announcements: get().announcements.filter((a) => a.id !== id) })
    await call('cmd_dismiss_announcement', { id })
  },

  dismissWhatsNew: async () => {
    set({ showWhatsNew: false })
    await call('cmd_mark_version_seen')
  },

  setProgress: (progress) => set({ progress }),
}))
