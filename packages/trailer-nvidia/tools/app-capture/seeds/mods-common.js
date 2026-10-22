//@include trailer-common.js
// It builds its own interface: Vivianne writes a mod (an Agent board panel)
// from the presets the app itself ships, and the app runs it.
const PR = await mod('/src/modules/mods/presets/index.ts')
const preset = (id) => PR.PRESETS.find((p) => p.id === id)
const MODS = [
  ['agent-board', 'dashboard', 'Agent board', {}],
  ['checklist', 'checklist', 'Checklist', { fence: 'checklist' }],
]
window.__MODFILES = {}
const files = {}
for (const [key, pid, name, extra] of MODS) {
  files[key] = preset(pid).files({ id: key, name, ...extra })
  for (const f of files[key]) window.__MODFILES[key + '/' + f.path] = f.content
}
const manifestOf = (key) => {
  const m = JSON.parse(files[key].find((f) => f.path === 'mod.json').content)
  return { ...m, contributes: { panels: [], sidebar: [], chat: [], tools: [], themes: [], ...m.contributes } }
}
// Each permission with the words Settings shows for it (mods::manifest).
const WORDS = { 'agent.status': 'See what your agents are doing' }
const modInfo = (key, status) => ({ key, scope: 'user', dir: 'C:/Users/you/.viv/mods/' + key, status, manifest: manifestOf(key), error: null, permissions: manifestOf(key).permissions.map((p) => [p, WORDS[p] || p]) })
// The board's mod.json and page, as Vivianne writes them.
const boardJson = files['agent-board'].find((f) => f.path === 'mod.json').content
const boardHtml = files['agent-board'].find((f) => f.path === 'panel.html').content
const T1 = 60000
const H0 = Date.UTC(2026, 9, 22, 13, 0)
const AGENTS = [
  { id: 'a1', kind: 'specialist', name: 'Lead', project: 'Vivianne', task: 'Fix the failing build', activity: 'running cargo test', state: 'working', startedAt: H0, updatedAt: H0, thread: null },
  { id: 'a2', kind: 'specialist', name: 'Reviewer', project: 'Storefront', task: 'Review #88', activity: 'reading the refunds diff', state: 'working', startedAt: H0, updatedAt: H0, thread: null },
  { id: 'a3', kind: 'chat', name: 'Vivianne', project: 'Orders API', task: 'Plan the database migration', activity: 'writing the backfill', state: 'working', startedAt: H0, updatedAt: H0, thread: 't1' },
]
window.__STUB = {
  ...(window.__STUB || {}),
  cmd_notch_status: AGENTS,
  cmd_mods_fingerprint: {},
}
const build = [
  user('Build me a panel that shows every agent at work', T1),
  ...tools([
    ['guide', { topic: 'mods/index' }, 'Mods: mod.json, pages load /_sdk/v1.js, ask for the fewest permissions.'],
  ], T1 + 4000),
  ...tools([
    ['write', { path: 'C:/Users/you/.viv/mods/agent-board/mod.json', content: boardJson }, 'Wrote mod.json'],
    ['write', { path: 'C:/Users/you/.viv/mods/agent-board/panel.html', content: boardHtml }, 'Wrote panel.html'],
  ], T1 + 9000),
  say('The **Agent board** is built: a panel with a card for every agent at work, updated live. It asks for one permission, `agent.status`, to see what agents are doing. Turn it on in Settings → Mods.', T1 + 16000),
]
