import { create } from 'zustand'
import { call } from '../../../ipc'
import { useMcpStore } from './mcpStore'

export interface ConnectionField {
  key: string
  label: string
  secret: boolean
  placeholder: string
}

/** Mirrors `connections::AppView`. */
export interface ConnectionApp {
  id: string
  name: string
  category: string
  description: string
  domain: string
  auth: 'oauth' | 'open' | 'token' | 'native'
  fields: ConnectionField[]
  help: string
  helpUrl: string
  connected: boolean
  account: string | null
}

interface ConnectionsState {
  apps: ConnectionApp[]
  loaded: boolean
  /** The app whose sign-in is in progress, if any. */
  pending: string | null
  errors: Record<string, string>
  refresh: () => Promise<void>
  connect: (id: string, fields?: Record<string, string>) => Promise<boolean>
  cancel: () => Promise<void>
  disconnect: (id: string) => Promise<void>
}

const message = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e))

export const useConnections = create<ConnectionsState>((set, get) => ({
  apps: [],
  loaded: false,
  pending: null,
  errors: {},
  refresh: async () => {
    const apps = await call<ConnectionApp[]>('cmd_connections_list')
    set({ apps: apps ?? [], loaded: true })
  },
  connect: async (id, fields) => {
    const { [id]: _, ...errors } = get().errors
    set({ pending: id, errors })
    try {
      await call('cmd_connections_connect', { id, fields: fields ?? null })
      await Promise.all([get().refresh(), useMcpStore.getState().refresh()])
      return true
    } catch (e) {
      const text = message(e)
      // Cancelling isn't a failure worth showing.
      if (!/cancel/i.test(text)) set((s) => ({ errors: { ...s.errors, [id]: text } }))
      return false
    } finally {
      set({ pending: null })
    }
  },
  cancel: async () => {
    await call('cmd_connections_cancel')
  },
  disconnect: async (id) => {
    await call('cmd_connections_disconnect', { id })
    await Promise.all([get().refresh(), useMcpStore.getState().refresh()])
  },
}))

const icons = new Map<string, Promise<string | null>>()

/** An app's icon, fetched once per session (the backend keeps it on disk). */
export function appIcon(domain: string): Promise<string | null> {
  let p = icons.get(domain)
  if (!p) {
    p = call<string>('cmd_connections_icon', { domain }).catch(() => null)
    icons.set(domain, p)
  }
  return p
}
