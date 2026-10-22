//@include modes-common.js
// Threads: the Orders API project, a conversation under way.
const T1 = 60000
A.useAgentStore.setState({ initialized: true, isBusy: false, init: async () => {}, messages: [
  user('Plan the database migration for orders', T1),
  ...tools([['read', { path: 'db/schema.sql' }], ['grep', { pattern: 'order_id', path: 'src' }]], T1 + 8000),
  say('Three steps: add orders_v2 beside the old table, backfill in batches of 5,000, then switch reads behind a flag. Rollback is one flag flip. Want me to start with the dry run?', T1 + 30000),
] })
return 'w1'
