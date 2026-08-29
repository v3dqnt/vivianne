/**
 * Mic capture, energy VAD, and the voice session state machine.
 *
 * Adapted from the pre-existing (deleted, recovered from git history)
 * `VoiceMode.tsx` mic pipeline — same RMS-energy VAD and utterance-finalize
 * logic — with three real changes: no wake-word gate (a session opens
 * explicitly, so there's nothing to detect passively), the same VAD loop
 * also runs DURING playback to detect barge-in, and TTS goes through the
 * abortable `speak()` dispatcher instead of a bare `Audio.play()`.
 */
import { useCallback, useEffect, useRef } from 'react'
import { useAgentStore } from '../../ai/store/agentStore'
import { contentToText } from '../../ai/lib/types'
import { useVoiceStore } from '../store/voiceStore'
import { preloadWhisper, transcribe, speak } from './voiceEngine'

const SAMPLE_RATE = 16000
const MIN_UTTERANCE_MS = 400
const SILENCE_TAIL_MS = 900
const ENERGY_THRESHOLD = 0.008
const MAX_UTTERANCE_MS = 15000
// Barge-in needs a slightly higher bar and a short sustained window — a
// single loud frame (a cough, a chair creak) shouldn't cut Vivianne off.
const BARGE_IN_THRESHOLD = 0.012
const BARGE_IN_SUSTAIN_MS = 150

const GREETINGS = ['Hey, listening.', 'Yes?', 'Go ahead.', "What's up?"]

function voicePrompt(command: string): string {
  return `[voice command] ${command}

You are Vivianne answering by voice. Follow these rules:
- Reply briefly and conversationally — your answer is read aloud, so no markdown, lists, or code blocks unless truly necessary.
- Screenshot ONLY when the answer genuinely requires seeing the screen (e.g. "what's on my screen", "what does this error say", "read this for me"). Call the computer tool with action="screenshot" for that, then describe what you see. For everything else, do NOT take a screenshot.
- Use your other tools when they help: web_search/web_fetch for current facts, bash/powershell to inspect or act on the system.
- Keep it to a sentence or two unless asked for detail.`
}

async function openMic(): Promise<{ stream: MediaStream; ctx: AudioContext; source: MediaStreamAudioSourceNode } | null> {
  try {
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, sampleRate: SAMPLE_RATE },
    })
    const ctx = new AudioContext({ sampleRate: SAMPLE_RATE })
    if (ctx.state === 'suspended') await ctx.resume()
    const source = ctx.createMediaStreamSource(stream)
    return { stream, ctx, source }
  } catch (e) {
    console.error('[voice] mic open failed', e)
    return null
  }
}

export function useVoicePipeline() {
  const settings = useVoiceStore()
  const messages = useAgentStore((s) => s.messages)
  const isBusy = useAgentStore((s) => s.isBusy)
  const streamingIndex = useAgentStore((s) => s.streamingIndex)
  const sendMessage = useAgentStore((s) => s.sendMessage)

  const sessionOpenRef = useRef(false)
  const runningRef = useRef(false)
  const capturingRef = useRef(false)
  const bufRef = useRef<Float32Array[]>([])
  const activeStartRef = useRef(0)
  const lastVoiceMsRef = useRef(0)
  const tickRef = useRef(0)
  const bargeVoiceStartRef = useRef<number | null>(null)
  const audioRef = useRef<{ stream: MediaStream; ctx: AudioContext; source: MediaStreamAudioSourceNode } | null>(null)
  const processorRef = useRef<ScriptProcessorNode | null>(null)
  const finalizeInFlightRef = useRef(false)
  // The reply text already handed to TTS, tracked as a content PREFIX so a
  // reload/re-render with identical text doesn't re-speak it, but genuinely
  // new text (including mid-stream growth) speaks only the delta.
  const spokenPrefixRef = useRef('')
  const ttsAbortRef = useRef<AbortController | null>(null)
  const speakQueueRef = useRef<Promise<void>>(Promise.resolve())

  const setStatus = useVoiceStore((s) => s.setStatus)
  const setTranscript = useVoiceStore((s) => s.setTranscript)
  const setLastReply = useVoiceStore((s) => s.setLastReply)
  const setError = useVoiceStore((s) => s.setError)

  const flattenBuffer = useCallback((): Float32Array => {
    const chunks = bufRef.current
    const total = chunks.reduce((a, b) => a + b.length, 0)
    const out = new Float32Array(total)
    let o = 0
    for (const c of chunks) {
      out.set(c, o)
      o += c.length
    }
    return out
  }, [])

  const enqueueSpeech = useCallback(
    (text: string) => {
      if (!text.trim()) return
      speakQueueRef.current = speakQueueRef.current.then(async () => {
        if (!sessionOpenRef.current) return
        ttsAbortRef.current?.abort()
        const controller = new AbortController()
        ttsAbortRef.current = controller
        setStatus('speaking')
        await speak(text, {
          engine: settings.ttsEngine,
          cartesiaApiKey: settings.cartesiaApiKey,
          cartesiaVoiceId: settings.cartesiaVoiceId,
          kokoroVoice: settings.kokoroVoice,
          signal: controller.signal,
        })
        if (sessionOpenRef.current && !controller.signal.aborted) setStatus('listening')
      })
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [settings.ttsEngine, settings.cartesiaApiKey, settings.cartesiaVoiceId, settings.kokoroVoice],
  )

  const finalizeUtterance = useCallback(async () => {
    if (finalizeInFlightRef.current) return
    finalizeInFlightRef.current = true
    const audio = flattenBuffer()
    bufRef.current = []
    capturingRef.current = false
    setStatus('transcribing')
    try {
      const text = await transcribe(settings.whisperModel, audio)
      if (!text) {
        setStatus('listening')
        return
      }
      setTranscript(text)
      spokenPrefixRef.current = ''
      setLastReply('')
      setStatus('responding')
      await sendMessage(voicePrompt(text))
    } catch (e) {
      console.error('[voice] finalize failed', e)
      setError(e instanceof Error ? e.message : String(e))
      setStatus('error')
      window.setTimeout(() => {
        if (sessionOpenRef.current) {
          setError(null)
          setStatus('listening')
        }
      }, 4000)
    } finally {
      finalizeInFlightRef.current = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [flattenBuffer, sendMessage, settings.whisperModel])

  const beginCapture = useCallback(() => {
    ttsAbortRef.current?.abort()
    capturingRef.current = true
    bufRef.current = []
    activeStartRef.current = tickRef.current
    lastVoiceMsRef.current = tickRef.current
    setTranscript('')
    setStatus('listening')
  }, [setStatus, setTranscript])

  const handlersRef = useRef({ finalizeUtterance, beginCapture })
  handlersRef.current = { finalizeUtterance, beginCapture }

  // ── Mic loop: runs for the entire session, both while listening and while
  // Vivianne is speaking (that second case is what makes barge-in possible). ──
  const openSession = useCallback(async () => {
    if (sessionOpenRef.current) return
    sessionOpenRef.current = true
    setStatus('starting')
    setError(null)

    await preloadWhisper(settings.whisperModel).catch((e) => {
      console.error('[voice] whisper preload failed', e)
    })

    const audio = await openMic()
    if (!sessionOpenRef.current) {
      audio?.stream.getTracks().forEach((t) => t.stop())
      void audio?.ctx.close()
      return
    }
    if (!audio) {
      setError('Microphone unavailable. Grant mic permission and try again.')
      setStatus('error')
      sessionOpenRef.current = false
      return
    }
    audioRef.current = audio

    const proc = audio.ctx.createScriptProcessor(4096, 1, 1)
    processorRef.current = proc
    const chunkMs = (4096 / SAMPLE_RATE) * 1000

    proc.onaudioprocess = (ev) => {
      if (!runningRef.current) return
      const input = ev.inputBuffer.getChannelData(0)
      let sum = 0
      for (let i = 0; i < input.length; i++) sum += input[i] * input[i]
      const rms = Math.sqrt(sum / input.length)
      const isVoice = rms > ENERGY_THRESHOLD

      tickRef.current += chunkMs
      const tick = tickRef.current
      if (isVoice) lastVoiceMsRef.current = tick

      const status = useVoiceStore.getState().status

      if (status === 'speaking') {
        // Barge-in watch: sustained voice while Vivianne is talking cuts her
        // off and starts capturing immediately.
        if (rms > BARGE_IN_THRESHOLD) {
          if (bargeVoiceStartRef.current === null) bargeVoiceStartRef.current = tick
          else if (tick - bargeVoiceStartRef.current > BARGE_IN_SUSTAIN_MS) {
            bargeVoiceStartRef.current = null
            handlersRef.current.beginCapture()
          }
        } else {
          bargeVoiceStartRef.current = null
        }
        return
      }

      if (capturingRef.current) {
        bufRef.current.push(new Float32Array(input))
        const elapsed = tick - activeStartRef.current
        const silence = tick - lastVoiceMsRef.current
        const heardSomething = lastVoiceMsRef.current > activeStartRef.current
        if ((heardSomething && silence > SILENCE_TAIL_MS && elapsed > MIN_UTTERANCE_MS) || elapsed > MAX_UTTERANCE_MS) {
          void handlersRef.current.finalizeUtterance()
        }
      } else if (status === 'listening' && isVoice) {
        handlersRef.current.beginCapture()
        bufRef.current.push(new Float32Array(input))
      }
    }

    audio.source.connect(proc)
    const silentGain = audio.ctx.createGain()
    silentGain.gain.value = 0
    proc.connect(silentGain)
    silentGain.connect(audio.ctx.destination)
    runningRef.current = true

    const greeting = GREETINGS[Math.floor(Math.random() * GREETINGS.length)]
    setStatus('speaking')
    const controller = new AbortController()
    ttsAbortRef.current = controller
    await speak(greeting, {
      engine: settings.ttsEngine,
      cartesiaApiKey: settings.cartesiaApiKey,
      cartesiaVoiceId: settings.cartesiaVoiceId,
      kokoroVoice: settings.kokoroVoice,
      signal: controller.signal,
    })
    if (sessionOpenRef.current) setStatus('listening')
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings.whisperModel, settings.ttsEngine, settings.cartesiaApiKey, settings.cartesiaVoiceId, settings.kokoroVoice])

  const closeSession = useCallback(() => {
    sessionOpenRef.current = false
    runningRef.current = false
    capturingRef.current = false
    ttsAbortRef.current?.abort()
    try {
      processorRef.current?.disconnect()
    } catch {
      // already disconnected
    }
    audioRef.current?.stream.getTracks().forEach((t) => t.stop())
    void audioRef.current?.ctx.close().catch(() => {})
    audioRef.current = null
    processorRef.current = null
    bufRef.current = []
    spokenPrefixRef.current = ''
    setStatus('idle')
    setTranscript('')
    setLastReply('')
  }, [setStatus, setTranscript, setLastReply])

  useEffect(() => closeSession, [closeSession])

  // ── Speak the assistant's reply as it streams, sentence by sentence ──────
  const liveMessage = (() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      const m = messages[i]
      if (m.type === 'assistant') {
        const text = contentToText(m.content).trim()
        const streaming = streamingIndex === i
        if (text || streaming) return { text, streaming }
      }
    }
    return null
  })()

  useEffect(() => {
    if (!sessionOpenRef.current) return
    if (useVoiceStore.getState().status !== 'responding' && useVoiceStore.getState().status !== 'speaking') return
    if (!liveMessage) return

    const text = liveMessage.text
    if (!text.startsWith(spokenPrefixRef.current)) spokenPrefixRef.current = ''
    const unspoken = text.slice(spokenPrefixRef.current.length)
    if (!unspoken) return

    if (!liveMessage.streaming) {
      spokenPrefixRef.current = text
      setLastReply(text.slice(0, 800))
      enqueueSpeech(unspoken.slice(0, 800))
      return
    }
    const match = /^([\s\S]*[.!?])(?:\s+|$)/.exec(unspoken)
    if (match) {
      spokenPrefixRef.current += match[0]
      enqueueSpeech(match[1].slice(0, 800))
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [liveMessage?.text, liveMessage?.streaming, enqueueSpeech, setLastReply])

  // Once the agent finishes and nothing is left to speak, drop back to
  // listening — matches the old code's "linger, then return to passive".
  useEffect(() => {
    if (!sessionOpenRef.current) return
    if (isBusy) return
    if (useVoiceStore.getState().status !== 'responding') return
    const t = window.setTimeout(() => {
      if (sessionOpenRef.current && useVoiceStore.getState().status === 'responding') setStatus('listening')
    }, 500)
    return () => window.clearTimeout(t)
  }, [isBusy, setStatus])

  return { openSession, closeSession, isSessionOpen: () => sessionOpenRef.current }
}
