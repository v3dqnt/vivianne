// Scene 5: code mode. Sam opens a new thread in northwind-site, picks issue
// #142 from the Issues tab, asks Vivianne to fix it, switches the model,
// sends; Vivianne delegates to subagents and lands the fix.
//
// The app has no Issues tab yet (GitHub issues are agent tools only), so
// `issues.js` adds one to the real dock: a clone of the Preview tab and a
// panel drawn in the app's own type and colours.
const fs = require('fs')
const path = require('path')

module.exports = (S) => {
  const T0 = Date.parse('2026-11-02T09:14:00Z')
  const subs = S.subagents.map((a, i) => ({
    id: `sa${i + 1}`, subagentType: a.name.toLowerCase().replace(/\s+/g, '-'), description: a.task, prompt: a.task,
    cwd: '/Users/sam/code/northwind-site', status: 'running', startedAtEpochMs: T0, elapsedMs: 0, turns: 0, toolCalls: 0, output: null, error: null,
  }))
  const stats = (id, name) => ({ contextTokens: 18400, contextWindow: 1000000, contextPercent: 1.8, messageCount: 0, inputTokens: 0, outputTokens: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, modelId: id, modelName: name, provider: 'anthropic', maxTokens: 64000, breakdown: { systemPrompt: 9000, toolSchemas: 9400, user: 0, assistant: 0, toolResults: 0 } })
  const init = `
    // A clock the scenario advances, so the app's own durations ("Task 1 of 4
    // done · 1m 38s") add up to the story's run time.
    window.__now = ${T0};
    Date.now = () => window.__now;
    window.__tick = (ms) => (window.__now += ms);
    window.__SUBS = [];
    window.__STATS = ${JSON.stringify(stats('claude-sonnet-5-5', 'Claude Sonnet 5.5'))};
    window.__MOCK__ = Object.assign(window.__MOCK__ || {}, {
      cmd_coding_list_subagents: () => window.__SUBS,
      cmd_git_current_branch: 'main',
      cmd_agent_session_stats: () => window.__STATS,
      cmd_agent_set_model: (a) => { window.__STATS = Object.assign({}, window.__STATS, { modelId: a.id, modelName: a.name }); return null },
    });
    window.__agent = (event) => window.__emit('agent_event', { threadId: '', event });
  `
  const issuesJs = fs.readFileSync(path.join(__dirname, 'issues.js'), 'utf8')
  const ev = (page, event) => page.evaluate((e) => window.__agent(e), event)
  const tick = (page, s) => page.evaluate((ms) => window.__tick(ms), s * 1000)
  const setSubs = (page, list) => page.evaluate((l) => (window.__SUBS = l), list)
  const text = (t) => [{ type: 'text', text: t }]
  const assistant = (content, ts, stopReason = 'toolUse') => ({ type: 'assistant', content, stopReason, errorMessage: null, timestamp: ts })
  const say = async (page, t, ts, stopReason) => {
    const m = assistant(text(t), ts, stopReason)
    await ev(page, { type: 'message_start', payload: { message: m } })
    await ev(page, { type: 'message_end', payload: { message: m } })
  }
  const toolStart = (id, name, args) => ({ type: 'tool_execution_start', payload: { tool_call_id: id, tool_name: name, args } })
  const toolEnd = (id, name, out, details = null) => ({ type: 'tool_execution_end', payload: { tool_call_id: id, tool_name: name, result: { content: text(out), details, terminate: null }, is_error: false } })
  const tool = async (page, id, name, args, out) => {
    await ev(page, toolStart(id, name, args))
    if (out !== undefined) await ev(page, toolEnd(id, name, out))
  }
  const plan = (st) => ({
    todos: [
      { content: 'Find why the Pay now tap does nothing on WebKit', status: st[0] },
      { content: 'Fix the cause', status: st[1] },
      { content: 'Add a WebKit regression test', status: st[2] },
      { content: 'Run the test suite', status: st[3] },
    ],
  })
  const issues = (page, view) => page.evaluate(([src, story, view]) => {
    window.STORY = story
    if (!window.__issues) (0, eval)(src)
    window.__issues(view)
    // App issue: the floating plan pill (TodoPanel) covers the first user
    // message while the transcript is short. Inset the transcript so the pill
    // clears it (capture only).
    for (const el of document.querySelectorAll('div')) {
      if (getComputedStyle(el).maxWidth === '880px' && el.closest('[class*="dv-"]')) el.style.paddingTop = '44px'
    }
  }, [issuesJs, S, view])

  return {
    init: [init],
    states: [
      {
        name: 'cm-0',
        run: async (page) => {
          await page.getByText('New thread', { exact: true }).first().click()
          await page.waitForTimeout(300)
          await issues(page, 'tab')
        },
      },
      { name: 'cm-issues', run: (page) => issues(page, 'list') },
      { name: 'cm-issue', run: (page) => issues(page, 'detail') },
      {
        name: 'cm-typed',
        run: async (page) => {
          await page.locator('textarea').first().click()
          await page.keyboard.type(S.prompt, { delay: 0 })
          await issues(page, 'detail')
        },
      },
      {
        name: 'cm-menu',
        run: async (page) => {
          await page.getByText('Claude Sonnet 5.5').first().click()
          await page.waitForTimeout(500)
          // The menu opens scrolled to the current model, tucking the row above it under the header.
          // (Nothing else on the page is scrolled at this point.)
          await page.evaluate(() => document.querySelectorAll('*').forEach((el) => el.scrollTop > 0 && (el.scrollTop = 0)))
          // App bug (ModelSelector.tsx STICKY_HEADER_H = 84, but the search block is
          // shorter): provider headers stick 14px low, over each group's first model.
          // Pin them at the block's real height for the capture only.
          await page.evaluate(() => {
            const heads = [...document.querySelectorAll('.ui-Menu-list .ui-ListSubheader')]
            const block = [...document.querySelectorAll('.ui-Menu-list > *')].find((n) => getComputedStyle(n).position === 'sticky' && getComputedStyle(n).top === '0px')
            if (block) heads.forEach((h) => (h.style.top = `${block.getBoundingClientRect().height}px`))
          })
          await page.waitForTimeout(200)
        },
      },
      {
        name: 'cm-picked',
        run: async (page) => {
          await page.getByRole('option', { name: /Claude Opus 5\.5/ }).or(page.getByText('Claude Opus 5.5')).first().click()
          await page.waitForTimeout(500)
          await issues(page, 'detail')
        },
      },
      {
        name: 'cm-run-1',
        run: async (page) => {
          await page.locator('textarea').first().press('Enter')
          await setSubs(page, subs.map((s, i) => ({ ...s, elapsedMs: 6000 + i * 1100, turns: 2, toolCalls: 2 + i })))
          await ev(page, { type: 'agent_start' })
          const u = { type: 'user', content: text(S.prompt), timestamp: T0 }
          await ev(page, { type: 'message_start', payload: { message: u } })
          await ev(page, { type: 'message_end', payload: { message: u } })
          await tick(page, 2)
          await say(page, "On it. I'll split this up: one agent traces the click handler, one reproduces the bug on WebKit, one writes the regression test.", T0 + 2000)
          await tool(page, 'td1', 'todo_write', plan(['in_progress', 'pending', 'pending', 'pending']), 'Plan updated')
          for (const [i, a] of S.subagents.entries()) await tool(page, `tk${i}`, 'task', { description: a.task, subagent_type: subs[i].subagentType, prompt: a.task })
          await issues(page, 'detail')
        },
      },
      {
        name: 'cm-run-2',
        run: async (page) => {
          await tick(page, 96)
          await setSubs(page, subs.map((s, i) => ({ ...s, status: 'completed', elapsedMs: [72000, 95000, 64000][i], turns: [6, 8, 5][i], toolCalls: [11, 14, 7][i], output: 'done' })))
          for (const [i] of S.subagents.entries()) await ev(page, toolEnd(`tk${i}`, 'task', 'done'))
          await say(page, 'Found it. On touch devices an invisible `.promo-overlay` sits on top of the button and swallows the tap — Safari never fires `click`.', T0 + 98000)
          await tick(page, 4)
          await tool(page, 'td2', 'todo_write', plan(['completed', 'in_progress', 'pending', 'pending']), 'Plan updated')
          await tick(page, 20)
          await tool(
            page,
            'ed1',
            'edit',
            {
              path: 'src/checkout/PromoOverlay.css',
              old_string: '.promo-overlay {\n  position: fixed;\n  inset: 0;\n  z-index: 40;\n}',
              new_string: '.promo-overlay {\n  position: fixed;\n  inset: 0;\n  z-index: 40;\n  pointer-events: none;\n}\n\n.promo-overlay > * {\n  pointer-events: auto;\n}',
            },
            'Edited src/checkout/PromoOverlay.css',
          )
          // Subagents are done: fold their panel so the diff has room.
          await page.getByText('3 subagents', { exact: true }).first().click().catch(() => {})
          await page.getByText('Edited', { exact: false }).last().click().catch(() => {})
          await page.waitForTimeout(300)
          await issues(page, 'detail')
        },
      },
      {
        name: 'cm-run-3',
        run: async (page) => {
          await tick(page, 12)
          await tool(page, 'td3', 'todo_write', plan(['completed', 'completed', 'completed', 'in_progress']), 'Plan updated')
          await tick(page, 43)
          await tool(page, 'sh1', 'bash', { command: 'npm test -- --project=webkit' }, '48 passed (31.2s)')
          await tool(page, 'td4', 'todo_write', plan(['completed', 'completed', 'completed', 'completed']), 'Plan updated')
          await tick(page, 15)
          await say(page, `${S.fixSummary}\n\nDone in ${S.elapsed}.`, T0 + 192000, 'stop')
          await ev(page, { type: 'agent_end', payload: { messages: [], stats: null, cost: null } })
          await page.waitForTimeout(300)
          await issues(page, 'fixed')
        },
      },
    ],
  }
}
