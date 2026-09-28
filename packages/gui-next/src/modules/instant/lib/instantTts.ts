/**
 * Speaking Viv Instant's replies.
 *
 * Uses the browser's built-in `SpeechSynthesis` rather than the local Kokoro
 * model the old voice mode preferred. Viv Instant's utterances are one or two
 * short sentences ("Opening Chrome", "Done — three tabs closed"), where
 * SpeechSynthesis's instant start beats Kokoro's better voice but ~1s of model
 * latency. Kokoro remains a dependency and is a reasonable option to add behind
 * a setting; it is not the right default here.
 */

/** Cancel any in-flight speech. Barge-in: if the user starts talking again,
 * whatever is being said stops immediately rather than talking over them. */
export function stopSpeaking(): void {
  if (typeof window === 'undefined' || !window.speechSynthesis) return
  window.speechSynthesis.cancel()
}

/**
 * Speak `text`, resolving when it finishes (or is cancelled).
 *
 * Resolves rather than rejects on error: speech is a courtesy on top of the
 * actual work, and a voice that failed to play should never fail the run that
 * produced it.
 */
export function speak(text: string, opts: { signal?: AbortSignal } = {}): Promise<void> {
  const clean = text.trim()
  if (!clean || typeof window === 'undefined' || !window.speechSynthesis) {
    return Promise.resolve()
  }
  if (opts.signal?.aborted) return Promise.resolve()

  return new Promise((resolve) => {
    const utterance = new SpeechSynthesisUtterance(clean)
    utterance.rate = 1.05

    let settled = false
    const done = () => {
      if (settled) return
      settled = true
      opts.signal?.removeEventListener('abort', onAbort)
      resolve()
    }
    function onAbort() {
      window.speechSynthesis.cancel()
      done()
    }

    utterance.onend = done
    // `onerror` fires for a plain `cancel()` too, so it resolves like any
    // other end rather than being treated as a failure.
    utterance.onerror = done
    opts.signal?.addEventListener('abort', onAbort, { once: true })

    // Chromium keeps a queue; anything still pending is stale by definition
    // since Viv Instant only ever has one thing to say at a time.
    window.speechSynthesis.cancel()
    window.speechSynthesis.speak(utterance)
  })
}
