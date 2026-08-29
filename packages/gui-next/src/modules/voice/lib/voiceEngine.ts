/**
 * Voice engine — STT via native whisper.cpp (Rust backend, unchanged from the
 * pre-existing `voice/mod.rs`), TTS via a fallback chain: Cartesia Sonic
 * (streaming, low-latency, needs an API key) → Kokoro (local ONNX, no key,
 * not streaming) → browser `SpeechSynthesis` (always available, robotic).
 *
 * Every `speak*` function takes an `AbortSignal` so a barge-in (the user
 * starts talking while Vivianne is) can cut playback immediately — that's
 * the whole point of routing TTS through here rather than a bare `new
 * Audio(...).play()` per call site.
 */

import { call } from '../../../ipc'

// ── STT ──────────────────────────────────────────────────────────────────

/** Map a UI model id to the short whisper.cpp model id the backend understands. */
function toWhisperId(model: string): string {
  const m = model.toLowerCase()
  if (m.includes('small.en')) return 'small.en'
  if (m.includes('base.en')) return 'base.en'
  if (m.includes('tiny.en')) return 'tiny.en'
  if (m.includes('small')) return 'small'
  if (m.includes('base')) return 'base'
  if (m.includes('tiny')) return 'tiny'
  return 'tiny.en'
}

let preloadPromise: Promise<void> | null = null

/** Preload (download + build) the whisper.cpp model so the first utterance isn't slow. */
export async function preloadWhisper(model: string): Promise<void> {
  const id = toWhisperId(model)
  if (!preloadPromise) {
    preloadPromise = call<void>('cmd_whisper_preload', { model: id }).then(() => undefined).catch((e) => {
      preloadPromise = null
      throw e
    })
  }
  return preloadPromise
}

/** whisper.cpp expects 16 kHz mono f32 — the mic pipeline captures at that
 * rate already, so samples pass straight through. */
export async function transcribe(model: string, audio: Float32Array): Promise<string> {
  const id = toWhisperId(model)
  const pcm = Array.from(audio)
  const text = await call<string>('cmd_whisper_transcribe', { pcm, model: id, language: 'en' })
  return (text ?? '').trim()
}

// ── Gapless streaming PCM playback (shared by Cartesia) ─────────────────

/** Schedules incoming PCM16LE chunks back-to-back on a single AudioContext
 * so playback starts the instant the first chunk arrives instead of waiting
 * for the whole utterance — this is what actually delivers Cartesia's
 * low-latency win; buffering the full response first would throw it away. */
class PcmStreamPlayer {
  private ctx: AudioContext
  private nextStartTime = 0
  private sources: AudioBufferSourceNode[] = []
  private endedCount = 0
  private totalScheduled = 0
  private onAllEnded: (() => void) | null = null
  private aborted = false
  private sampleRate: number

  constructor(sampleRate: number) {
    this.sampleRate = sampleRate
    this.ctx = new AudioContext({ sampleRate })
  }

  /** Decodes signed 16-bit little-endian PCM and schedules it for playback. */
  pushPcm16(bytes: Uint8Array) {
    if (this.aborted) return
    const sampleCount = bytes.length / 2
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
    const float = new Float32Array(sampleCount)
    for (let i = 0; i < sampleCount; i++) {
      float[i] = view.getInt16(i * 2, true) / 32768
    }
    const buffer = this.ctx.createBuffer(1, sampleCount, this.sampleRate)
    buffer.copyToChannel(float, 0)

    const src = this.ctx.createBufferSource()
    src.buffer = buffer
    src.connect(this.ctx.destination)
    const startAt = Math.max(this.nextStartTime, this.ctx.currentTime)
    this.totalScheduled++
    src.onended = () => {
      this.endedCount++
      if (this.doneReceiving && this.endedCount >= this.totalScheduled) this.onAllEnded?.()
    }
    src.start(startAt)
    this.nextStartTime = startAt + buffer.duration
    this.sources.push(src)
  }

  doneReceiving = false

  /** Resolves once every scheduled chunk has finished playing. Call after
   * the server's "done" message, once no more chunks are coming. */
  waitForDrain(): Promise<void> {
    this.doneReceiving = true
    if (this.endedCount >= this.totalScheduled) return Promise.resolve()
    return new Promise((resolve) => {
      this.onAllEnded = resolve
    })
  }

  abort() {
    this.aborted = true
    for (const s of this.sources) {
      try {
        s.onended = null
        s.stop()
      } catch {
        // already stopped/ended — fine
      }
    }
    this.sources = []
    void this.ctx.close().catch(() => {})
  }
}

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}

// ── Cartesia Sonic (streaming, primary) ──────────────────────────────────

// Cartesia versions its API by release date via this query param. Verify
// against https://docs.cartesia.ai when wiring a real key — a stale date
// here degrades to a clear connection/protocol error, not silent breakage.
const CARTESIA_API_VERSION = '2026-08-14'
const CARTESIA_SAMPLE_RATE = 24000

async function speakCartesia(text: string, apiKey: string, voiceId: string, signal: AbortSignal): Promise<void> {
  if (!apiKey.trim()) throw new Error('No Cartesia API key configured')

  const player = new PcmStreamPlayer(CARTESIA_SAMPLE_RATE)
  const ws = new WebSocket(`wss://api.cartesia.ai/tts/websocket?cartesia_version=${CARTESIA_API_VERSION}&api_key=${encodeURIComponent(apiKey)}`)

  const onAbort = () => {
    player.abort()
    try {
      ws.send(JSON.stringify({ context_id: contextId, cancel: true }))
    } catch {
      // socket may already be closing — the player.abort() above is what
      // actually matters for barge-in latency, this is just tidy shutdown
    }
    try {
      ws.close()
    } catch {
      // ignore
    }
  }
  const contextId = crypto.randomUUID()
  signal.addEventListener('abort', onAbort, { once: true })

  try {
    await new Promise<void>((resolve, reject) => {
      ws.onopen = () => {
        ws.send(
          JSON.stringify({
            model_id: 'sonic-3.5',
            transcript: text,
            voice: voiceId.trim() ? { id: voiceId } : 'default',
            context_id: contextId,
            output_format: { container: 'raw', encoding: 'pcm_s16le', sample_rate: CARTESIA_SAMPLE_RATE },
          }),
        )
      }
      ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(ev.data as string) as { type: string; data?: string; done?: boolean; error?: string }
          if (msg.type === 'chunk' && msg.data) {
            player.pushPcm16(base64ToBytes(msg.data))
          } else if (msg.type === 'done') {
            ws.close()
            player.waitForDrain().then(resolve, reject)
          } else if (msg.type === 'error') {
            reject(new Error(msg.error ?? 'Cartesia TTS error'))
          }
        } catch (e) {
          reject(e instanceof Error ? e : new Error(String(e)))
        }
      }
      ws.onerror = () => reject(new Error('Cartesia WebSocket error'))
      ws.onclose = (ev) => {
        if (!ev.wasClean && !signal.aborted) reject(new Error(`Cartesia connection closed (${ev.code})`))
      }
    })
  } finally {
    signal.removeEventListener('abort', onAbort)
  }
}

// ── Kokoro (local ONNX, offline fallback) ────────────────────────────────

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let kokoroPromise: Promise<any> | null = null

// eslint-disable-next-line @typescript-eslint/no-explicit-any
async function getKokoro(): Promise<any> {
  if (!kokoroPromise) {
    kokoroPromise = (async () => {
      const mod = await import('kokoro-js')
      return mod.KokoroTTS.from_pretrained('onnx-community/Kokoro-82M-v1.0-ONNX', { dtype: 'q8' })
    })().catch((e) => {
      kokoroPromise = null
      throw e
    })
  }
  return kokoroPromise
}

async function speakKokoro(text: string, voice: string, signal: AbortSignal): Promise<void> {
  const tts = await getKokoro()
  if (signal.aborted) return
  const out = await tts.generate(text, { voice })

  let blob: Blob
  if (typeof out.toBlob === 'function') {
    blob = out.toBlob()
  } else if (typeof out.toWav === 'function') {
    const wav: Uint8Array = out.toWav()
    const buf = wav.buffer.slice(wav.byteOffset, wav.byteOffset + wav.byteLength) as ArrayBuffer
    blob = new Blob([buf], { type: 'audio/wav' })
  } else {
    throw new Error('Kokoro output has neither toBlob nor toWav')
  }

  const url = URL.createObjectURL(blob)
  const el = new Audio(url)
  const onAbort = () => el.pause()
  signal.addEventListener('abort', onAbort, { once: true })
  try {
    await new Promise<void>((resolve) => {
      el.onended = () => resolve()
      el.onerror = () => resolve()
      el.onpause = () => resolve()
      el.play().catch(() => resolve())
    })
  } finally {
    signal.removeEventListener('abort', onAbort)
    URL.revokeObjectURL(url)
  }
}

// ── Browser SpeechSynthesis (last-resort fallback) ───────────────────────

function speakBrowser(text: string, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (typeof window === 'undefined' || !window.speechSynthesis) return resolve()
    const u = new SpeechSynthesisUtterance(text)
    const onAbort = () => window.speechSynthesis.cancel()
    signal.addEventListener('abort', onAbort, { once: true })
    const done = () => {
      signal.removeEventListener('abort', onAbort)
      resolve()
    }
    u.onend = done
    u.onerror = done
    window.speechSynthesis.speak(u)
  })
}

// ── Dispatcher ────────────────────────────────────────────────────────────

export interface SpeakOptions {
  engine: 'cartesia' | 'kokoro' | 'browser'
  cartesiaApiKey?: string
  cartesiaVoiceId?: string
  kokoroVoice?: string
  signal: AbortSignal
}

/** Tries the configured engine, then falls back down the chain on failure —
 * mirrors the old voiceEngine's behavior, just with Cartesia added ahead of
 * Kokoro. A user-triggered abort (barge-in) is NOT a failure to fall back
 * from — it propagates as a normal early return. */
export async function speak(text: string, opts: SpeakOptions): Promise<void> {
  if (!text.trim() || opts.signal.aborted) return
  const chain: Array<() => Promise<void>> = []
  if (opts.engine === 'cartesia') chain.push(() => speakCartesia(text, opts.cartesiaApiKey ?? '', opts.cartesiaVoiceId ?? '', opts.signal))
  if (opts.engine === 'cartesia' || opts.engine === 'kokoro') chain.push(() => speakKokoro(text, opts.kokoroVoice ?? 'af_heart', opts.signal))
  chain.push(() => speakBrowser(text, opts.signal))

  for (const attempt of chain) {
    if (opts.signal.aborted) return
    try {
      await attempt()
      return
    } catch (e) {
      if (opts.signal.aborted) return
      console.error('[voice] TTS attempt failed, falling back:', e)
    }
  }
}
