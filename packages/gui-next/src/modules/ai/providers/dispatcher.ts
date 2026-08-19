/**
 * Listens for the Rust backend's `provider_stream_request` event and routes
 * it to the matching provider implementation, which streams
 * `ai-stream-token` events back — the exact event Rust's
 * `call_provider_stream_and_await` already listens for, so nothing on the
 * Rust consumer side changes.
 *
 * Provider modules are dynamically imported per request rather than
 * statically imported up front — `universalProvider.ts` pulls in three
 * Vercel AI SDK provider packages, and a session only ever exercises one or
 * two of the six providers. A static import paid for all providers' SDK
 * packages on every launch regardless of which model was selected; dynamic
 * import only loads (and only once, since ESM caches the module) whichever
 * provider(s) actually get used.
 */
import { listen, emit } from '@tauri-apps/api/event'
import type { ProviderStreamRequest } from './types'
import { emptyUsage, now } from './types'

let initialized = false

const SDK_API_KINDS = new Set(['anthropicMessages', 'openAiChat', 'mistralChat', 'openAiResponses', 'googleGenerative'])

/**
 * End a request with an error. Rust treats a `done: true` payload as the end
 * of the turn, so this is what turns any frontend-side failure into an
 * immediate, named error instead of dead air until Rust's 120s idle guard.
 */
function failRequest(req: ProviderStreamRequest, message: string) {
  void emit('ai-stream-token', {
    streamId: req.streamId,
    delta: '',
    done: true,
    isThinking: false,
    message: {
      role: 'assistant',
      content: [],
      api: req.apiKind,
      provider: req.provider,
      model: req.modelId,
      usage: emptyUsage(),
      stopReason: 'error',
      errorMessage: message,
      timestamp: now(),
      diagnostics: null,
    },
  })
}

function route(req: ProviderStreamRequest): Promise<void> {
  if (SDK_API_KINDS.has(req.apiKind)) return import('./universalProvider').then((m) => m.streamUniversal(req))
  if (req.apiKind === 'azureOpenAiResponses') return import('./azureOpenai').then((m) => m.streamAzureOpenAi(req))
  if (req.apiKind === 'bedrock') return import('./bedrock').then((m) => m.streamBedrock(req))
  return Promise.reject(
    new Error(
      `No provider implementation is wired up for API kind "${req.apiKind}" (provider: ${req.provider}). This is a bug in Vivianne, not a problem with your key.`,
    ),
  )
}

export async function initProviderDispatcher(): Promise<void> {
  if (initialized) return
  initialized = true
  try {
    await listen<ProviderStreamRequest>('provider_stream_request', (event) => {
      const req = event.payload
      // Never drop a request. A rejected dynamic import (chunk load failure
      // after an update), an unexpected throw inside a provider, or an
      // unrouted apiKind all used to leave Rust waiting on `ai-stream-token`
      // events that could never arrive — the turn hung for the full 120s
      // idle guard with nothing to explain it.
      route(req).catch((e) => failRequest(req, e instanceof Error ? e.message : String(e)))
    })
  } catch (e) {
    // Registration itself failed — allow a later call to retry rather than
    // leaving the app permanently unable to answer any request at all.
    initialized = false
    throw e
  }
}
