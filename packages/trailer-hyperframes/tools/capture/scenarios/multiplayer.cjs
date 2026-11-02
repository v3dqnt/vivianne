// Scene 7: Sam invites Leo (Work together → Copy invite), Leo's invite comes
// back, the shared project opens with Leo online, and both of them prompt
// Vivianne: Sam in his chat, Leo's agent visibly working in the Multiplayer
// panel. Every frame is the app's real multiplayer UI.
module.exports = (S) => {
  const T0 = Date.parse('2026-11-02T09:30:00Z')
  const LEO = 'leo-7Kq2hTn4pW9xZ3vB8mR1'
  const leoInvite = 'vivianne-invite:' + Buffer.from(JSON.stringify({ v: 1, id: LEO, name: S.invite.friendName })).toString('base64url')
  const [askLeo, leoReply, samPrompt] = S.collabPrompts
  const init = `
    window.__now = ${T0};
    Date.now = () => window.__now;
    window.__tick = (ms) => (window.__now += ms);
    window.__MP = { status: { state: 'disconnected' }, chat: [], peer: [], collab: { mine: [], partner: [], myUsage: {}, partnerUsage: {}, notes: [], lastSync: null, pendingMerge: false } };
    window.__MOCK__ = Object.assign(window.__MOCK__ || {}, {
      cmd_multiplayer_my_id: 'sam-3Fh8Lw2nQd6yTb5kJc0X',
      cmd_multiplayer_get_display_name: ${JSON.stringify(S.user)},
      cmd_multiplayer_status: () => window.__MP.status,
      cmd_multiplayer_peer_messages: () => window.__MP.peer,
      cmd_multiplayer_chat_log: () => window.__MP.chat,
      cmd_multiplayer_collab: () => window.__MP.collab,
      cmd_multiplayer_test_partner_id: null,
      cmd_git_current_branch: 'main',
    });
    window.__agent = (event) => window.__emit('agent_event', { threadId: '', event });
  `
  const chat = (page, fromPeer, text) =>
    page.evaluate(([fromPeer, text]) => {
      window.__emit('multiplayer_chat', { fromPeer, text, timestamp: Date.now() })
    }, [fromPeer, text])
  const tick = (page, s) => page.evaluate((ms) => window.__tick(ms), s * 1000)
  // Messages are stamped in the page, on its demo clock.
  const ev = (page, event) =>
    page.evaluate((e) => {
      if (e.payload && e.payload.message) e.payload.message.timestamp = Date.now()
      window.__agent(e)
    }, event)
  const composer = (page) => page.getByPlaceholder('Do anything')
  const text = (t) => [{ type: 'text', text: t }]

  return {
    init: [init],
    states: [
      { name: 'mp-0', run: async () => {} },
      {
        name: 'mp-new',
        run: async (page) => {
          await page.mouse.click(32, 218) // rail: + New project
          await page.waitForTimeout(400)
        },
      },
      {
        name: 'mp-pair',
        run: async (page) => {
          await page.getByText('Multiplayer project instead').click()
          await page.waitForTimeout(400)
        },
      },
      {
        name: 'mp-copied',
        run: async (page) => {
          await page.getByRole('button', { name: /Copy invite/ }).click()
          await page.waitForTimeout(200)
        },
      },
      {
        name: 'mp-pasted',
        run: async (page) => {
          await page.getByPlaceholder('vivianne-invite:…').fill(leoInvite)
          await page.getByLabel('Project name').fill('northwind-site')
          await page.getByLabel('Folder').fill('/Users/sam/code/northwind-site')
          await page.waitForTimeout(300)
        },
      },
      {
        name: 'mp-live',
        run: async (page) => {
          await page.keyboard.press('Escape')
          await page.evaluate(async ([leo, name]) => {
            window.__MP.status = { state: 'connected', peerId: leo, peerDisplayName: name }
            const ps = await import('/src/modules/sessions/store/projectsStore.ts')
            ps.useProjectsStore.setState((s) => ({ projects: s.projects.map((p) => (p.id === 'p-site' ? { ...p, multiplayer: { peer_id: leo, peer_display_name: name, git_remote: null } } : p)) }))
            window.__emit('multiplayer_status', window.__MP.status)
            const lay = await import('/src/modules/workspace/layout.ts')
            lay.openPanel('multiplayer')
          }, [LEO, S.invite.friendName])
          await page.waitForTimeout(500)
          await tick(page, 20)
          await chat(page, true, 'in 🔥')
          await tick(page, 8)
          await chat(page, false, askLeo.text)
        },
      },
      {
        name: 'mp-typing',
        run: async (page) => {
          await tick(page, 6)
          await chat(page, true, leoReply.text)
          await page.evaluate(() => window.__emit('multiplayer_peer_message', { type: 'user', content: [{ type: 'text', text: 'footer' }], timestamp: Date.now() }))
          await composer(page).click()
          await page.keyboard.type(samPrompt.text, { delay: 0 })
        },
      },
      {
        name: 'mp-both',
        run: async (page) => {
          await tick(page, 4)
          await composer(page).press('Enter')
          await ev(page, { type: 'agent_start' })
          const u = { type: 'user', content: text(samPrompt.text), timestamp: 0 }
          await ev(page, { type: 'message_start', payload: { message: u } })
          await ev(page, { type: 'message_end', payload: { message: u } })
          await tick(page, 2)
          const a = { type: 'assistant', content: text('On it — matching the pricing page to the site theme. Leo is on the footer, so I’ll stay out of `Footer.tsx`.'), stopReason: 'toolUse', errorMessage: null, timestamp: 0 }
          await ev(page, { type: 'message_start', payload: { message: a } })
          await ev(page, { type: 'message_end', payload: { message: a } })
          await ev(page, { type: 'tool_execution_start', payload: { tool_call_id: 'r1', tool_name: 'read', args: { path: 'src/pages/pricing.tsx' } } })
          await page.evaluate(() => window.__emit('multiplayer_peer_message', { type: 'assistant', content: [{ type: 'text', text: 'Fixing the footer overlap' }], stopReason: 'toolUse', errorMessage: null, timestamp: Date.now() }))
          await page.waitForTimeout(300)
        },
      },
    ],
  }
}
