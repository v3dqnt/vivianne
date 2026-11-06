#!/usr/bin/env node
/**
 * Writes a "UI debug" thread into ~/.viv/agent/sessions: one conversation
 * holding every kind of message, tool call and rich block the chat can draw,
 * so the code-mode redesign can be checked in one scroll.
 *
 *   node packages/gui-next/scripts/ui-debug-thread.mjs
 *
 * Re-running overwrites the same thread (fixed id). Reload the app, or switch
 * threads, to see changes.
 */
import { mkdirSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { deflateSync } from 'node:zlib'

const ID = 'session_1793300000000'
let clock = Date.now() - 60 * 60 * 1000
const t = () => (clock += 4000)
let callN = 0
const cid = () => `call_debug_${++callN}`

// ── a small PNG, drawn here so the thread has a real image ──────────────────
function png(w, h, px) {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    return c >>> 0
  })
  const crc = (buf) => {
    let c = 0xffffffff
    for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8)
    return (c ^ 0xffffffff) >>> 0
  }
  const chunk = (type, data) => {
    const len = Buffer.alloc(4)
    len.writeUInt32BE(data.length)
    const td = Buffer.concat([Buffer.from(type), data])
    const c = Buffer.alloc(4)
    c.writeUInt32BE(crc(td))
    return Buffer.concat([len, td, c])
  }
  const raw = Buffer.alloc((w * 3 + 1) * h)
  for (let y = 0; y < h; y++) {
    raw[y * (w * 3 + 1)] = 0
    for (let x = 0; x < w; x++) raw.set(px(x, y), y * (w * 3 + 1) + 1 + x * 3)
  }
  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(w, 0)
  ihdr.writeUInt32BE(h, 4)
  ihdr[8] = 8
  ihdr[9] = 2
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]).toString('base64')
}
const SKY = png(320, 180, (x, y) => [80 + (y * 120) / 180, 160 + (x * 60) / 320, 255 - (y * 40) / 180])

// ── message builders ─────────────────────────────────────────────────────────
const text = (s) => ({ type: 'text', text: s })
const user = (...content) => ({ type: 'user', content: content.map((c) => (typeof c === 'string' ? text(c) : c)), timestamp: t() })
const usage = { input: 2140, output: 380, cacheRead: 9200, cacheWrite: 0, totalTokens: 11720, cost: null }
const assistant = (content, extra = {}) => ({ type: 'assistant', content, stopReason: 'stop', errorMessage: null, timestamp: t(), usage, ...extra })

/** One tool call and its result, as two messages. */
function tool(name, args, output, { error = false, details = null, images = [] } = {}) {
  const id = cid()
  return [
    { type: 'assistant', content: [{ type: 'toolCall', id, name, arguments: args }], stopReason: 'toolUse', errorMessage: null, timestamp: t(), usage },
    {
      type: 'toolResult',
      toolCallId: id,
      toolName: name,
      content: [text(output), ...images.map((b64) => ({ type: 'image', url: null, base64: b64, mediaType: 'image/png' }))],
      details,
      isError: error,
      timestamp: t(),
      arguments: args,
    },
  ]
}
const thinking = (s) => ({ type: 'thinking', thinking: s })
const fence = (lang, body) => '```' + lang + '\n' + body + '\n```'

// ── the conversation ─────────────────────────────────────────────────────────
const messages = [
  // 1. Plain conversation and the markdown it can carry
  user('Show me everything the chat can draw. Start with plain text and markdown.'),
  assistant([
    thinking('The user wants a tour of every renderer. Start with prose and markdown, then tools, then the rich blocks.'),
    text(
      [
        "Here is the **markdown** set. A sentence with *italic*, `inline code`, a [link](https://example.com) and a file path `src/App.tsx`.",
        '',
        '## A heading',
        '### A smaller heading',
        '',
        '- A bullet',
        '- Another bullet with **bold**',
        '  - A nested bullet',
        '',
        '1. First step',
        '2. Second step',
        '',
        '> A quote, for when something is worth setting apart.',
        '',
        '| Column | Value | Note |',
        '|---|---:|---|',
        '| Alpha | 12 | first row |',
        '| Beta | 340 | second row |',
        '',
        'Inline math $E = mc^2$ and a display formula:',
        '',
        '$$\\int_0^1 x^2\\,dx = \\tfrac{1}{3}$$',
        '',
        fence('ts', "export function greet(name: string): string {\n  return `Hello, ${name}`\n}"),
        '',
        '---',
        '',
        'And a closing line after a rule.',
      ].join('\n'),
    ),
  ]),

  // 2. Reading and searching (these fold into one tool group)
  user('Now read and search the project.'),
  assistant([thinking('Read the entry point, list the folder, then search.'), text('Looking at the project first.')], { stopReason: 'toolUse' }),
  ...tool('read', { path: 'src/main.tsx' }, "import { StrictMode } from 'react'\nimport { createRoot } from 'react-dom/client'\nimport App from './App'\n\ncreateRoot(document.getElementById('root')!).render(<App />)\n"),
  ...tool('ls', { path: 'src' }, 'src/\n  App.tsx\n  main.tsx\n  ui/\n  modules/\n  theme.ts'),
  ...tool('grep', { pattern: 'useAgentStore', path: 'src' }, 'src/modules/ai/components/Composer.tsx:140:  const isBusy = useAgentStore((s) => s.isBusy)\nsrc/modules/ai/components/ChatView.tsx:22:  const messages = useAgentStore((s) => s.messages)\n\n2 matches'),
  ...tool('glob', { pattern: 'src/**/*.test.ts' }, 'src/lib/diff.test.ts\nsrc/modules/ai/lib/toolGroups.test.ts'),
  ...tool('code_search', { query: 'where is the composer submit handled' }, 'src/modules/ai/components/Composer.tsx:278  const submit = (typed, a) => { ... }'),

  // 3. Running things: success, failure, long output
  assistant([text('Running the tests.')], { stopReason: 'toolUse' }),
  ...tool('bash', { command: 'npm test' }, '[stdout]\n> gui-next@0.1.0 test\n> node --test\n\n✔ diff splits hunks (2.1ms)\n✔ tool groups fold reads (1.4ms)\nℹ tests 2\nℹ pass 2\nℹ fail 0\n\n[exit code: 0]'),
  ...tool('powershell', { command: 'npx tsc -p tsconfig.app.json --noEmit' }, "[stdout]\nsrc/App.tsx(12,7): error TS2322: Type 'string' is not assignable to type 'number'.\n\n[exit code: 2]", { error: true }),
  ...tool('bash', { command: 'git log --oneline -40' }, Array.from({ length: 40 }, (_, i) => `${(0xabc123 + i * 7919).toString(16).slice(0, 7)} Commit message number ${40 - i}`).join('\n')),

  // 4. Changing files: write (new and overwrite), edit, apply_patch
  assistant([thinking('Fix the type error, then add a test file.'), text('Fixing the type error.')], { stopReason: 'toolUse' }),
  ...tool('edit', { path: 'src/App.tsx', old_string: "const count: number = '0'", new_string: 'const count: number = 0' }, 'Edited src/App.tsx (1 replacement).'),
  ...tool('write', { path: 'src/lib/sum.ts', content: 'export const sum = (a: number, b: number) => a + b\n' }, 'Wrote src/lib/sum.ts (1 line).'),
  ...tool('write', { path: 'README.md', content: '# Project\n\nA new, shorter readme.\n' }, 'Wrote README.md (3 lines).', { details: { previous_content: '# Project\n\nThe old readme, longer than the new one.\nWith a second paragraph.\n' } }),
  ...tool('apply_patch', { patch: '*** Begin Patch ...' }, 'Applied patch to 2 files.', {
    details: {
      filesDiff: [
        { path: 'src/theme.ts', before: 'export const radius = 10\n', after: 'export const radius = 12\n' },
        { path: 'src/ui/Button.tsx', before: "const height = 28\n", after: "const height = 32\nconst gap = 6\n" },
      ],
    },
  }),
  ...tool('edit', { path: 'src/missing.ts', old_string: 'foo', new_string: 'bar' }, 'Error: old_string not found in src/missing.ts', { error: true }),

  // 5. The web
  assistant([text('Checking the docs online.')], { stopReason: 'toolUse' }),
  ...tool('web_search', { query: 'react 19 use hook' }, '1. React 19 release notes\n   https://react.dev/blog/2024/12/05/react-19\n2. The use() API\n   https://react.dev/reference/react/use\n3. Upgrading to React 19\n   https://react.dev/blog/2024/04/25/react-19-upgrade-guide'),
  ...tool('web_fetch', { url: 'https://react.dev/reference/react/use' }, 'use is a React API that lets you read the value of a resource like a Promise or context.'),

  // 6. Planning, delegation, the team
  ...tool('todo_write', { todos: [] }, '[x] Read the project\n[x] Fix the type error\n[~] Add tests for sum\n[ ] Update the changelog'),
  ...tool('task', { description: 'Write unit tests for sum', subagent_type: 'general' }, 'Subagent completed (6 tool calls, 3 turns):\n\nAdded src/lib/sum.test.ts with 4 cases; all pass.'),
  ...tool('message_agent', { to: 'all', text: 'The type error is fixed; tests are next.' }, 'Sent.'),

  // 7. A screenshot from a tool, and a running call
  ...tool('dip', { action: 'screenshot' }, 'Screenshot of the main window.', { images: [SKY] }),
  ...tool('math', { expression: 'integrate(x^2, x, 0, 1)' }, '1/3'),
  assistant([text('All of that is the tool layer. Now the rich blocks.')]),

  // 8. Intelligent UI
  user('Show the interactive answers.'),
  assistant([
    text(
      [
        'A report card:',
        '',
        fence(
          'ui',
          JSON.stringify({
            title: 'Weekly dependency check',
            icon: '📦',
            status: { label: '2 need review', tone: 'warning' },
            blocks: [
              { type: 'stats', items: [{ label: 'Checked', value: 148, format: 'integer' }, { label: 'Outdated', value: 9, format: 'integer' }, { label: 'Security', value: 2, format: 'integer' }] },
              { type: 'steps', items: [{ title: 'Scan lockfile', text: '148 packages', state: 'done' }, { title: 'Check advisories', text: '2 found', state: 'active' }, { title: 'Open fixes', state: 'todo' }] },
              { type: 'cards', items: [{ title: 'lodash 4.17.20', text: 'Prototype pollution fixed in 4.17.21.', badge: 'Security', tone: 'error', icon: '🚨' }, { title: 'react 18.2', text: '19.0 is out; no breaking changes in our usage.', badge: 'Optional', tone: 'info' }] },
              { type: 'kv', items: [{ label: 'Branch', value: 'main' }, { label: 'Owner', value: 'vedant' }] },
              { type: 'progress', label: 'Upgraded', value: 0.62, hint: '92 of 148' },
              { type: 'badges', items: [{ label: 'npm', tone: 'neutral' }, { label: 'weekly', tone: 'info' }] },
              { type: 'callout', tone: 'warning', text: 'The two security fixes are safe to merge today.' },
              { type: 'buttons', items: [{ label: 'Open the fixes', send: 'Open PRs for the two security updates', primary: true }, { label: 'Skip', send: 'Skip this week' }] },
            ],
          }),
        ),
        '',
        'A live calculator:',
        '',
        fence(
          'ui',
          JSON.stringify({
            title: 'Savings',
            inputs: [
              { id: 'monthly', type: 'slider', label: 'Each month', min: 50, max: 2000, step: 50, value: 400, format: 'currency' },
              { id: 'rate', type: 'slider', label: 'Yearly return', min: 0, max: 0.12, step: 0.005, value: 0.06, format: 'percent' },
              { id: 'years', type: 'number', label: 'Years', value: 10, format: 'integer' },
            ],
            computed: { total: 'monthly * 12 * years * (1 + rate * years / 2)' },
            blocks: [
              { type: 'inputs' },
              { type: 'stats', items: [{ label: 'After {years} years', value: 'total', format: 'currency' }] },
              { type: 'chart', chart: 'line', x: { from: 0, to: 20, step: 1 }, series: [{ name: 'Balance', y: 'monthly * 12 * x * (1 + rate * x / 2)' }] },
            ],
          }),
        ),
      ].join('\n'),
    ),
  ]),
  assistant([
    text(
      [
        'A chart:',
        '',
        fence('chart', JSON.stringify({ type: 'bar', title: 'Build time (s)', xLabel: 'Week', series: [{ name: 'Before', points: [{ x: 'W1', y: 42 }, { x: 'W2', y: 39 }, { x: 'W3', y: 44 }] }, { name: 'After', points: [{ x: 'W1', y: 31 }, { x: 'W2', y: 27 }, { x: 'W3', y: 25 }] }] })),
        '',
        'A diagram:',
        '',
        fence('mermaid', 'flowchart LR\n  A[Composer] --> B[agentStore]\n  B --> C[cmd_agent_prompt]\n  C --> D[loop_core]\n  D --> E[(Provider)]'),
        '',
        'An OpenUI card:',
        '',
        fence(
          'openui-lang',
          [
            'root = Card([header, now, week, more])',
            'header = CardHeader("Panaji, Goa", "Now, from the forecast")',
            'now = TextContent("**29 °C** · humid, light breeze", "large-heavy")',
            'week = Table([days, highs, skies])',
            'days = Col("Day", ["Fri", "Sat", "Sun"])',
            'highs = Col("High (°C)", [31, 30, 29], "number")',
            'skies = Col("Sky", ["Sunny", "Cloudy", "Showers"])',
            'more = FollowUpBlock([FollowUpItem("Hourly for today")])',
          ].join('\n'),
        ),
        '',
        'A widget:',
        '',
        fence(
          'widget',
          '<style>.w{padding:16px;display:grid;gap:12px}button{font:inherit;padding:6px 14px;border-radius:999px;border:0;background:var(--viv-primary);color:var(--viv-primary-foreground)}</style><div class="w"><div id="n">0 clicks</div><button id="b">Click</button></div><script>let n=0;b.onclick=()=>{n++;document.getElementById("n").textContent=n+" clicks"}</script>',
        ),
      ].join('\n'),
    ),
  ]),

  // 9. A user message with an image, and the failure shapes
  user('Here is a screenshot of the bug.', { type: 'image', url: null, base64: SKY, mediaType: 'image/png' }),
  assistant([text('Thanks, I can see it. One more case: when the provider fails.')]),
  { type: 'assistant', content: [text('Error: 429 Too Many Requests: rate limit reached for this key.')], stopReason: 'error', errorMessage: '429 Too Many Requests: rate limit reached for this key.', timestamp: t(), usage: null },
  user('That covers it.'),
  assistant([text('That is every message, tool and block the chat draws today.')]),
]

const session = {
  id: ID,
  name: 'UI debug: every message, tool and block',
  messages,
  created_at: new Date().toISOString(),
  workspace_folders: [homedir()],
  access_mode: 'full',
}

const dir = join(homedir(), '.viv', 'agent', 'sessions')
mkdirSync(dir, { recursive: true })
const file = join(dir, `${ID}.json`)
writeFileSync(file, JSON.stringify(session, null, 2))
console.log(`Wrote ${messages.length} messages to ${file}`)
