/**
 * Self-check for `IdleWatchdog` — run with `node src/modules/ai/providers/shared.check.ts`
 * from `packages/gui-next` (Node 22+ strips the types).
 *
 * Exists because this class was silently dead for a long time: it rejected a
 * `promise` field nobody awaited, so a hung provider was never cancelled and
 * every stall surfaced as Rust's generic 120s backstop error instead. The
 * contract that matters is "it aborts its signal" — assert exactly that.
 */
import { IdleWatchdog } from './shared.ts'

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
const assert = (cond: boolean, msg: string) => {
  if (!cond) throw new Error(`FAILED: ${msg}`)
}

async function main() {
  // Fires when nothing pokes it, and aborts the signal callers pass to fetch.
  const fires = new IdleWatchdog(50)
  assert(fires.signal.aborted === false, 'must not abort before the timeout')
  await sleep(90)
  assert(fires.timedOut === true, 'timedOut must be set once it fires')
  assert(fires.signal.aborted === true, 'must abort the signal — this is what cancels the request')
  fires.stop()

  // poke() re-arms: steady streaming must never trip it.
  const poked = new IdleWatchdog(60)
  for (let i = 0; i < 4; i++) {
    await sleep(30)
    poked.poke()
  }
  assert(poked.signal.aborted === false, 'poke() must re-arm — a streaming response must not be killed')
  poked.stop()

  // stop() disarms for good (normal end of stream).
  const stopped = new IdleWatchdog(40)
  stopped.stop()
  await sleep(80)
  assert(stopped.signal.aborted === false, 'stop() must prevent a late abort')

  // `expired` unblocks a caller whose stream never throws on abort — the
  // real failure seen against OpenRouter, where aborting the signal alone
  // left `for await` hanging until Rust's 120s backstop.
  const hung = new IdleWatchdog(50)
  const neverSettles = new Promise<string>(() => {})
  const raced = await Promise.race([neverSettles, hung.expired]).then(
    () => 'resolved',
    (e: Error) => e.message,
  )
  assert(raced.includes('stalled'), `expired must reject a hung stream, got: ${raced}`)
  hung.stop()

  console.log('IdleWatchdog: all checks passed')
}

void main()
