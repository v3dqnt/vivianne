//@include trailer-common.js
// Memory: the Orders API thread, with the Memory panel open on the right.
const PS = await mod('/src/modules/dock/store/panelStore.ts')
const NOW = Date.now()
const d = (days) => NOW - days * 86400000
const mem = (id, text, kind, days, used, userWide = false) => ({ id, key: id, text, kind, createdAt: d(days), confidence: 0.9, accessCount: used, lastAccessedAt: used ? d(days / 4) : 0, userWide })
window.__STUB = {
  ...(window.__STUB || {}),
  cmd_memory_list: [
    mem('u1', 'Prefers small PRs, with a test for every fix.', 'preference', 21, 14, true),
    mem('u2', 'Never deploys after 18:00 IST.', 'instruction', 30, 6, true),
    mem('p1', 'Orders live in Postgres 15; order_id is a bigint.', 'fact', 12, 9),
    mem('p2', 'Backfills run in batches of 5,000 to keep locks short.', 'fact', 2, 3),
    mem('p3', 'Refund webhooks retry 3 times, then page on-call.', 'fact', 4, 2),
    mem('p4', 'Planned the orders_v2 migration; the dry run passed.', 'episodic', 1, 1),
  ],
}
const T1 = 60000
A.useAgentStore.setState({ initialized: true, isBusy: false, init: async () => {}, messages: [
  user('Plan the database migration for orders', T1),
  say('Backfill in batches of 5,000, as before, so locks stay short. Reads switch behind a flag; rollback is one flip.', T1 + 30000),
] })
await new Promise((r) => setTimeout(r, 300))
PS.usePanelStore.getState().openRight('memory')
await new Promise((r) => setTimeout(r, 600))
return 'm5'
