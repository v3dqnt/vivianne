import { create } from 'zustand'
import { call } from '../../../ipc'
import { CLOUD_PROVIDERS } from '../lib/providers'

export type RotationStrategy = 'fill_first' | 'round_robin' | 'least_used' | 'random'
export type CredentialStatus = 'ok' | 'retryable' | 'quota_or_billing' | 'other'

export interface PoolEntryInfo {
  id: string
  label: string
  source: 'primary' | 'pool'
  status: CredentialStatus
  requestCount: number
  cooldownUntil: number | null
  lastUsedAt: number | null
  inCooldown: boolean
}

/**
 * Single home for every provider's API-key state: the plain "connected?"
 * status the provider list shows, and — once a provider's key-pool popover
 * is opened — the full pool of primary + extra keys and its rotation
 * strategy. Previously split across this file (single key) and a separate
 * credentialPoolStore.ts (extra keys), mirroring the same artificial split
 * the Rust side had between auth.rs and credential_pool.rs before those
 * were merged too — one provider, one list of usable keys, one place to
 * manage them.
 */
interface AuthStoreState {
  /** Whether each provider has at least one usable key — drives the
   * "Connected"/"Not connected" label in the provider list. */
  status: Record<string, boolean>
  loading: boolean
  /** Full pool listing per provider (primary + extra keys), populated
   * on-demand when a provider's key-pool popover opens. */
  entries: Record<string, PoolEntryInfo[]>
  strategies: Record<string, RotationStrategy>
  poolLoading: boolean

  refresh: () => Promise<void>
  saveKey: (providerId: string, apiKey: string) => Promise<void>
  deleteKey: (providerId: string) => Promise<void>

  refreshPool: (providerId: string) => Promise<void>
  addPoolKey: (providerId: string, label: string, apiKey: string) => Promise<void>
  removePoolKey: (providerId: string, credentialId: string) => Promise<void>
  setPoolStrategy: (providerId: string, strategy: RotationStrategy) => Promise<void>
  resetPoolCooldowns: (providerId: string) => Promise<void>
}

export const useAuthStore = create<AuthStoreState>((set, get) => ({
  status: {},
  loading: false,
  entries: {},
  strategies: {},
  poolLoading: false,

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
    set((s) => ({ status: { ...s.status, [providerId]: true } }))
  },

  deleteKey: async (providerId) => {
    await call('auth_delete_credential', { providerId })
    set((s) => ({ status: { ...s.status, [providerId]: false } }))
  },

  refreshPool: async (providerId) => {
    set({ poolLoading: true })
    try {
      const [entries, strategy] = await Promise.all([
        call<PoolEntryInfo[]>('cmd_pool_list', { providerId }),
        call<RotationStrategy>('cmd_pool_get_strategy', { providerId }),
      ])
      set((s) => ({
        entries: { ...s.entries, [providerId]: entries ?? [] },
        strategies: { ...s.strategies, [providerId]: strategy ?? 'fill_first' },
      }))
    } finally {
      set({ poolLoading: false })
    }
  },

  addPoolKey: async (providerId, label, apiKey) => {
    await call('cmd_pool_add', { providerId, label, apiKey })
    await get().refreshPool(providerId)
  },

  removePoolKey: async (providerId, credentialId) => {
    await call('cmd_pool_remove', { providerId, credentialId })
    await get().refreshPool(providerId)
  },

  setPoolStrategy: async (providerId, strategy) => {
    await call('cmd_pool_set_strategy', { providerId, strategy })
    set((s) => ({ strategies: { ...s.strategies, [providerId]: strategy } }))
  },

  resetPoolCooldowns: async (providerId) => {
    await call('cmd_pool_reset_cooldowns', { providerId })
    await get().refreshPool(providerId)
  },
}))
