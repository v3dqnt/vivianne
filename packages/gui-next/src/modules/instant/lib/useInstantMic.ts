import { useCallback, useEffect, useRef } from 'react'
import { call } from '../../../ipc'

/** whisper.cpp's expected input rate. Resampling is left to the browser by
 * asking the AudioContext for this rate directly. */
const SAMPLE_RATE = 16_000

/** RMS above which a frame counts as speech. Empirical: low enough for a
 * normal speaking voice at arm's length, high enough that fan noise and key
 * clicks do not hold the utterance open forever. */
const ENERGY_THRESHOLD = 0.008

/** Silence after speech before the utterance is considered finished. */
const SILENCE_TAIL_MS = 900

/** Ignore blips shorter than this — a cough or a door should not become a
 * command. */
const MIN_UTTERANCE_MS = 400

/** Hard ceiling, so a stuck-open mic cannot buffer forever. */
const MAX_UTTERANCE_MS = 15_000

/**
 * How often to decode a rough transcript while the user is still talking.
 *
 * This is purely so the notch can show words appearing as they are spoken —
 * the confirmation that it heard you, which otherwise only arrives after you
 * stop. Every interim decode competes with the final one for the same single
 * whisper state, so this is deliberately slow enough to stay out of its way;
 * an interim is skipped outright whenever a decode is already running.
 */
const INTERIM_EVERY_MS = 700

/**
 * The capture worklet, inlined.
 *
 * An `AudioWorkletNode` rather than the deprecated `ScriptProcessorNode` the
 * old voice pipeline used: ScriptProcessor runs its callback on the main
 * thread, so it glitches under render pressure — and this overlay is
 * eventually going to have an animated 3D character in the same webview.
 *
 * Inlined as a blob URL because a worklet must be loaded from a URL, and a
 * separate asset file would need Vite plumbing to stay resolvable in both dev
 * and the bundled app.
 */
const WORKLET_SOURCE = `
class InstantCapture extends AudioWorkletProcessor {
  process(inputs) {
    const input = inputs[0]
    if (input && input[0]) {
      // Copy: the underlying buffer is reused by the audio thread after this
      // returns, so posting it directly would deliver whatever comes next.
      this.port.postMessage(new Float32Array(input[0]))
    }
    return true
  }
}
registerProcessor('instant-capture', InstantCapture)
`

function rms(frame: Float32Array): number {
  let sum = 0
  for (let i = 0; i < frame.length; i++) sum += frame[i] * frame[i]
  return Math.sqrt(sum / frame.length)
}

interface MicCallbacks {
  /** A complete utterance, already transcribed. Empty strings are filtered. */
  onUtterance: (text: string) => void
  /** Speech detected — used to show a live indicator. */
  onSpeechStart?: () => void
  /** A rough transcript of speech still in progress. Overwritten repeatedly
   * and never acted on — only the final `onUtterance` text is executed. */
  onInterim?: (text: string) => void
  /** Loudness of each audio frame (RMS, 0..1), for a live level meter. */
  onLevel?: (level: number) => void
  onError?: (message: string) => void
  /**
   * Whisper model id. Read from settings by the caller rather than here so the
   * hook stays a pure audio concern; `undefined` lets the Rust side pick its
   * own default instead of hard-coding a second one on this side.
   */
  model?: string
}

/**
 * Microphone capture with energy-based voice activity detection.
 *
 * Buffers speech, and when the speaker stops, ships the whole utterance to
 * whisper.cpp in one call. Endpointing is pure VAD: a Jev-based "are they
 * finished?" decision slots in where `SILENCE_TAIL_MS` is applied, and would
 * shave most of that tail off the perceived latency.
 */
export function useInstantMic({ onUtterance, onSpeechStart, onInterim, onError, onLevel, model }: MicCallbacks) {
  const ctxRef = useRef<AudioContext | null>(null)
  const streamRef = useRef<MediaStream | null>(null)
  const nodeRef = useRef<AudioWorkletNode | null>(null)
  // Callbacks in refs so starting the mic does not depend on their identity —
  // otherwise every parent re-render would tear down and restart capture.
  const cbs = useRef({ onUtterance, onSpeechStart, onInterim, onError, onLevel, model })
  cbs.current = { onUtterance, onSpeechStart, onInterim, onError, onLevel, model }

  const stop = useCallback(() => {
    nodeRef.current?.port.close()
    nodeRef.current?.disconnect()
    nodeRef.current = null
    streamRef.current?.getTracks().forEach((t) => t.stop())
    streamRef.current = null
    void ctxRef.current?.close()
    ctxRef.current = null
  }, [])

  const start = useCallback(async () => {
    if (ctxRef.current) return
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          channelCount: 1,
          // The browser's own cleanup is better than anything done after the
          // fact, and whisper is noticeably more accurate with it on.
          echoCancellation: true,
          noiseSuppression: true,
          autoGainControl: true,
        },
      })
      streamRef.current = stream

      const ctx = new AudioContext({ sampleRate: SAMPLE_RATE })
      ctxRef.current = ctx
      const blob = new Blob([WORKLET_SOURCE], { type: 'application/javascript' })
      const url = URL.createObjectURL(blob)
      try {
        await ctx.audioWorklet.addModule(url)
      } finally {
        URL.revokeObjectURL(url)
      }

      const node = new AudioWorkletNode(ctx, 'instant-capture')
      nodeRef.current = node
      ctx.createMediaStreamSource(stream).connect(node)
      // Not connected to the destination: this is a tap, and routing it to the
      // speakers would echo the user back at themselves.

      let buffer: Float32Array[] = []
      let speaking = false
      let speechStartedAt = 0
      let lastVoiceAt = 0
      let busy = false
      let lastInterimAt = 0

      /** Decode what has been captured so far, without consuming the buffer. */
      const interim = async () => {
        // Never at the expense of the real decode: an interim that queued
        // ahead of the final transcript would add its own latency to the one
        // result that actually matters.
        if (busy) return
        const total = buffer.reduce((n, f) => n + f.length, 0)
        if (total === 0) return
        const pcm = new Float32Array(total)
        let at = 0
        for (const f of buffer) {
          pcm.set(f, at)
          at += f.length
        }
        busy = true
        try {
          const text = await call<string>('cmd_instant_stt_transcribe', {
            pcm: Array.from(pcm),
            model: cbs.current.model,
            partial: true,
          })
          const clean = (text ?? '').trim()
          if (clean) cbs.current.onInterim?.(clean)
        } catch {
          // Interims are cosmetic; a failed one must never surface as an error.
        } finally {
          busy = false
        }
      }

      const flush = async () => {
        const frames = buffer
        buffer = []
        speaking = false
        const total = frames.reduce((n, f) => n + f.length, 0)
        if (total === 0) return
        const pcm = new Float32Array(total)
        let at = 0
        for (const f of frames) {
          pcm.set(f, at)
          at += f.length
        }
        // One decode at a time. Whisper saturates a core, and overlapping
        // calls would queue behind each other on the Rust side anyway while
        // making the ordering of results ambiguous.
        if (busy) return
        busy = true
        try {
          const text = await call<string>('cmd_instant_stt_transcribe', {
            pcm: Array.from(pcm),
            model: cbs.current.model,
            partial: false,
          })
          const clean = (text ?? '').trim()
          if (clean) cbs.current.onUtterance(clean)
        } catch (e) {
          cbs.current.onError?.(e instanceof Error ? e.message : String(e))
        } finally {
          busy = false
        }
      }

      node.port.onmessage = (ev: MessageEvent<Float32Array>) => {
        const frame = ev.data
        const now = performance.now()
        const level = rms(frame)
        cbs.current.onLevel?.(level)
        const loud = level > ENERGY_THRESHOLD

        if (loud) {
          if (!speaking) {
            speaking = true
            speechStartedAt = now
            cbs.current.onSpeechStart?.()
          }
          lastVoiceAt = now
        }
        if (speaking) {
          buffer.push(frame)
          const spoken = now - speechStartedAt
          const quietFor = now - lastVoiceAt
          if (spoken > MAX_UTTERANCE_MS || (quietFor > SILENCE_TAIL_MS && spoken > MIN_UTTERANCE_MS)) {
            void flush()
          } else if (quietFor > SILENCE_TAIL_MS) {
            // Long enough silence but too short to be speech — drop it rather
            // than transcribing a cough.
            buffer = []
            speaking = false
            lastInterimAt = 0
          } else if (now - lastInterimAt > INTERIM_EVERY_MS && spoken > MIN_UTTERANCE_MS) {
            // Still talking — show what has been heard so far.
            lastInterimAt = now
            void interim()
          }
        }
      }
    } catch (e) {
      stop()
      const msg = e instanceof Error ? e.message : String(e)
      cbs.current.onError?.(
        msg.includes('Permission') || msg.includes('NotAllowed')
          ? 'Microphone access was refused.'
          : `Couldn't start the microphone: ${msg}`,
      )
    }
  }, [stop])

  // Capture must never outlive the overlay — a live mic in a hidden window is
  // exactly the thing a user would be upset to discover.
  useEffect(() => stop, [stop])

  return { start, stop }
}
