import { useCallback, useEffect, useRef, useState } from 'react'
import { Box, ButtonBase } from '../../../ui'
import { call, on } from '../../../ipc'
import { useTransparentWindowBody } from '../../../lib/transparentWindow'
import { useInstantStore, type InstantStep } from '../store/instantStore'
import { useInstantMic } from '../lib/useInstantMic'
import { speak, stopSpeaking } from '../lib/instantTts'
import { Notch, NOTCH_MUTED, type NotchTone } from './Notch'
import { RestingNotch } from './RestingNotch'
import type { RobotHeadState } from 'robot-heads'

/** Steps that look things up: the face searches. */
const SEARCHING = /^(web_|browser_|read|grep|glob|ls|recall|search|find|list|inspect|screenshot|ocr)/

/** Mirrors `instant::route::Route`. */
interface Route {
  kind: 'computer' | 'general' | 'project' | 'status'
  project: { id: string; name: string } | null
  work: boolean
}

/**
 * What counts as "stop" during a destructive step's countdown.
 *
 * Kept broad and anchored loosely rather than to whole-string equality, because
 * whisper returns punctuation and filler ("Stop!", "no, stop") and three
 * seconds is not long enough to say it twice. Over-matching here costs a
 * skipped destructive step, which is the direction the error should fall.
 */
const CANCEL_PHRASE = /\b(stop|cancel|no|wait|dont|don't|abort)\b/i

/** Matches `instant::window::COLLAPSED_W`. */
const COLLAPSED_W = 200

/** What the status line says for each phase. */
function statusFor(phase: string, hasSteps: boolean, route: Route | null): string {
  if (phase === 'working' && route) {
    if (route.kind === 'general') return 'Answering'
    if (route.kind === 'status') return 'Checking'
    if (route.kind === 'project' && route.project) return route.work ? `To ${route.project.name}` : route.project.name
  }
  switch (phase) {
    case 'listening':
      return 'Listening'
    case 'transcribing':
      return 'One moment'
    case 'working':
      return hasSteps ? 'Working' : 'Thinking'
    case 'done':
      return 'Done'
    case 'error':
      return 'Failed'
    default:
      return 'Viv'
  }
}

/**
 * The Viv Instant notch — the entire UI of the `viv_instant` window.
 *
 * Hangs from the top-centre of the screen as a small pill and grows downward as
 * it has something to show. Two constraints shape it:
 *
 * - **It never holds keyboard focus.** The Rust side never calls `set_focus`,
 *   because every keystroke Viv Instant sends is meant for whatever application
 *   the user was already in — a focused overlay would swallow exactly the input
 *   it is trying to deliver. That is also why Escape is a global shortcut on
 *   the Rust side rather than a key handler here.
 * - **The window is resized to the content whenever it changes.** The window is
 *   transparent but not click-through, so any pixel it occupies swallows mouse
 *   events from the app underneath. Slack space would punch an invisible hole
 *   in the user's screen — and at the top of the display, that means other
 *   apps' title bars and tab strips.
 */
/** The reply, shown word by word as it is spoken, scrolled to the newest
 * words, never cut off. */
function RevealedText({ text, revealed, error }: { text: string; revealed: number | null; error: boolean }) {
  const ref = useRef<HTMLDivElement | null>(null)
  const words = text.split(/(\s+)/)
  let shown = text
  if (revealed !== null) {
    let seen = 0
    let out = ''
    for (const w of words) {
      if (/\S/.test(w)) {
        if (seen >= revealed) break
        seen++
      }
      out += w
    }
    shown = out
  }
  useEffect(() => {
    const el = ref.current
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' })
  }, [shown])
  return (
    <Box
      ref={ref}
      sx={{
        color: error ? '#ff9b9b' : 'inherit',
        maxHeight: 132,
        overflowY: 'auto',
        scrollbarWidth: 'none',
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
        '&::-webkit-scrollbar': { display: 'none' },
      }}
    >
      {shown}
    </Box>
  )
}

export function InstantOverlay() {
  // Without this the window paints an opaque rectangle over the screen:
  // CssBaseline gives every window a solid body background, which is right for
  // the main window and wrong for a transparent overlay.
  useTransparentWindowBody()

  const phase = useInstantStore((s) => s.phase)
  const transcript = useInstantStore((s) => s.transcript)
  const interim = useInstantStore((s) => s.interim)
  const steps = useInstantStore((s) => s.steps)
  const summary = useInstantStore((s) => s.summary)
  const error = useInstantStore((s) => s.error)
  const risk = useInstantStore((s) => s.risk)
  const download = useInstantStore((s) => s.download)
  const store = useInstantStore
  // What the request turned out to be (`instant::route`), for the title.
  const [route, setRoute] = useState<Route | null>(null)

  // One controller per run, so a new activation cuts off the previous reply
  // mid-sentence rather than letting it talk over the new one.
  const speechRef = useRef<AbortController | null>(null)

  // True while Viv Instant is talking out loud.
  //
  // The microphone hears the speakers, so without this the overlay transcribes
  // its own voice and acts on it — and during a destructive step's countdown
  // that means cancelling itself. Barge-in is what makes a plain flag
  // sufficient: `onSpeechStart` aborts the utterance the moment the user
  // actually speaks, which clears this before their words are transcribed.
  const talkingRef = useRef(false)
  // The window an "ask about this window" question is about, until it's sent.
  const assistRef = useRef<string | null>(null)
  // The face's "speaking" state; the ref above is what the logic reads.
  const [speaking, setSpeaking] = useState(false)
  // How many words of the reply have been spoken: the text appears in step
  // with the voice. `null` once nothing is being revealed (show it all).
  const [revealed, setRevealed] = useState<number | null>(null)
  // When the last reply finished being said, for how long it stays up.
  const spokeWordsRef = useRef(0)

  const say = useCallback((text: string, signal?: AbortSignal, reveal = false) => {
    talkingRef.current = true
    setSpeaking(true)
    spokeWordsRef.current = text.trim().split(/\s+/).length
    setRevealed(reveal ? 0 : null)
    return speak(text, { signal, onProgress: reveal ? (n) => setRevealed(n) : undefined }).finally(() => {
      talkingRef.current = false
      setSpeaking(false)
      setRevealed(null)
    })
  }, [])

  // Re-read on every activation rather than once at mount: the overlay window
  // is created at app start and then lives for the whole session, so a model
  // changed in Settings would otherwise not take effect until a restart.
  const [sttModel, setSttModel] = useState<string | undefined>(undefined)

  const onUtterance = useCallback(
    (text: string) => {
      // Anything captured while the overlay itself is talking is its own voice
      // coming back through the microphone, not an instruction.
      if (talkingRef.current) return

      const s = store.getState()
      const assist = assistRef.current
      // Held and released without a word (or a tap too short to hear).
      if (!text && !assist) {
        if (s.phase === 'listening' || s.phase === 'transcribing') s.fail("Didn't catch that — hold the keys while you speak.")
        return
      }
      if (!text && s.phase !== 'listening' && s.phase !== 'transcribing') return

      // During a destructive step's countdown, speech is the affordance that
      // actually fits: the premise is that the user is looking at the app being
      // driven, not at this notch, so requiring them to find and click a button
      // in three seconds would make Cancel decorative.
      if (s.risk && CANCEL_PHRASE.test(text)) {
        void call('cmd_instant_cancel_step', {})
        return
      }

      // Otherwise ignore speech that arrives while a run is already going.
      // Barge-in that re-plans mid-execution is a much bigger design than this,
      // and silently starting a second run would be worse than ignoring it.
      if (s.phase === 'working') return
      // Said nothing about it: ask the obvious thing.
      const said = text || 'What is this, and is anything wrong or worth fixing here?'
      s.setTranscript(said)
      s.setPhase('working')
      assistRef.current = null
      const instruction = assist
        ? `The user is looking at the window "${assist}". First take a screenshot of that window (the screenshot tool with window: true), then answer: ${said}`
        : said
      void call('cmd_instant_run', { instruction }).catch((e) => {
        store.getState().fail(e instanceof Error ? e.message : String(e))
      })
    },
    [store],
  )

  // The microphone's level, for the wave. Throttled: frames arrive ~30 times
  // a second and the bars don't need more.
  const [level, setLevel] = useState(0)
  const levelAt = useRef(0)
  const { start, stop, release } = useInstantMic({
    onLevel: (l) => {
      const now = performance.now()
      if (now - levelAt.current < 60) return
      levelAt.current = now
      setLevel(l)
    },
    onUtterance,
    onSpeechStart: () => {
      // The microphone hears the speakers: while talking, "speech" is almost
      // always its own voice, and cutting itself off was what truncated long
      // replies. Holding the chord again (`instant:activate`) still stops it.
      if (talkingRef.current) return
      speechRef.current?.abort()
      stopSpeaking()
      talkingRef.current = false
    },
    onInterim: (text) => {
      if (talkingRef.current) return
      const s = store.getState()
      if (s.phase === 'listening') s.setInterim(text)
    },
    onError: (message) => store.getState().fail(message),
    model: sttModel,
  })

  useEffect(() => {
    const offs: Array<() => void> = []
    const subscribed: Array<Promise<void>> = []
    const track = (p: Promise<() => void>) => subscribed.push(p.then((off) => void offs.push(off)))

    track(on<{ hold?: boolean; assist?: string | null } | null>('instant:activate', (payload) => {
      // "Ask about this window": the next thing said is about that window.
      assistRef.current = payload?.assist ?? null
      speechRef.current?.abort()
      stopSpeaking()
      talkingRef.current = false
      store.getState().activate()
      setRoute(null)
      void (async () => {
        const cfg = await call<{ instantSttModel?: string }>('cmd_get_settings').catch(() => null)
        const chosen = cfg?.instantSttModel
        setSttModel(chosen)
        // Preloaded here rather than at app start: a user who never presses the
        // chord should never pay the model download or its ~190MB of buffers.
        await call('cmd_instant_stt_preload', { model: chosen }).catch(() => {})
      })()
      void start({ hold: !!payload?.hold })
    }))

    // Push-to-talk: the chord came up — what was said is transcribed and run.
    track(on('instant:release', () => {
      if (store.getState().phase === 'listening') store.getState().setPhase('transcribing')
      release()
    }))

    track(on('instant:dismiss', () => {
      speechRef.current?.abort()
      stopSpeaking()
      talkingRef.current = false
      stop()
      store.getState().dismiss()
      setRoute(null)
      // Not hidden here: the backend either hides the window or, when the
      // notch is always on, puts it back to resting.
    }))

    track(on<Route>('instant:route', setRoute))

    track(on<InstantStep>('instant:step', (step) => {
      store.getState().upsertStep(step)
    }))

    track(on<{ summary: string; ms: number }>('instant:risky', ({ summary, ms }) => {
      // The deadline is derived here rather than sent, because the two sides
      // have different clocks — a timestamp from Rust would be compared against
      // the webview's `Date.now()` and could start the bar already expired.
      store.getState().setRisk({ summary, ms, deadline: Date.now() + ms })
      // Deliberately contains no cancel word: this sentence is played into a
      // room with a live microphone, and "say stop to cancel" transcribed back
      // would cancel the very step it is announcing. The bar on screen carries
      // the wording; the voice carries the warning.
      void say(`Careful — about to ${summary}.`)
    }))

    track(on<{ received: number; total: number; index: number; count: number }>(
      'instant:stt_download',
      ({ received, total, index, count }) => {
        store.getState().setDownload({
          percent: total > 0 ? Math.round((received / total) * 100) : 0,
          index,
          count,
        })
      },
    ))

    track(on('instant:stt_download_done', () => {
      store.getState().setDownload(null)
    }))

    track(on('instant:risky_done', () => {
      store.getState().setRisk(null)
    }))

    track(on<string>('instant:done', (text) => {
      store.getState().finish(text)
      const controller = new AbortController()
      speechRef.current = controller
      void say(text, controller.signal, true)
    }))

    track(on<string>('instant:error', (message) => {
      store.getState().fail(message)
    }))

    track(on('instant:aborted', () => {
      speechRef.current?.abort()
      stopSpeaking()
      talkingRef.current = false
      store.getState().setPhase('done')
    }))

    // Every listener attached: the backend now delivers anything it queued
    // while this window was still loading — on a first summon, the
    // `instant:activate` that opened it (see `overlays.rs`).
    void Promise.all(subscribed).then(() => call('cmd_overlay_ready'))

    return () => {
      offs.forEach((off) => off())
      stop()
      stopSpeaking()
      talkingRef.current = false
    }
  }, [say, start, stop, release, store])

  // Once it has answered (and finished saying so), the notch goes by itself.
  useEffect(() => {
    if (phase !== 'done' && phase !== 'error') return
    let timer = 0
    const check = () => {
      if (talkingRef.current) {
        timer = window.setTimeout(check, 500)
        return
      }
      // Long answers stay up long enough to be read: ~3 words a second on
      // top of a base, counted from when the voice stopped.
      const reading = Math.min(20000, spokeWordsRef.current * 300)
      timer = window.setTimeout(() => {
        const now = store.getState().phase
        if ((now === 'done' || now === 'error') && !talkingRef.current) void call('cmd_instant_dismiss').catch(() => {})
      }, (phase === 'error' ? 6000 : 4000) + reading)
    }
    check()
    return () => window.clearTimeout(timer)
  }, [phase, store])

  const working = phase === 'working'
  const heard = interim || transcript
  const current = [...steps].reverse().find((st) => st.status === 'running') ?? steps[steps.length - 1]

  // The risk countdown, from its absolute deadline.
  const [now, setNow] = useState(Date.now())
  useEffect(() => {
    if (!risk) return
    const t = window.setInterval(() => setNow(Date.now()), 100)
    return () => window.clearInterval(t)
  }, [risk])

  const tone: NotchTone =
    phase === 'listening' ? 'listening' : phase === 'error' ? 'error' : phase === 'done' ? 'done' : working || phase === 'transcribing' ? 'working' : 'idle'

  // The robot: attentive while you talk, thinking until it picks a step,
  // searching while it looks things up, talking while it speaks.
  const running = steps.find((st) => st.status === 'running')
  const face: RobotHeadState = speaking
    ? 'speaking'
    : phase === 'listening'
      ? 'listening'
      : phase === 'transcribing'
        ? 'thinking'
        : phase === 'error'
          ? 'error'
          : phase === 'done'
            ? 'happy'
            : working
              ? !running
                ? 'thinking'
                : SEARCHING.test(running.tool)
                  ? 'searching'
                  : 'working'
              : 'idle'

  // One line: what it heard while listening, the step it's on while working.
  const detail = download
    ? `Downloading speech model ${download.index}/${download.count} · ${download.percent}%`
    : phase === 'working' && current
      ? current.summary
      : heard || (phase === 'listening' ? 'Speak while holding, release to send' : phase === 'transcribing' ? 'Getting that…' : null)

  // A second line only for what must be read: the answer, an error, or a
  // risky step about to run.
  const body = risk ? (
    <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
      <Box component="span" sx={{ flex: 1, minWidth: 0 }}>
        <Box component="span" sx={{ color: '#fbbf24', fontWeight: 600 }}>
          About to {risk.summary}
        </Box>
        <Box component="span" sx={{ color: NOTCH_MUTED }}> · say “stop” or</Box>
      </Box>
      <ButtonBase
        onClick={() => void call('cmd_instant_cancel_step', {})}
        sx={{ height: 22, px: 1, borderRadius: '999px', fontSize: 11, fontWeight: 600, color: '#0b0b0c', bgcolor: '#fbbf24', flexShrink: 0 }}
      >
        Cancel
      </ButtonBase>
    </Box>
  ) : error || summary ? (
    <RevealedText text={error ?? summary ?? ''} revealed={error ? null : revealed} error={!!error} />
  ) : null

  // Between commands: the always-on status notch.
  if (phase === 'idle') return <RestingNotch />

  return (
    <Notch
      tone={tone}
      face={face}
      title={statusFor(phase, steps.length > 0, route)}
      detail={detail}
      level={level}
      onStop={working ? () => void call('cmd_instant_abort', {}) : null}
      body={body}
      progress={risk ? Math.max(0, risk.deadline - now) / Math.max(1, risk.ms) : download ? download.percent / 100 : null}
      resizeCommand="cmd_instant_set_size"
      minWidth={COLLAPSED_W}
    />
  )
}
