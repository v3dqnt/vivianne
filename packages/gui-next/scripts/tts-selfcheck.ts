/**
 * Checks Viv Instant's speech queue against a fake speech engine: run with
 *   npx tsx scripts/tts-selfcheck.ts
 */

// A fake engine: one utterance at a time, each "spoken" in 30 ms, with hooks
// to drop or interrupt a chosen piece the way WebView2 sometimes does.
type U = { text: string; onstart?: () => void; onend?: () => void; onerror?: (e: { error: string }) => void; onboundary?: () => void; rate: number }
const said: string[] = []
let queue: U[] = []
let speakingNow: U | null = null
let dropNext = new Set<string>()
/** How many more times each piece is interrupted. */
let interrupts = new Map<string, number>()

function pump() {
  if (speakingNow || !queue.length) return
  const u = queue.shift()!
  speakingNow = u
  if (dropNext.delete(u.text)) {
    // Dropped: no events at all, and the engine goes idle.
    speakingNow = null
    setTimeout(pump, 0)
    return
  }
  setTimeout(() => u.onstart?.(), 0)
  setTimeout(() => {
    // Cancelled while it was being said: cancel() already reported it.
    if (speakingNow !== u) return
    speakingNow = null
    const n = interrupts.get(u.text) ?? 0
    if (n > 0) {
      interrupts.set(u.text, n - 1)
      u.onerror?.({ error: 'interrupted' })
    }
    else {
      said.push(u.text)
      u.onend?.()
    }
    pump()
  }, 30)
}

const synth = {
  get speaking() {
    return !!speakingNow
  },
  get pending() {
    return queue.length > 0
  },
  paused: false,
  speak(u: U) {
    queue.push(u)
    setTimeout(pump, 0)
  },
  cancel() {
    const was = [speakingNow, ...queue].filter(Boolean) as U[]
    queue = []
    speakingNow = null
    for (const u of was) setTimeout(() => u.onerror?.({ error: 'canceled' }), 0)
  },
  pause() {},
  resume() {},
}
const g = globalThis as unknown as Record<string, unknown>
g.window = { speechSynthesis: synth }
g.SpeechSynthesisUtterance = class {
  text: string
  rate = 1
  constructor(t: string) {
    this.text = t
  }
}

const { speak, stopSpeaking } = await import('../src/modules/instant/lib/instantTts')

let failed = 0
function eq(name: string, got: unknown, want: unknown) {
  const ok = JSON.stringify(got) === JSON.stringify(want)
  if (!ok) failed++
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : `: got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`}`)
}
const reset = () => {
  said.length = 0
  dropNext = new Set()
  interrupts = new Map()
}

const text = 'First sentence here. Second one follows. Third ends it.'
const pieces = ['First sentence here.', 'Second one follows.', 'Third ends it.']

{
  reset()
  let progress = 0
  await speak(text, { onProgress: (n) => (progress = n) })
  eq('every sentence is said, in order', said, pieces)
  eq('progress reaches the last word', progress, 9)
}
{
  reset()
  dropNext.add(pieces[1])
  await speak(text)
  eq('a dropped sentence is said again, and the rest follows', said, pieces)
}
{
  reset()
  interrupts.set(pieces[1], 1)
  await speak(text)
  eq('an outside interruption is retried once', said, pieces)
}
{
  reset()
  interrupts.set(pieces[1], 2)
  await speak(text)
  eq('a second interruption gives the voice up', said, [pieces[0]])
}
{
  reset()
  const p = speak(text)
  setTimeout(stopSpeaking, 45)
  await p
  eq('stopping ends it after the sentence being said', said.length < 3, true)
}
{
  reset()
  const first = speak('Old reply goes on and on. Still the old one.')
  await new Promise((r) => setTimeout(r, 10))
  await Promise.all([first, speak('New reply.')])
  eq('a new reply supersedes the old one', said, ['New reply.'])
}
{
  reset()
  const c = new AbortController()
  const p = speak(text, { signal: c.signal })
  setTimeout(() => c.abort(), 45)
  await p
  eq('aborting resolves without saying the rest', said.length < 3, true)
}

console.log(failed ? `\n${failed} failed` : '\nall passed')
process.exit(failed ? 1 : 0)
