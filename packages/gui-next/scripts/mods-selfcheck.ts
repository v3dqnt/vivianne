/**
 * Checks the mod bridge's rules without the app: run with
 *   npx tsx scripts/mods-selfcheck.ts
 */
import { ModConnection, workspacePath, RATE_LIMIT, type HostApi } from '../src/modules/mods/host/bridge'
import type { ModInfo } from '../src/modules/mods/lib/types'

let passed = 0
let failed = 0
async function check(name: string, fn: () => Promise<void> | void) {
  try {
    await fn()
    passed++
    console.log(`ok   ${name}`)
  } catch (e) {
    failed++
    console.log(`FAIL ${name}: ${e instanceof Error ? e.message : e}`)
  }
}
function assert(cond: unknown, msg: string) {
  if (!cond) throw new Error(msg)
}
async function rejects(p: Promise<unknown>, includes: string) {
  try {
    await p
  } catch (e) {
    const m = e instanceof Error ? e.message : String(e)
    assert(m.includes(includes), `expected an error with "${includes}", got "${m}"`)
    return
  }
  throw new Error('expected the call to be refused')
}

const sent: string[] = []
const host: HostApi = {
  storageGet: async () => ({ n: 1 }),
  storageSet: async () => {},
  notify: () => {},
  messages: () => [
    { type: 'user', content: [{ type: 'text', text: 'hi' }], timestamp: 1 },
    { type: 'assistant', content: [{ type: 'text', text: 'hello' }], stopReason: 'stop', errorMessage: null, timestamp: 2 },
  ],
  compose: () => {},
  send: async (t) => {
    sent.push(t)
  },
  agentStatus: async () => [],
  subscribeStatus: () => () => {},
  workspace: () => 'C:\\work\\proj',
  readFile: async (p) => `read ${p}`,
  listDir: async () => [],
  writeFile: async () => {},
  openUrl: async () => {},
  openPanel: () => {},
}

function mod(permissions: string[], status: ModInfo['status'] = 'on'): ModInfo {
  return {
    key: 'u.test',
    scope: 'user',
    dir: '',
    status,
    error: null,
    permissions: [],
    manifest: {
      apiVersion: 1,
      id: 'test',
      name: 'Test',
      version: '1',
      description: '',
      permissions,
      contributes: { panels: [{ id: 'main', title: 'Main', entry: 'p.html' }], sidebar: [], chat: [], tools: [], themes: [] },
    },
  }
}

await check('a call without its permission is refused', async () => {
  const c = new ModConnection(mod([]), 'panel', host, () => {})
  await rejects(c.handle('chat.send', { text: 'x' }), 'chat.send')
  assert(sent.length === 0, 'nothing was sent')
  await rejects(c.handle('files.read', { path: 'a.txt' }), 'files.read')
})

await check('a granted call goes through', async () => {
  const c = new ModConnection(mod(['chat.read', 'chat.send']), 'panel', host, () => {})
  const msgs = (await c.handle('chat.read', null)) as { role: string; text: string }[]
  assert(msgs.length === 2 && msgs[1].text === 'hello', 'reads the conversation as text')
  await c.handle('chat.send', { text: 'go' })
  assert(sent[0] === 'go', 'sent')
})

await check('storage needs no permission', async () => {
  const c = new ModConnection(mod([]), 'sidebar', host, () => {})
  const v = (await c.handle('storage.get', null)) as { n: number }
  assert(v.n === 1, 'got storage')
})

await check('a mod that is off can call nothing', async () => {
  const c = new ModConnection(mod(['notify'], 'off'), 'panel', host, () => {})
  await rejects(c.handle('notify', { text: 'x' }), 'off')
})

await check('unknown methods are refused', async () => {
  const c = new ModConnection(mod(['notify']), 'panel', host, () => {})
  await rejects(c.handle('app.secrets', null), 'unknown method')
})

await check('calls past the rate limit are refused', async () => {
  const c = new ModConnection(mod([]), 'panel', host, () => {})
  for (let i = 0; i < RATE_LIMIT; i++) await c.handle('storage.get', null)
  await rejects(c.handle('storage.get', null), 'too many')
})

await check('file paths stay inside the workspace', async () => {
  assert(workspacePath('C:\\work\\proj', 'src/a.ts') === 'C:\\work\\proj\\src\\a.ts', 'joins with the root separator')
  assert(workspacePath('/home/u/p', './a/b.md') === '/home/u/p/a/b.md', 'posix root')
  for (const bad of ['../secret', 'a/../../x', '/etc/passwd', 'C:\\Windows\\x', '\\\\server\\share']) {
    let threw = false
    try {
      workspacePath('C:\\work\\proj', bad)
    } catch {
      threw = true
    }
    assert(threw, `refuses ${bad}`)
  }
  const c = new ModConnection(mod(['files.read']), 'panel', host, () => {})
  await rejects(c.handle('files.read', { path: '../x' }), 'inside the workspace')
})

await check('a mod can only open its own panels', async () => {
  const c = new ModConnection(mod(['open']), 'panel', host, () => {})
  await c.handle('open.panel', { id: 'main' })
  await rejects(c.handle('open.panel', { id: 'other' }), 'no panel')
})

console.log(`\n${passed} passed, ${failed} failed`)
process.exit(failed ? 1 : 0)
