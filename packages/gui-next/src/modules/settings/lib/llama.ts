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

export interface LlamaEndpoint {
  id: string
  name: string
  modelPath: string
  port: number
  ngl: number
  ctxSize: number
  autoStart: boolean
  engine: LocalEngine
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
