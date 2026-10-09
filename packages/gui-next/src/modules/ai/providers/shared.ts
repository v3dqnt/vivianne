/**
 * Cross-provider helpers. `fetchWithTimeout` is used directly by the two
 * providers still on hand-rolled fetch (Azure, Bedrock — both need
 * pre-built headers/signed bodies computed in Rust, which doesn't fit the
 * Vercel AI SDK's own request-construction). Everything else here backs the
 * SDK-based providers (Anthropic, OpenAI Chat/Responses, Google) — see
 * `sdkShared.ts` for the message/tool conversion and stream-chunk
 * translation the SDK migration replaced the old hand-rolled SSE parsing
 * with (`parseSseData`, `ThinkTagParser`, `mapOpenAiStopReason`, and the
 * provider-error-normalization helpers are gone — the SDK owns SSE framing
 * and surfaces structured `APICallError`s instead).
 */

/** No response headers within this long = the provider never really answered. */
export const CONNECT_TIMEOUT_MS = 60_000
/** No further stream bytes within this long = the connection is hung. */
export const STREAM_IDLE_TIMEOUT_MS = 90_000

/**
 * `fetch` with a hard timeout on getting response headers back.
 *
 * Plain `fetch()` has no timeout — a provider that accepts the TCP connection
 * and then never responds leaves the await pending indefinitely, which is what
 * made hung requests present as "spinner forever, then nothing". The Rust side
 * has its own 120s idle guard, but that's a last-resort backstop, not a
 * usable UX.
 */
export async function fetchWithTimeout(
  url: string,
  init: RequestInit,
  timeoutMs: number = CONNECT_TIMEOUT_MS,
): Promise<Response> {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  try {
    return await fetch(url, { ...init, signal: controller.signal })
  } catch (e) {
    if (controller.signal.aborted) {
      throw new Error(`Provider did not respond within ${Math.round(timeoutMs / 1000)}s — the connection hung. Try again, or switch model/provider.`)
    }
    throw e
  } finally {
    clearTimeout(timer)
  }
}

/**
 * Idle watchdog for a streaming response. Armed on construction (so a
 * provider that accepts the connection and then never sends a single byte
 * is caught too, not just one that dies mid-response), re-armed on every
 * `poke()` — one poke per stream chunk.
 *
 * On timeout it ABORTS the request through `signal`. That abort is the
 * whole point: the previous version only rejected a `promise` field that no
 * caller ever awaited, so a hung provider was never cancelled and never
 * reported — the request just sat there, the frontend emitted nothing, and
 * the turn hung until Rust's 120s backstop in
 * `call_provider_stream_and_await` gave up. That backstop is where the
 * generic "stalled — no data received for 120s" error came from, on every
 * provider, for what was really a dead frontend timeout.
 */
export class IdleWatchdog {
  private timer: ReturnType<typeof setTimeout> | undefined
  private readonly controller = new AbortController()
  private readonly timeoutMs: number
  private expire: ((e: Error) => void) | undefined
  /** Set once the timeout actually fired — lets a caller tell this abort
   * apart from any other one and report the real cause. */
  timedOut = false

  /**
   * Rejects when the watchdog fires. `Promise.race` this against the stream
   * consumption: aborting `signal` only helps if the SDK actually propagates
   * the abort into its stream iterator, and when it doesn't, `for await`
   * blocks forever and the turn dies on Rust's 120s backstop anyway — which
   * is exactly what was still happening on a real OpenRouter stall with the
   * abort alone. This arm cannot be swallowed.
   *
   * Only rejects if the timer actually fires, and `stop()` (called in the
   * caller's `finally`) clears it — so a normal stream never leaves an
   * unhandled rejection behind.
   */
  readonly expired: Promise<never>

  constructor(timeoutMs: number = STREAM_IDLE_TIMEOUT_MS) {
    this.timeoutMs = timeoutMs
    this.expired = new Promise<never>((_, reject) => {
      this.expire = reject
    })
    // Pre-attach a handler so a caller that never races this arm (or races
    // it and loses) can't turn the timeout into an unhandled rejection —
    // which in the webview is a hard crash, i.e. strictly worse than the
    // stall this class exists to report. `race()` attaches its own handler,
    // so it still observes the rejection normally.
    this.expired.catch(() => {})
    this.arm()
  }

  /** Pass to `streamText({ abortSignal })` / `fetch({ signal })`. */
  get signal(): AbortSignal {
    return this.controller.signal
  }

  get message(): string {
    return `Provider stream stalled — no data for ${Math.round(this.timeoutMs / 1000)}s. The connection hung; try again, or switch model/provider.`
  }

  private arm() {
    this.timer = setTimeout(() => {
      this.timedOut = true
      // Cancel the real HTTP request first, then unblock the caller.
      this.controller.abort(new Error(this.message))
      this.expire?.(new Error(this.message))
    }, this.timeoutMs)
  }

  poke(): void {
    if (this.timer) clearTimeout(this.timer)
    this.arm()
  }

  stop(): void {
    if (this.timer) clearTimeout(this.timer)
  }
}

/**
 * Provider origins for connection preconnect — mirrors the canonical
 * `default_base_url` table in `ai::provider_registry` (Rust). Only the
 * origin matters here (not the exact path/version suffix), since the
 * browser's connection pool is keyed per-origin — warming the TCP+TLS
 * handshake now means it's already up by the time the real request fires.
 */
const PRECONNECT_ORIGINS: Record<string, string> = {
  anthropic: 'https://api.anthropic.com',
  openai: 'https://api.openai.com',
  groq: 'https://api.groq.com',
  xai: 'https://api.x.ai',
  deepseek: 'https://api.deepseek.com',
  together: 'https://api.together.xyz',
  fireworks: 'https://api.fireworks.ai',
  openrouter: 'https://openrouter.ai',
  cerebras: 'https://api.cerebras.ai',
  nvidia: 'https://integrate.api.nvidia.com',
  'nvidia-nim': 'https://integrate.api.nvidia.com',
  huggingface: 'https://router.huggingface.co',
  google: 'https://generativelanguage.googleapis.com',
  mistral: 'https://api.mistral.ai',
}

/** Who is calling, for hosts that attribute requests to apps. OpenRouter
 * reads these (its app rankings, and the gate on models it serves only to
 * listed agent apps); every other host ignores unknown headers. Mirrors the
 * Rust client (`ai/api/openai_completions.rs`). */
export const APP_HEADERS: Record<string, string> = {
  'HTTP-Referer': 'https://github.com/v3dqnt/vivianne',
  'X-Title': 'Vivianne',
}

let lastPreconnected: string | null = null

/** Fire-and-forget: warms the connection to a provider's origin ahead of the
 * real request (on model selection / app init), shaving the TCP+TLS
 * handshake off the critical path of the first token. Opaque response is
 * fine — only the handshake matters, not what comes back. */
export function preconnectProvider(provider: string): void {
  if (provider === lastPreconnected) return
  const origin = PRECONNECT_ORIGINS[provider]
  if (!origin) return
  lastPreconnected = provider
  void fetch(origin, { method: 'HEAD', mode: 'no-cors' }).catch(() => {})
}

/**
 * Parse the affordable-token count out of OpenRouter's low-credit 402 body,
 * e.g. "You requested up to 4096 tokens, but can only afford 38." → 38.
 */
export function parseOpenRouterAffordableTokens(body: string): number | null {
  const marker = 'can only afford '
  const idx = body.indexOf(marker)
  if (idx === -1) return null
  const rest = body.slice(idx + marker.length)
  const match = rest.match(/^\d+/)
  if (!match) return null
  const n = parseInt(match[0], 10)
  return Number.isNaN(n) ? null : n
}
