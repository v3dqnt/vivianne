import { create } from 'zustand'
import { call } from '../../../ipc'
import type { CustomApiFormat, CustomModel, CustomProvider, CustomProviderConfig } from '../lib/customProviders'

/** Same read-modify-write pattern `llamaStore.ts` uses for `localEndpoints`
 * — spread the existing settings blob so unrelated keys survive untouched. */
async function persist(providers: CustomProvider[]) {
  const settings = (await call<Record<string, unknown>>('cmd_get_settings')) ?? {}
  const configs: CustomProviderConfig[] = providers.map(({ hasKey: _hasKey, ...rest }) => rest)
  await call('cmd_save_settings', { settings: { ...settings, customProviders: configs } })
}

interface CustomProviderStoreState {
  providers: CustomProvider[]
  loading: boolean
  refresh: () => Promise<void>
  addProvider: (input: { name: string; baseUrl: string; apiFormat: CustomApiFormat; apiKey: string }) => Promise<string>
  updateProvider: (id: string, patch: { name?: string; baseUrl?: string; apiFormat?: CustomApiFormat }) => Promise<void>
  setApiKey: (id: string, apiKey: string) => Promise<void>
  removeProvider: (id: string) => Promise<void>
  addModel: (providerId: string, model: CustomModel) => Promise<void>
  removeModel: (providerId: string, modelId: string) => Promise<void>
}

export const useCustomProviderStore = create<CustomProviderStoreState>((set, get) => ({
  providers: [],
  loading: false,

  refresh: async () => {
    set({ loading: true })
    try {
      const settings = (await call<Record<string, unknown>>('cmd_get_settings')) ?? {}
      const raw = Array.isArray(settings.customProviders) ? (settings.customProviders as CustomProviderConfig[]) : []
      const withKeys = await Promise.all(
        raw.map(async (p) => ({ ...p, hasKey: (await call<boolean>('auth_is_authenticated', { providerId: p.id })) ?? false })),
      )
      set({ providers: withKeys })
    } finally {
      set({ loading: false })
    }
  },

  addProvider: async ({ name, baseUrl, apiFormat, apiKey }) => {
    // `provider_registry.rs`'s `provider_spec()` reads the wire format
    // straight out of this id (`custom:<apiFormat>:<uuid>`) — it has no
    // other way to know which of the four request shapes a given custom
    // provider speaks, since it's a pure string→spec function with no
    // access to this store's state.
    const id = `custom:${apiFormat}:${crypto.randomUUID()}`
    const next: CustomProvider = { id, name, baseUrl, apiFormat, models: [], hasKey: false }
    const providers = [...get().providers, next]
    set({ providers })
    await persist(providers)
    if (apiKey.trim()) await get().setApiKey(id, apiKey.trim())
    return id
  },

  updateProvider: async (id, patch) => {
    const providers = get().providers.map((p) => (p.id === id ? { ...p, ...patch } : p))
    set({ providers })
    await persist(providers)
  },

  setApiKey: async (id, apiKey) => {
    await call('auth_store_api_key', { providerId: id, apiKey })
    set({ providers: get().providers.map((p) => (p.id === id ? { ...p, hasKey: true } : p)) })
  },

  removeProvider: async (id) => {
    await call('auth_delete_credential', { providerId: id }).catch(() => {})
    const providers = get().providers.filter((p) => p.id !== id)
    set({ providers })
    await persist(providers)
  },

  addModel: async (providerId, model) => {
    const providers = get().providers.map((p) => (p.id === providerId ? { ...p, models: [...p.models, model] } : p))
    set({ providers })
    await persist(providers)
  },

  removeModel: async (providerId, modelId) => {
    const providers = get().providers.map((p) => (p.id === providerId ? { ...p, models: p.models.filter((m) => m.id !== modelId) } : p))
    set({ providers })
    await persist(providers)
  },
}))
