/**
 * llama.cpp local model endpoints.
 *
 * Backed by two real, already-wired Rust pieces (verified directly against
 * `agent::llama_manager` and `ai::local_registry`, not assumed from the old
 * frontend):
 *   - `LlamaManager` (`cmd_llama_*`) owns the `llama-server` subprocess
 *     lifecycle for a given GGUF path/port/ngl/ctx-size.
 *   - `LocalEndpointRegistry` (`cmd_*_local_endpoint`) is what
 *     `cmd_agent_set_model` actually reads the base URL from when a model's
 *     provider is `"local"` and no `baseUrl` was passed directly — so an
 *     endpoint only becomes *selectable* once it's registered there, which
 *     this module does exactly when (and only when) the server is actually
 *     running, not just configured.
 *
 * Persisted the same way the backend already expects: `settings.json`'s
 * `localEndpoints` array (`{ kind: "llama", config: {...} }[]`) is what
 * `lib.rs`'s auto-start-on-launch block reads — reusing that key means an
 * endpoint added here also survives a restart with no backend change, and
 * doesn't clobber non-llama entries some other client might add later.
 */

// `LlamaStatus` carries `#[serde(rename_all = "camelCase")]` in Rust, which
// lowercases the first letter of each single-word unit variant — the real
// wire values are "starting"/"running"/"stopped" and `{ crashed: "..." }`,
// NOT the Rust source's PascalCase spelling. Verified against the actual
// serialized output, not assumed from the enum's Rust identifiers.
export type LlamaStatus = 'starting' | 'running' | 'stopped' | { crashed: string }

/** Which local server binary an endpoint runs — both take a GGUF file and
 * expose an OpenAI-compatible `/v1` API, so the model-routing/chat side
 * never needs to know which one it's talking to. */
export type LocalEngine = 'llamaCpp' | 'mistralRs'

/** Optional llama.cpp performance flags. Mirrors
 * `agent::llama_manager::LlamaTuning`; every field omitted means "let the
 * engine decide", which is what every endpoint saved before this existed
 * does. */
export interface LlamaTuning {
  /** `-t`. llama.cpp's own default is derived from the whole CPU, which
   * over-subscribes on hybrid P+E-core machines. */
  threads?: number | null
  /** `--device`, e.g. "Vulkan1". Auto-selection is usually right; this is the
   * escape hatch for when it is not. */
  device?: string | null
  /** `-ctk`/`-ctv`. Unset means q8_0: half the KV cache memory at no
   * measured throughput cost. "f16" opts back out. */
  kvCacheType?: string | null
  /** `--mlock`. Opt-in: it can fail without the OS privilege. */
  mlock?: boolean
  /** `--cache-reuse`. Unset means 256; 0 turns it off. */
  cacheReuse?: number | null
  /** `--spec-type`. Unset means "ngram-mod" (lossless, no draft model);
   * "none" turns speculative decoding off. */
  speculative?: string | null
  /** `--cache-ram` MiB of host RAM for the prompt cache. Unset means 2048
   * (llama.cpp's own default is 8192); 0 disables it. */
  cacheRamMib?: number | null
  /** `--sleep-idle-seconds`: unload the model after this long idle. Unset
   * or 0 = never. */
  sleepIdleSeconds?: number | null
  /** Raw extra flags, whitespace-separated. */
  extraArgs?: string | null

  // Placement
  /** `--n-cpu-moe`: experts of the first N layers stay on the CPU. */
  nCpuMoe?: number | null
  /** `-ts`, e.g. "3,1". */
  tensorSplit?: string | null
  /** `-mg`. */
  mainGpu?: number | null
  /** `false` = `--no-mmap`. */
  mmap?: boolean | null
  // Throughput
  batchSize?: number | null
  ubatchSize?: number | null
  parallel?: number | null
  threadsBatch?: number | null
  /** "on" | "off" | "auto". */
  flashAttn?: string | null
  // Sampling defaults
  temperature?: number | null
  topP?: number | null
  topK?: number | null
  minP?: number | null
  repeatPenalty?: number | null
  // The model
  alias?: string | null
  chatTemplate?: string | null
  mmproj?: string | null
  lora?: string | null
  ropeScale?: number | null
}

/** What a GGUF file says about itself (`local_models::GgufInfo`). */
export interface GgufInfo {
  name: string | null
  architecture: string | null
  sizeLabel: string | null
  contextLength: number | null
  expertCount: number | null
  fileType: number | null
  format: string | null
  formatNote: string | null
  fileBytes: number
}

export interface LlamaEndpoint {
  id: string
  name: string
  modelPath: string
  port: number
  ngl: number
  ctxSize: number
  autoStart: boolean
  engine: LocalEngine
  tuning?: LlamaTuning
  status: LlamaStatus
}

export function engineLabel(engine: LocalEngine): string {
  return engine === 'mistralRs' ? 'mistral.rs' : 'llama.cpp'
}

/** The composite model id `cmd_agent_set_model` expects for a llama endpoint. */
export function llamaModelId(endpointId: string): string {
  return `llama:${endpointId}`
}

export function statusLabel(status: LlamaStatus): string {
  if (typeof status === 'string') return status.charAt(0).toUpperCase() + status.slice(1)
  return `Crashed: ${status.crashed}`
}

export function isRunning(status: LlamaStatus): boolean {
  return status === 'running'
}

export function isBusyStatus(status: LlamaStatus): boolean {
  return status === 'starting'
}
