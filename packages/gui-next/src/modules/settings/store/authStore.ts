import { create } from 'zustand'
import { call } from '../../../ipc'
import { CLOUD_PROVIDERS } from '../lib/providers'

interface AuthStoreState {
  status: Record<string, boolean>
  loading: boolean
  refresh: () => Promise<void>
  saveKey: (providerId: string, apiKey: string) => Promise<void>
  deleteKey: (providerId: string) => Promise<void>
}

export const useAuthStore = create<AuthStoreState>((set, get) => ({
  status: {},
  loading: false,

  refresh: async () => {
    set({ loading: true })
    try {
      const entries = await Promise.all(
        CLOUD_PROVIDERS.map(async (p) => [p.id, (await call<boolean>('auth_is_authenticated', { providerId: p.id })) ?? false] as const),
      )
      set({ status: Object.fromEntries(entries) })
    } finally {
      set({ loading: false })
    }
  },

  saveKey: async (providerId, apiKey) => {
    await call('auth_store_api_key', { providerId, apiKey })
    set({ status: { ...get().status, [providerId]: true } })
  },

  deleteKey: async (providerId) => {
    await call('auth_delete_credential', { providerId })
    set({ status: { ...get().status, [providerId]: false } })
  },
}))
