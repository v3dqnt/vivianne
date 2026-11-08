/**
 * Speaking Viv Instant's replies.
 *
 * Uses the browser's built-in `SpeechSynthesis`, whose quirks shape this:
 *
 * - Long utterances get cut off, so a reply is spoken as sentence-sized pieces.
 * - The pieces are handed over one at a time, each when the previous ends,
 *   rather than queued up front: a long queue is where Chromium stalls, and
 *   one at a time means a piece the engine drops can be noticed and said
 *   again instead of silently taking the rest of the reply with it.
 * - Chromium stops firing events for an utterance that has been garbage
 *   collected, so the piece being said is always held.
 * - No pause()/resume() "keepalive": it worked around a timeout in desktop
 *   Chrome's online voices, and on WebView2's Windows voices it is what cut
 *   replies off mid-sentence.
 *
 * Progress is reported as the number of words spoken so far, so the overlay
 * can reveal the text in step with the voice: from `boundary` events where the
 * voice emits them, otherwise from an estimate of the speaking rate.
 */

/** Bumped whenever speech is stopped or superseded; anything older is stale. */
let generation = 0

export function stopSpeaking(): void {
  generation++
  if (typeof window === 'undefined' || !window.speechSynthesis) return
  window.speechSynthesis.cancel()
}

export interface SpeakOptions {
  signal?: AbortSignal
  /** Called with how many words of `text` have been spoken (monotonic). */
  onProgress?: (wordsSpoken: number, totalWords: number) => void
}

/** Sentence-sized pieces, each short enough that no voice cuts it off. */
function chunks(text: string): string[] {
  const sentences = text.match(/[^.!?\n]+[.!?]*[\s]*|\n+/g) ?? [text]
  const out: string[] = []
  for (const raw of sentences) {
    let s = raw.trim()
    while (s.length > 220) {
      const cut = s.lastIndexOf(',', 220) > 80 ? s.lastIndexOf(',', 220) + 1 : s.lastIndexOf(' ', 220)
      const at = cut > 0 ? cut : 220
      out.push(s.slice(0, at).trim())
      s = s.slice(at).trim()
    }
    if (s) out.push(s)
  }
  return out
}

const wordCount = (s: string) => (s.trim() ? s.trim().split(/\s+/).length : 0)

/** How often the engine is checked on (also the reveal's step when the voice
 * sends no boundary events), and how long it may go quiet mid-piece before
 * the piece counts as dropped. */
const WATCH_MS = 150
const QUIET_MS = 1500

/**
 * Speak `text`, resolving once all of it has been said (or it is cancelled).
 * Resolves rather than rejects: a voice that failed must never fail the run.
 */
export function speak(text: string, opts: SpeakOptions = {}): Promise<void> {
  const clean = text.trim()
  if (!clean || typeof window === 'undefined' || !window.speechSynthesis) {
    opts.onProgress?.(wordCount(clean), wordCount(clean))
    return Promise.resolve()
  }
  if (opts.signal?.aborted) return Promise.resolve()

  const pieces = chunks(clean)
  const total = wordCount(clean)
  const synth = window.speechSynthesis
  // Whatever was being said is superseded.
  stopSpeaking()
  const gen = generation

  return new Promise((resolve) => {
    let settled = false
    let spoken = 0
    let index = 0
    // Held so its events keep firing (see above); null between pieces.
    let current: SpeechSynthesisUtterance | null = null
    // Each piece gets one second chance, whether it was dropped or interrupted.
    let retried = false
    let lastSign = performance.now()
    let began = 0
    let sawBoundary = false

    const report = (n: number) => {
      const next = Math.min(total, Math.max(spoken, n))
      if (next !== spoken) {
        spoken = next
        opts.onProgress?.(spoken, total)
      }
    }
    const stale = () => gen !== generation || !!opts.signal?.aborted
    const finish = (complete: boolean) => {
      if (settled) return
      settled = true
      current = null
      clearInterval(watchdog)
      opts.signal?.removeEventListener('abort', onAbort)
      if (complete) report(total)
      resolve()
    }
    function onAbort() {
      if (gen === generation) stopSpeaking()
      finish(false)
    }
    opts.signal?.addEventListener('abort', onAbort, { once: true })

    const offsets: number[] = []
    pieces.reduce((at, p) => (offsets.push(at), at + wordCount(p)), 0)

    const advance = () => {
      report(offsets[index] + wordCount(pieces[index]))
      index++
      retried = false
      sayCurrent()
    }

    function sayCurrent() {
      if (stale()) return finish(false)
      if (index >= pieces.length) return finish(true)
      const i = index
      const piece = pieces[i]
      const u = new SpeechSynthesisUtterance(piece)
      u.rate = 1.05
      // Events from a piece that has since been replaced (a retry) are ignored.
      const live = () => current === u && index === i && !settled
      u.onstart = () => {
        if (!live()) return
        lastSign = began = performance.now()
        sawBoundary = false
        report(offsets[i])
      }
      u.onboundary = (e) => {
        if (!live()) return
        lastSign = performance.now()
        sawBoundary = true
        report(offsets[i] + wordCount(piece.slice(0, e.charIndex)))
      }
      u.onend = () => {
        if (live()) advance()
      }
      u.onerror = (e) => {
        if (!live()) return
        if (stale()) return finish(false)
        // Interrupted by something other than us (another window took the
        // voice, an engine hiccup): say it once more. A second interruption
        // means the voice is wanted elsewhere.
        if (e.error === 'interrupted' || e.error === 'canceled') {
          if (retried) return finish(false)
          retried = true
          return sayCurrent()
        }
        // Anything else (no voice for this text, synthesis failed): skip the
        // piece rather than leave the rest unsaid.
        advance()
      }
      current = u
      began = 0
      lastSign = performance.now()
      synth.speak(u)
    }

    const watchdog = setInterval(() => {
      if (settled) return
      if (stale()) return finish(false)
      if (synth.paused) synth.resume()
      const now = performance.now()
      // No boundary events (many voices): estimate ~2.7 words/s at rate 1.05.
      if (began && !sawBoundary) {
        const words = wordCount(pieces[index] ?? '')
        report(offsets[index] + Math.min(words - 1, Math.floor(((now - began) / 1000) * 2.7 * 1.05)))
      }
      // The engine went quiet without finishing the piece: it was dropped.
      if (current && !synth.speaking && !synth.pending && now - lastSign > QUIET_MS) {
        if (retried) advance()
        else {
          retried = true
          sayCurrent()
        }
      }
    }, WATCH_MS)

    sayCurrent()
  })
}
