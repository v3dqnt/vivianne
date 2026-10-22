//@include modes-common.js
// It builds its own interface: a report answered as a live summary card.
const CARD = {
  title: 'Orders migration', icon: '🗄️', status: { label: 'On track', tone: 'success' },
  blocks: [
    { type: 'stats', items: [{ label: 'Rows backfilled', value: 1240000, format: 'integer' }, { label: 'Batches', value: 248, format: 'integer' }, { label: 'Errors', value: 0, format: 'integer' }] },
    { type: 'steps', items: [
      { title: 'Add orders_v2 beside the old table', state: 'done' },
      { title: 'Backfill in batches of 5,000', state: 'done' },
      { title: 'Switch reads behind a flag', state: 'active' },
      { title: 'Drop the old table', state: 'todo' },
    ] },
    { type: 'callout', tone: 'success', text: 'Rollback is one flag flip.' },
    { type: 'buttons', items: [{ label: 'Switch reads now', send: 'Switch reads to orders_v2', primary: true }, { label: 'Show the dry run', send: 'Show the dry run' }] },
  ],
}
const T1 = 60000
A.useAgentStore.setState({ initialized: true, isBusy: false, init: async () => {}, messages: [
  user('How is the orders migration going?', T1),
  ...tools([['read', { path: 'logs/backfill.log' }], ['bash', { command: 'psql -c "select count(*) from orders_v2"' }, '1240000']], T1 + 6000),
  say('Backfill is done and clean; reads switch next.\n\n```ui\n' + JSON.stringify(CARD) + '\n```', T1 + 20000),
] })
