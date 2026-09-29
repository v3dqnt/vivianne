import { create } from 'zustand'
import { call } from '../../../ipc'
import type { LlamaEndpoint, LlamaTuning, LocalEngine } from '../lib/llama'
import { llamaModelId } from '../lib/llama'

/**
 * Persists to `settings.json`'s `localEndpoints` array — the same key
 * `lib.rs`'s auto-start-on-launch block already reads, so an endpoint added
 * here survives a restart with zero backend changes. Only touches `kind ===
 * "llama"` entries; anything else in that array (other endpoint kinds) is
 * left as-is.
 */
async function syncPersistedEndpoints(endpoints: LlamaEndpoint[]) {
  const settings = (await call<Record<string, unknown>>('cmd_get_settings')) ?? {}
  const all = Array.isArray(settings.localEndpoints) ? (settings.localEndpoints as { kind?: string }[]) : []
  const nonLlama = all.filter((e) => e?.kind !== 'llama')
  const llama = endpoints.map(({ id, name, modelPath, port, ngl, ctxSize, autoStart, engine, tuning }) => ({
    kind: 'llama',
    config: { id, name, modelPath, port, ngl, ctxSize, autoStart, engine, tuning },
  }))
  await call('cmd_save_settings', { settings: { ...settings, localEndpoints: [...nonLlama, ...llama] } })
}

/** Default context window for a new local model. Local models get the lean
 * tool profile (`agent::tool_profile`), so the first request is ~6–8k tokens
 * rather than the ~20k a cloud model is sent — but 8192 would still leave
 * almost nothing for the conversation. 32768 is the trained context of the
 * common coding models and, with the default q8_0 KV cache, fits beside a
 * 9B Q4 model on an 8 GB card. */
export const DEFAULT_CTX_SIZE = 32768
export const DEFAULT_NGL = 999

interface LlamaStoreState {
  endpoints: LlamaEndpoint[]
  refresh: () => Promise<void>
  addEndpoint: (
    modelPath: string,
    name: string,
    ctxSize?: number,
    ngl?: number,
    engine?: LocalEngine,
    tuning?: LlamaTuning,
  ) => Promise<void>
  /** Change an existing endpoint's config (context size, GPU layers, name).
   * Stops the server if running — the change only takes effect on next start. */
  updateEndpoint: (
    id: string,
    patch: { name?: string; ctxSize?: number; ngl?: number; engine?: LocalEngine; tuning?: LlamaTuning },
  ) => Promise<void>
  removeEndpoint: (id: string) => Promise<void>
  start: (id: string) => Promise<void>
  stop: (id: string) => Promise<void>
  pickGgufFile: () => Promise<string | null>
}

let pollHandle: ReturnType<typeof setInterval> | null = null

export const useLlamaStore = create<LlamaStoreState>((set, get) => ({
  endpoints: [],

  refresh: async () => {
    const list = (await call<LlamaEndpoint[]>('cmd_llama_list')) ?? []
    set({ endpoints: list })
    // Poll while anything is starting/running — a crash or the ~30s health
    // check finishing should show up without reopening Settings.
    const active = list.some((e) => e.status === 'starting' || e.status === 'running')
    if (active && !pollHandle) pollHandle = setInterval(() => void get().refresh(), 3000)
    else if (!active && pollHandle) {
      clearInterval(pollHandle)
      pollHandle = null
    }
  },

  pickGgufFile: async () => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({ multiple: false, filters: [{ name: 'GGUF model', extensions: ['gguf'] }] })
    return !selected || Array.isArray(selected) ? null : selected
  },

  addEndpoint: async (modelPath, name, ctxSize = DEFAULT_CTX_SIZE, ngl = DEFAULT_NGL, engine = 'llamaCpp', tuning) => {
    const id = crypto.randomUUID()
    // Default port steps past whatever's already claimed, so a second/third
    // endpoint doesn't collide with one already running.
    const usedPorts = new Set(get().endpoints.map((e) => e.port))
    let port = 8081
    while (usedPorts.has(port)) port++
    await call('cmd_llama_register', { id, name, modelPath, port, ngl, ctxSize, autoStart: false, engine, tuning: tuning ?? null })
    await get().refresh()
    await syncPersistedEndpoints(get().endpoints)
  },

  updateEndpoint: async (id, patch) => {
    const ep = get().endpoints.find((e) => e.id === id)
    if (!ep) return
    const next = { ...ep, ...patch }
    // `register` is insert-only (won't overwrite an existing id), and the
    // running server has the old ctx baked in — so a config change means
    // unregister (which stops it) then register fresh. Takes effect on the
    // next Start.
    await call('cmd_llama_unregister', { id })
    await call('cmd_unregister_local_endpoint', { id }).catch(() => {})
    await call('cmd_llama_register', {
      id,
      name: next.name,
      modelPath: next.modelPath,
      port: next.port,
      ngl: next.ngl,
      ctxSize: next.ctxSize,
      autoStart: next.autoStart,
      engine: next.engine,
      tuning: next.tuning ?? null,
    })
    await get().refresh()
    await syncPersistedEndpoints(get().endpoints)
  },

  removeEndpoint: async (id) => {
    await call('cmd_llama_unregister', { id }) // stops the process first internally
    await call('cmd_unregister_local_endpoint', { id }).catch(() => {})
    const endpoints = get().endpoints.filter((e) => e.id !== id)
    set({ endpoints })
    await syncPersistedEndpoints(endpoints)
  },

  start: async (id) => {
    await call('cmd_llama_start', { id })
    const endpoint = get().endpoints.find((e) => e.id === id)
    if (endpoint) {
      // Only registered as selectable once actually started — a model
      // pointed at a server that was never started would just hang on
      // connection-refused instead of not existing in the picker.
      await call('cmd_register_local_endpoint', {
        id,
        name: endpoint.name,
        baseUrl: `http://127.0.0.1:${endpoint.port}/v1`,
        modelId: llamaModelId(id),
        kind: 'llama',
        apiKey: null,
      })
    }
    await get().refresh()
  },

  stop: async (id) => {
    await call('cmd_llama_stop', { id })
    await call('cmd_unregister_local_endpoint', { id })
    await get().refresh()
  },
}))
