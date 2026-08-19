import { create } from 'zustand'
import { listen } from '@tauri-apps/api/event'
import { call, hasTauri } from '../../../ipc'
import type { DevServerConfig, DevServerStatus } from '../lib/devServer'

interface DevServerStoreState {
  servers: [DevServerConfig, DevServerStatus][]
  refresh: () => Promise<void>
  register: (config: DevServerConfig) => Promise<void>
  start: (id: string) => Promise<void>
  stop: (id: string) => Promise<void>
}

let listening = false

export const useDevServerStore = create<DevServerStoreState>((set, get) => {
  // The backend only emits `dev_server_url_detected` once, the moment it
  // finds the URL in the dev tool's own output — refresh on that event so
  // "Starting…" flips to the live iframe without the user having to poll
  // manually. Registered once at module scope (not per-`start()` call, and
  // not torn down) since this store — like every other Zustand store in the
  // app — lives for the whole session.
  // `hasTauri()` guard: `listen` reaches into the Tauri global, which throws
  // when the UI is opened in a plain browser (the dev-server preview).
  if (!listening && hasTauri()) {
    listening = true
    void listen('dev_server_url_detected', () => {
      void get().refresh()
    })
  }

  return {
    servers: [],

    refresh: async () => {
      const list = await call<[DevServerConfig, DevServerStatus][]>('cmd_dev_server_list')
      set({ servers: list ?? [] })
    },

    register: async (config) => {
      await call('cmd_dev_server_register', { ...config })
      await get().refresh()
    },

    start: async (id) => {
      await call('cmd_dev_server_start', { id })
      await get().refresh()
    },

    stop: async (id) => {
      await call('cmd_dev_server_stop', { id })
      await get().refresh()
    },
  }
})
