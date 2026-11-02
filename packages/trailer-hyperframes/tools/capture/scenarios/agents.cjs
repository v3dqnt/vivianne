// Scene 3: the General tab's always-on agent (Ava) briefs Sam. Each state
// adds one thing to the last, so the scene can cut between captures
// invisibly and animate only what is new.
const ROOT = '/Users/sam/.viv/general'
const T0 = Date.parse('2026-11-02T07:42:00Z')

module.exports = (S) => {
  const init = `
    window.__MOCK__ = Object.assign(window.__MOCK__ || {}, {
      cmd_general_agents_root: ${JSON.stringify(ROOT)},
      cmd_list_team: (a) => a.root === ${JSON.stringify(ROOT)} ? [{
        id: 'ava', name: ${JSON.stringify(S.agent.name)}, role: ${JSON.stringify(S.agent.role)}, status: 'active', reportsTo: null, model: null, allowedTools: null,
        owns: [], color: null, created: '2026-09-01', proposedBy: 'user', reason: null,
        instructions: 'Keep Sam on top of email, Slack and messages.', feedback: [], path: ${JSON.stringify(ROOT)} + '/ava'
      }] : [],
      cmd_bots_status: (a) => [{ root: a.root, botId: 'ava', state: 'idle', activity: null }],
      cmd_bots_dm: () => [],
      cmd_bots_room: () => [],
    })
  `
  let n = 0
  const message = (from, text, kind = 'message', steps) => ({ id: `m${++n}`, from, text, ts: T0 + n * 20000, kind, mentions: [], steps })
  const post = (page, m) => page.evaluate(([root, m]) => window.__emit('bots:message', { root, channel: 'ava', message: m }), [ROOT, m])
  const status = (page, state, activity) => page.evaluate(([root, state, activity]) => window.__emit('bots:status', { root, botId: 'ava', state, activity }), [ROOT, state, activity])
  const steps = (page, list) => page.evaluate(([root, steps]) => window.__emit('bots:steps', { root, botId: 'ava', channel: 'ava', steps }), [ROOT, list])
  const step = (id, tool, label, state) => ({ id, tool, label, state })

  const reading = [step('s1', 'gmail', 'Read Gmail · 14 new', 'done'), step('s2', 'slack', 'Read Slack · #launch, DMs', 'done'), step('s3', 'whatsapp', 'Read WhatsApp · 2 chats', 'running')]
  const done = reading.map((s) => ({ ...s, state: 'done' }))
  const b = S.briefing

  return {
    init: [init],
    states: [
      {
        name: 'ag-0',
        run: async (page) => {
          await page.mouse.click(32, 71) // rail: General
          await page.waitForTimeout(400)
          await page.getByText(S.agent.name, { exact: true }).first().click()
        },
      },
      {
        name: 'ag-1',
        run: async (page) => {
          await post(page, message('user', S.greeting))
          await status(page, 'working', 'Reading your inbox')
          await steps(page, reading)
        },
      },
      {
        name: 'ag-2',
        run: async (page) => {
          await status(page, 'idle', null)
          await post(page, message('ava', `${b.intro}\n\n**Email** — ${b.email}\n\n**Slack** — ${b.slack}`, 'message', done))
        },
      },
      {
        name: 'ag-3',
        run: async (page) => {
          await post(page, message('ava', `**${S.whatsapp.from}** on WhatsApp: “${S.whatsapp.text}”`, 'needs_you'))
        },
      },
      {
        name: 'ag-4',
        run: async (page) => {
          await post(page, message('ava', `${S.groceriesIntro}\n\n${S.groceries.map((g) => `- [ ] ${g}`).join('\n')}`))
        },
      },
    ],
  }
}
