/**
 * Checks that streamed tokens land whole and in order though they are
 * applied once per frame, without the app:
 *   npx tsx scripts/streaming-selfcheck.ts
 */
;(globalThis as { window?: unknown }).window = globalThis
// One "frame" per macrotask, as a browser would paint between them.
;(globalThis as { requestAnimationFrame?: unknown }).requestAnimationFrame = (f: () => void) => setTimeout(f, 0) as unknown as number
;(globalThis as { cancelAnimationFrame?: unknown }).cancelAnimationFrame = (id: number) => clearTimeout(id)
const { mockIPC } = await import('@tauri-apps/api/mocks')
mockIPC(() => null, { shouldMockEvents: true })
const { emit } = await import('@tauri-apps/api/event')
const { agentStoreFor } = await import('../src/modules/ai/store/agentStore')

let failed = 0
function check(name: string, ok: boolean, detail = '') {
  if (!ok) failed++
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ` ${detail}`}`)
}
const tick = () => new Promise((r) => setTimeout(r, 5))

const store = agentStoreFor('s:stream')
await store.getState().init()
const send = (event: unknown) => emit('agent_event', { threadId: 's:stream', event })
const assistant = (content: unknown[]) => ({ type: 'assistant', content, stopReason: 'stop', timestamp: 1 })

await send({ type: 'message_start', payload: { message: assistant([]) } })
let renders = 0
const off = store.subscribe(() => renders++)
for (const t of ['Hel', 'lo, ', 'wor', 'ld']) await send({ type: 'message_update', payload: { message: assistant([{ type: 'text', text: t }]) } })
await send({ type: 'message_update', payload: { message: assistant([{ type: 'thinking', thinking: 'hmm' }]) } })
await tick()
off()
const live = store.getState().messages.at(-1) as { content: Array<{ type: string; text?: string; thinking?: string }> }
check('tokens arrive whole and in order', live.content.find((c) => c.type === 'text')?.text === 'Hello, world', JSON.stringify(live.content))
check('thinking streams beside the text', live.content[0]?.type === 'thinking' && live.content[0]?.thinking === 'hmm', JSON.stringify(live.content))
check('five tokens, fewer renders', renders < 5, `${renders} renders`)

// A token still buffered when the message ends is not lost or reordered.
await send({ type: 'message_update', payload: { message: assistant([{ type: 'text', text: '!' }]) } })
await send({ type: 'message_end', payload: { message: assistant([{ type: 'text', text: 'Hello, world!' }]) } })
await tick()
const done = store.getState().messages.at(-1) as { content: Array<{ text?: string }> }
check('the final message wins at the end', done.content[0]?.text === 'Hello, world!' && store.getState().streamingIndex === null, JSON.stringify(done))
check('one message, not two', store.getState().messages.length === 1, String(store.getState().messages.length))

console.log(failed ? `\n${failed} failed` : '\nall passed')
process.exit(failed ? 1 : 0)
