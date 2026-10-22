//@include ui-common.js
// Make it yours: quick access along the bottom, the side panels stacked.
const ROOT = 'C:/dev/orders-api'
const NOW = Date.now()
const mem = (id, text, kind, days, used, userWide = false) => ({ id, key: id, text, kind, createdAt: NOW - days * 86400000, confidence: 0.9, accessCount: used, lastAccessedAt: used ? NOW - (days / 4) * 86400000 : 0, userWide })
window.__STUB = {
  ...(window.__STUB || {}),
  cmd_memory_list: [
    mem('u1', 'Prefers small PRs, with a test for every fix.', 'preference', 21, 14, true),
    mem('p2', 'Backfills run in batches of 5,000 to keep locks short.', 'fact', 2, 3),
    mem('p4', 'Planned the orders_v2 migration; the dry run passed.', 'episodic', 1, 1),
  ],
}
TS.useTeamStore.setState({ byRoot: { [ROOT]: [
  agent('db', 'Database', 'Schema changes and backfills', 'blue'),
  agent('qa', 'QA', 'Runs the suites on staging', 'green'),
] } })
await new Promise((r) => setTimeout(r, 300))
L.closePanel('files')
L.closePanel('preview')
await new Promise((r) => setTimeout(r, 150))
L.openPanel('quick')
await new Promise((r) => setTimeout(r, 200))
L.openPanel('agents')
await new Promise((r) => setTimeout(r, 200))
L.openPanel('memory')
await new Promise((r) => setTimeout(r, 200))
L.stackZone()
await new Promise((r) => setTimeout(r, 800))
return 'u2'
