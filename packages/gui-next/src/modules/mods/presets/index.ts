/**
 * Presets: starter mods for each slot, each a complete, working mod the
 * wizard writes into `~/.viv/mods/<id>/`. They double as examples of the
 * rules — every page loads the SDK, asks only for the permissions it uses,
 * and styles itself with the theme's `--viv-*` variables.
 */
import type { ModSlot } from '../lib/types'

export type PresetSlot = ModSlot | 'tool'

export interface PresetOptions {
  id: string
  name: string
  /** Chat presets: the fence they render. */
  fence?: string
  /** Web page preset: the local URL. */
  url?: string
  /** Tool preset. */
  tool?: { name: string; description: string; command: string; parameters: Record<string, unknown> }
}

export interface Preset {
  id: string
  slot: PresetSlot
  label: string
  description: string
  permissions: string[]
  /** Chat presets: the fence suggested by default. */
  fence?: string
  needsUrl?: boolean
  needsTool?: boolean
  files: (o: PresetOptions) => { path: string; content: string }[]
}

const page = (title: string, style: string, body: string, script: string) => `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>${title}</title>
<script src="/_sdk/v1.js"></script>
<style>
${style.trim()}
</style>
</head>
<body>
${body.trim()}
<script>
${script.trim()}
</script>
</body>
</html>
`

const manifest = (
  o: PresetOptions,
  description: string,
  permissions: string[],
  contributes: Record<string, unknown>,
) => ({
  path: 'mod.json',
  content: JSON.stringify(
    { apiVersion: 1, id: o.id, name: o.name, version: '1.0.0', description, permissions, contributes },
    null,
    2,
  ),
})

const card = `.card{background:var(--viv-card);border:1px solid var(--viv-border);border-radius:var(--viv-radius);padding:10px 12px}
.muted{color:var(--viv-muted-foreground)}`

// ── Panels ────────────────────────────────────────────────────────────────

const notesPanel: Preset = {
  id: 'notes',
  slot: 'panel',
  label: 'Notes',
  description: 'A scratch pad that saves as you type.',
  permissions: [],
  files: (o) => [
    manifest(o, 'A scratch pad that saves as you type', [], { panels: [{ id: 'main', title: o.name, entry: 'panel.html' }] }),
    {
      path: 'panel.html',
      content: page(
        o.name,
        `html,body{height:100%}body{display:flex;flex-direction:column;padding:10px;box-sizing:border-box;gap:6px}
textarea{flex:1;resize:none;font-family:var(--viv-mono);font-size:13px;line-height:1.6}
.status{font-size:11px;color:var(--viv-muted-foreground)}`,
        `<textarea id="text" placeholder="Write anything…"></textarea><div class="status" id="status">Loading…</div>`,
        `const text = document.getElementById('text')
const status = document.getElementById('status')
let timer
vivianne.storage.get().then((saved) => {
  text.value = (saved && saved.text) || ''
  status.textContent = 'Saved'
})
text.addEventListener('input', () => {
  status.textContent = 'Saving…'
  clearTimeout(timer)
  timer = setTimeout(() => vivianne.storage.set({ text: text.value }).then(() => (status.textContent = 'Saved')), 400)
})`,
      ),
    },
  ],
}

const webPanel: Preset = {
  id: 'web',
  slot: 'panel',
  label: 'Web page',
  description: 'A page you run locally (a dev server, a dashboard) in a panel.',
  permissions: [],
  needsUrl: true,
  files: (o) => [
    manifest(o, `Shows ${o.url}`, [], { panels: [{ id: 'main', title: o.name, url: o.url }] }),
  ],
}

const dashboardPanel: Preset = {
  id: 'dashboard',
  slot: 'panel',
  label: 'Agent dashboard',
  description: 'Cards for every agent at work, updated live.',
  permissions: ['agent.status'],
  files: (o) => [
    manifest(o, 'Every agent at work, live', ['agent.status'], { panels: [{ id: 'main', title: o.name, entry: 'panel.html' }] }),
    {
      path: 'panel.html',
      content: page(
        o.name,
        `body{padding:12px}${card}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(220px,1fr));gap:8px}
.state{font-size:11px;text-transform:uppercase;letter-spacing:.04em}
.working{color:var(--viv-primary)}h3{margin:0 0 4px;font-size:13px}`,
        `<div id="grid" class="grid"><div class="muted">No agents running.</div></div>`,
        `const grid = document.getElementById('grid')
function render(agents) {
  if (!agents || agents.length === 0) {
    grid.innerHTML = '<div class="muted">No agents running.</div>'
    return
  }
  grid.innerHTML = ''
  for (const a of agents) {
    const el = document.createElement('div')
    el.className = 'card'
    el.innerHTML = '<div class="state"></div><h3></h3><div class="muted task"></div><div class="activity"></div>'
    el.querySelector('.state').textContent = a.state
    el.querySelector('.state').classList.toggle('working', a.state === 'working')
    el.querySelector('h3').textContent = a.name + (a.project ? ' · ' + a.project : '')
    el.querySelector('.task').textContent = a.task || ''
    el.querySelector('.activity').textContent = a.activity || ''
    grid.appendChild(el)
  }
}
vivianne.agent.status().then(render)
vivianne.agent.onStatus(render)`,
      ),
    },
  ],
}

const blankPanel: Preset = {
  id: 'blank-panel',
  slot: 'panel',
  label: 'Blank panel',
  description: 'An empty page to build on.',
  permissions: [],
  files: (o) => [
    manifest(o, 'A panel', [], { panels: [{ id: 'main', title: o.name, entry: 'panel.html' }] }),
    { path: 'panel.html', content: page(o.name, 'body{padding:12px}', `<h2>${o.name}</h2><p class="muted">Edit panel.html in this mod's folder; the panel reloads when you save.</p>`, `vivianne.ready.then((ctx) => console.log('connected', ctx))`) },
  ],
}

// ── Chat components ───────────────────────────────────────────────────────

const checklistChat: Preset = {
  id: 'checklist',
  slot: 'chat',
  label: 'Checklist',
  description: 'Renders a list of "- [ ] item" lines as boxes you can tick.',
  permissions: [],
  fence: 'checklist',
  files: (o) => [
    manifest(o, `Renders \`\`\`${o.fence} blocks as a tickable checklist`, [], { chat: [{ fence: o.fence, entry: 'chat.html' }] }),
    {
      path: 'chat.html',
      content: page(
        o.name,
        `${card}ul{list-style:none;margin:0;padding:0}li{display:flex;gap:8px;align-items:flex-start;padding:3px 0}
li.done span{color:var(--viv-muted-foreground);text-decoration:line-through}`,
        `<div class="card"><ul id="list"></ul></div>`,
        `const list = document.getElementById('list')
// What the user ticked, by item text: kept while the reply streams in.
const ticked = new Map()
function render(text) {
  list.innerHTML = ''
  ;(text || '').split('\\n').filter((l) => l.trim()).forEach((line) => {
    const m = /^\\s*[-*]\\s*\\[( |x|X)\\]\\s*(.*)$/.exec(line)
    const label = m ? m[2] : line.replace(/^\\s*[-*]\\s*/, '')
    const done = ticked.has(label) ? ticked.get(label) : !!m && m[1].toLowerCase() === 'x'
    const li = document.createElement('li')
    const box = document.createElement('input')
    box.type = 'checkbox'
    box.checked = done
    const span = document.createElement('span')
    span.textContent = label
    li.className = done ? 'done' : ''
    box.onchange = () => { ticked.set(label, box.checked); li.className = box.checked ? 'done' : '' }
    li.append(box, span)
    list.appendChild(li)
  })
}
vivianne.ready.then((ctx) => render(ctx.data))
vivianne.on('data', render)`,
      ),
    },
  ],
}

const tableChat: Preset = {
  id: 'table',
  slot: 'chat',
  label: 'Sortable table',
  description: 'Renders CSV or a JSON array as a table you can sort.',
  permissions: [],
  fence: 'table',
  files: (o) => [
    manifest(o, `Renders \`\`\`${o.fence} blocks (CSV or JSON) as a sortable table`, [], { chat: [{ fence: o.fence, entry: 'chat.html' }] }),
    {
      path: 'chat.html',
      content: page(
        o.name,
        `${card}.card{padding:0;overflow:auto}table{border-collapse:collapse;width:100%;font-size:12px}
th,td{padding:6px 10px;border-bottom:1px solid var(--viv-border);text-align:left;white-space:nowrap}
th{cursor:pointer;user-select:none;color:var(--viv-muted-foreground);font-weight:600}td.num{text-align:right;font-variant-numeric:tabular-nums}`,
        `<div class="card"><table id="t"></table></div>`,
        `const t = document.getElementById('t')
let rows = [], cols = [], sort = { col: null, dir: 1 }
function parse(text) {
  text = (text || '').trim()
  try {
    const data = JSON.parse(text)
    if (Array.isArray(data) && data.length && typeof data[0] === 'object') {
      cols = Object.keys(data[0])
      return data.map((r) => cols.map((c) => r[c]))
    }
  } catch (e) {}
  const lines = text.split('\\n').filter((l) => l.trim())
  const split = (l) => l.split(',').map((c) => c.trim().replace(/^"|"$/g, ''))
  cols = split(lines.shift() || '')
  return lines.map(split)
}
const isNum = (v) => v !== '' && v != null && !isNaN(Number(v))
function render() {
  const sorted = sort.col == null ? rows : rows.slice().sort((a, b) => {
    const x = a[sort.col], y = b[sort.col]
    return (isNum(x) && isNum(y) ? Number(x) - Number(y) : String(x).localeCompare(String(y))) * sort.dir
  })
  t.innerHTML = ''
  const head = t.insertRow()
  cols.forEach((c, i) => {
    const th = document.createElement('th')
    th.textContent = c + (sort.col === i ? (sort.dir > 0 ? ' ▲' : ' ▼') : '')
    th.onclick = () => { sort = { col: i, dir: sort.col === i ? -sort.dir : 1 }; render() }
    head.appendChild(th)
  })
  sorted.forEach((r) => {
    const tr = t.insertRow()
    r.forEach((v) => { const td = tr.insertCell(); td.textContent = v == null ? '' : v; if (isNum(v)) td.className = 'num' })
  })
}
function update(text) { rows = parse(text); render() }
vivianne.ready.then((ctx) => update(ctx.data))
vivianne.on('data', update)`,
      ),
    },
  ],
}

const flashcardsChat: Preset = {
  id: 'flashcards',
  slot: 'chat',
  label: 'Flashcards',
  description: 'Turns "Q: … / A: …" pairs into cards you flip.',
  permissions: [],
  fence: 'flashcards',
  files: (o) => [
    manifest(o, `Renders \`\`\`${o.fence} blocks (Q:/A: pairs) as flip cards`, [], { chat: [{ fence: o.fence, entry: 'chat.html' }] }),
    {
      path: 'chat.html',
      content: page(
        o.name,
        `${card}.card{min-height:70px;display:flex;flex-direction:column;justify-content:center;cursor:pointer;text-align:center;font-size:14px}
.row{display:flex;justify-content:space-between;align-items:center;margin-top:6px;font-size:12px}`,
        `<div class="card" id="card"></div><div class="row"><button class="secondary" id="prev">Back</button><span class="muted" id="pos"></span><button class="secondary" id="next">Next</button></div>`,
        `let cards = [], i = 0, flipped = false
const el = (id) => document.getElementById(id)
function parse(text) {
  const out = []
  ;(text || '').split(/\\n(?=\\s*Q:)/).forEach((block) => {
    const q = /Q:\\s*([\\s\\S]*?)(?=\\n\\s*A:|$)/.exec(block)
    const a = /A:\\s*([\\s\\S]*)$/.exec(block)
    if (q) out.push({ q: q[1].trim(), a: a ? a[1].trim() : '' })
  })
  return out
}
function show() {
  if (!cards.length) { el('card').textContent = 'No cards'; el('pos').textContent = ''; return }
  const c = cards[i]
  el('card').textContent = flipped ? c.a : c.q
  el('card').style.opacity = flipped ? 0.85 : 1
  el('pos').textContent = (i + 1) + ' / ' + cards.length + (flipped ? ' · answer' : ' · tap to flip')
}
el('card').onclick = () => { flipped = !flipped; show() }
el('next').onclick = () => { i = (i + 1) % Math.max(cards.length, 1); flipped = false; show() }
el('prev').onclick = () => { i = (i - 1 + cards.length) % Math.max(cards.length, 1); flipped = false; show() }
function update(text) { cards = parse(text); i = Math.min(i, Math.max(cards.length - 1, 0)); show() }
vivianne.ready.then((ctx) => update(ctx.data))
vivianne.on('data', update)`,
      ),
    },
  ],
}

const blankChat: Preset = {
  id: 'blank-chat',
  slot: 'chat',
  label: 'Blank chat component',
  description: 'Renders a fence of your own; you write the page.',
  permissions: [],
  fence: 'widget',
  files: (o) => [
    manifest(o, `Renders \`\`\`${o.fence} blocks`, [], { chat: [{ fence: o.fence, entry: 'chat.html' }] }),
    {
      path: 'chat.html',
      content: page(o.name, card, `<div class="card"><pre id="out"></pre></div>`, `const out = document.getElementById('out')
vivianne.ready.then((ctx) => (out.textContent = ctx.data))
vivianne.on('data', (text) => (out.textContent = text))`),
    },
  ],
}

// ── Sidebar ───────────────────────────────────────────────────────────────

const linksSidebar: Preset = {
  id: 'links',
  slot: 'sidebar',
  label: 'Quick links',
  description: 'Links you use often, one click away.',
  permissions: ['open'],
  files: (o) => [
    manifest(o, 'Links you use often', ['open'], { sidebar: [{ id: 'main', title: o.name, entry: 'sidebar.html', height: 60 }] }),
    {
      path: 'sidebar.html',
      content: page(
        o.name,
        `body{padding:2px 8px 6px}a{display:block;padding:3px 6px;border-radius:6px;text-decoration:none;color:var(--viv-foreground);cursor:pointer}
a:hover{background:var(--viv-accent)}.x{float:right;opacity:.5}form{display:flex;gap:4px;margin-top:4px}input{flex:1;min-width:0;font-size:12px}`,
        `<div id="list"></div><form id="add"><input id="url" placeholder="https://…" required><button>Add</button></form>`,
        `let links = []
const list = document.getElementById('list')
function save() { vivianne.storage.set({ links }); render() }
function render() {
  list.innerHTML = ''
  links.forEach((l, i) => {
    const a = document.createElement('a')
    a.textContent = l.replace(/^https?:\\/\\//, '')
    a.onclick = () => vivianne.open.url(l)
    const x = document.createElement('span')
    x.className = 'x'
    x.textContent = '×'
    x.onclick = (e) => { e.stopPropagation(); links.splice(i, 1); save() }
    a.appendChild(x)
    list.appendChild(a)
  })
}
document.getElementById('add').onsubmit = (e) => {
  e.preventDefault()
  const input = document.getElementById('url')
  const v = input.value.trim()
  if (/^https?:\\/\\//.test(v)) { links.push(v); input.value = ''; save() }
}
vivianne.storage.get().then((s) => { links = (s && s.links) || []; render() })`,
      ),
    },
  ],
}

const timerSidebar: Preset = {
  id: 'focus-timer',
  slot: 'sidebar',
  label: 'Focus timer',
  description: '25 minutes of focus, 5 of rest, with a notification.',
  permissions: ['notify'],
  files: (o) => [
    manifest(o, 'A pomodoro timer', ['notify'], { sidebar: [{ id: 'main', title: o.name, entry: 'sidebar.html', height: 64 }] }),
    {
      path: 'sidebar.html',
      content: page(
        o.name,
        `body{padding:2px 10px 8px;display:flex;align-items:center;gap:8px}.time{font-family:var(--viv-mono);font-size:20px;flex:1}
.phase{font-size:11px;color:var(--viv-muted-foreground)}`,
        `<div style="flex:1"><div class="time" id="time">25:00</div><div class="phase" id="phase">Focus</div></div><button id="go">Start</button>`,
        `const FOCUS = 25 * 60, REST = 5 * 60
let left = FOCUS, phase = 'Focus', timer = null
const $ = (id) => document.getElementById(id)
function draw() {
  $('time').textContent = String(Math.floor(left / 60)).padStart(2, '0') + ':' + String(left % 60).padStart(2, '0')
  $('phase').textContent = phase
  $('go').textContent = timer ? 'Pause' : 'Start'
}
function tick() {
  left--
  if (left <= 0) {
    vivianne.notify(phase === 'Focus' ? 'Focus done. Take five.' : 'Break over. Back to it.')
    phase = phase === 'Focus' ? 'Break' : 'Focus'
    left = phase === 'Focus' ? FOCUS : REST
  }
  draw()
}
$('go').onclick = () => { if (timer) { clearInterval(timer); timer = null } else { timer = setInterval(tick, 1000) }; draw() }
draw()`,
      ),
    },
  ],
}

const todaySidebar: Preset = {
  id: 'today',
  slot: 'sidebar',
  label: 'Today',
  description: "A short to-do list for today that's kept between launches.",
  permissions: [],
  files: (o) => [
    manifest(o, "Today's to-dos", [], { sidebar: [{ id: 'main', title: o.name, entry: 'sidebar.html', height: 80 }] }),
    {
      path: 'sidebar.html',
      content: page(
        o.name,
        `body{padding:2px 8px 6px}label{display:flex;gap:6px;align-items:center;padding:2px 4px;font-size:12px}
label.done{color:var(--viv-muted-foreground);text-decoration:line-through}input[type=text]{width:100%;box-sizing:border-box;font-size:12px;margin-top:4px}`,
        `<div id="list"></div><input type="text" id="new" placeholder="Add a to-do and press Enter">`,
        `let items = []
const list = document.getElementById('list')
function save() { vivianne.storage.set({ items }); render() }
function render() {
  list.innerHTML = ''
  items.forEach((it, i) => {
    const l = document.createElement('label')
    l.className = it.done ? 'done' : ''
    const c = document.createElement('input')
    c.type = 'checkbox'
    c.checked = it.done
    c.onchange = () => { it.done = c.checked; save() }
    const s = document.createElement('span')
    s.textContent = it.text
    s.ondblclick = () => { items.splice(i, 1); save() }
    l.append(c, s)
    list.appendChild(l)
  })
}
document.getElementById('new').onkeydown = (e) => {
  if (e.key === 'Enter' && e.target.value.trim()) { items.push({ text: e.target.value.trim(), done: false }); e.target.value = ''; save() }
}
vivianne.storage.get().then((s) => { items = (s && s.items) || []; render() })`,
      ),
    },
  ],
}

const blankSidebar: Preset = {
  id: 'blank-sidebar',
  slot: 'sidebar',
  label: 'Blank section',
  description: 'An empty sidebar section to build on.',
  permissions: [],
  files: (o) => [
    manifest(o, 'A sidebar section', [], { sidebar: [{ id: 'main', title: o.name, entry: 'sidebar.html', height: 48 }] }),
    { path: 'sidebar.html', content: page(o.name, 'body{padding:4px 10px}', `<div class="muted">Edit sidebar.html in this mod's folder.</div>`, '') },
  ],
}

// ── Tools ─────────────────────────────────────────────────────────────────

const shellTool: Preset = {
  id: 'shell-tool',
  slot: 'tool',
  label: 'Shell command',
  description: 'A tool Vivianne can call that runs a command ({{param}} fills in arguments).',
  permissions: ['shell'],
  needsTool: true,
  files: (o) => [
    manifest(o, o.tool?.description ?? 'A shell tool', ['shell'], {
      tools: [
        {
          name: o.tool?.name ?? o.id.replace(/-/g, '_'),
          description: o.tool?.description ?? '',
          parameters: o.tool?.parameters ?? { type: 'object', properties: {} },
          command: o.tool?.command ?? 'echo hello',
        },
      ],
    }),
  ],
}

export const PRESETS: Preset[] = [
  notesPanel,
  webPanel,
  dashboardPanel,
  blankPanel,
  checklistChat,
  tableChat,
  flashcardsChat,
  blankChat,
  linksSidebar,
  timerSidebar,
  todaySidebar,
  blankSidebar,
  shellTool,
]

export const SLOT_LABELS: Record<PresetSlot, string> = {
  panel: 'Panel',
  chat: 'Chat component',
  sidebar: 'Sidebar section',
  tool: 'Tool',
}

/** A mod id from a name: lowercase letters, digits and dashes. */
export function idFromName(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 40)
}
