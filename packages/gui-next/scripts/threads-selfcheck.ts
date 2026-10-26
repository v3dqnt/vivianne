/**
 * Checks where threads run, without the app: a thread opened while the one on
 * screen is mid-turn gets its own agent thread, and the busy one keeps going.
 *   npx tsx scripts/threads-selfcheck.ts
 */
;(globalThis as { window?: unknown }).window = globalThis
const { mockIPC } = await import('@tauri-apps/api/mocks')

const calls: Array<{ cmd: string; args: Record<string, unknown> }> = []
mockIPC((cmd, args) => {
  calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> })
  if (cmd === 'cmd_get_sessions') return []
  if (cmd === 'cmd_new_session') return 'N'
  if (cmd === 'cmd_agent_messages') return []
  return null
}, { shouldMockEvents: true })

const { agentStoreFor, onTurnEnd } = await import('../src/modules/ai/store/agentStore')
const { activeThreadId } = await import('../src/modules/ai/store/activeThread')
const { useDaysStore, holderOf, GENERAL_SCOPE } = await import('../src/modules/sessions/store/daysStore')

let failed = 0
function check(name: string, ok: boolean, detail = '') {
  if (!ok) failed++
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ` ${detail}`}`)
}
const loadsSince = (n: number) => calls.slice(n).filter((c) => c.cmd === 'cmd_load_session').map((c) => `${c.args.id}@${c.args.threadId}`)

useDaysStore.setState({ scope: GENERAL_SCOPE })
const days = useDaysStore.getState()

let n = calls.length
await days.openThread('A')
check('an idle screen opens a thread on the primary', loadsSince(n).join() === 'A@' && activeThreadId() === '', loadsSince(n).join())

agentStoreFor('').setState({ isBusy: true })
n = calls.length
await days.openThread('B')
check('a busy screen opens the next thread beside it', loadsSince(n).join() === 'B@s:B' && activeThreadId() === 's:B', loadsSince(n).join())
check('the busy thread is still busy', agentStoreFor('').getState().isBusy)

n = calls.length
await days.openThread('A')
check('going back to a running thread reloads nothing', loadsSince(n).length === 0 && activeThreadId() === '', loadsSince(n).join())

const ended: string[] = []
const off = onTurnEnd((id) => ended.push(id))
agentStoreFor('').setState({ isBusy: false })
off()
check('a turn ending on a thread is reported for that thread', ended.join() === '', ended.join())

n = calls.length
await days.openThread('C')
check('an idle thread is reused for the next one', loadsSince(n).join() === 'C@' && holderOf('A') === undefined && holderOf('B') === 's:B', `${loadsSince(n).join()} A=${holderOf('A')}`)

agentStoreFor('').setState({ isBusy: true })
n = calls.length
await days.newThread()
const made = calls.slice(n).find((c) => c.cmd === 'cmd_new_session')
check('a new thread while busy starts beside the busy one', !!made && String(made.args.threadId).startsWith('s:new-') && holderOf('N') === made.args.threadId, JSON.stringify(made?.args))

console.log(failed ? `\n${failed} failed` : '\nall passed')
process.exit(failed ? 1 : 0)
