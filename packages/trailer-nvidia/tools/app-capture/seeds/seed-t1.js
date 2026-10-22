//@include modes-common.js
// Threads: a long sidebar of them, one per task.
const NAMES = ['Database migration', 'Refund webhooks', 'Rate limits', 'Retry on 402', 'Coupon totals', 'Orders v2 schema', 'Backfill script', 'Index for refunds', 'Flaky e2e on CI', 'Stripe test keys', 'Ledger export', 'Nightly report', 'Dead-letter queue', 'Timeouts in checkout', 'Audit log', 'Read replicas', 'Feature flags', 'Cache warmup', 'Webhook signatures', 'Partial refunds', 'Currency rounding', 'Load test', 'Alert thresholds', 'API versioning', 'Pagination cursor', 'Idempotency keys', 'Customer emails', 'Tax lines', 'Shipping estimates', 'Inventory sync', 'Fraud rules', 'Admin search', 'Bulk cancel', 'Order notes', 'Invoice PDFs', 'Status page', 'On-call runbook', 'Error budget', 'Docs for v2', 'Release 2.4']
D.useDaysStore.setState({
  threads: NAMES.map((name, i) => ({ id: 't' + i, name, created_at: new Date(Date.UTC(2026, 9, 21) - i * 86400000 * 1.6).toISOString(), workspace_folders: [], access_mode: null })),
  todayId: 't0',
})
const T1 = 60000
A.useAgentStore.setState({ initialized: true, isBusy: false, init: async () => {}, messages: [
  user('Plan the database migration for orders', T1),
  say('Three steps: add orders_v2 beside the old table, backfill in batches of 5,000, then switch reads behind a flag. Rollback is one flag flip.', T1 + 30000),
] })
return 't1'
