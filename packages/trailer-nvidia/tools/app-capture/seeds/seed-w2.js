//@include modes-common.js
// An agent team: three bots in the Storefront room.
const crew = [
  agent('rev', 'Reviewer', 'Reviews every PR', 'blue'),
  agent('qa', 'QA', 'Runs the suites on staging', 'green'),
  agent('rel', 'Release', 'Merges and ships', 'amber'),
]
TS.useTeamStore.setState({ byRoot: { [CREW]: crew } })
const k = (id) => CREW + '::' + id
const m = (id, from, text, h, mi, kind = 'message', mentions = []) => ({ id, from, text, ts: H(h, mi), kind, mentions })
BS.useBotsStore.setState({
  room: { [CREW]: [
    m('1', 'user', 'Heading out. @everyone get the refunds PR merged tonight.', 18, 40, 'message', ['everyone']),
    m('2', 'rev', 'Reviewed #88. One nit, fixed it. Approved.', 18, 52),
    m('3', 'qa', 'Full e2e on staging: 132 passed, refunds included.', 19, 31),
    m('4', 'rel', 'Merging after the nightly build at 02:00.', 19, 33, 'routine'),
  ] },
  status: {
    [k('rev')]: { root: CREW, botId: 'rev', state: 'working', activity: 'reviewing #88' },
    [k('qa')]: { root: CREW, botId: 'qa', state: 'working', activity: 'running e2e on staging' },
    [k('rel')]: { root: CREW, botId: 'rel', state: 'working', activity: 'waiting for the nightly build' },
  },
  channel: { [CREW]: 'room' },
  unread: { [k('rev')]: 1, [k('qa')]: 2 },
})
window.__STUB = {
  ...(window.__STUB || {}),
  cmd_bots_settings: { rules: { files: 'auto', commands: 'auto', computer: 'ask', web: 'auto', outside: 'hand_off' }, paused: false, alwaysOn: true, checkinHours: 4, lastCheckin: null },
  cmd_bots_routines: [
    { id: 'n1', title: 'Nightly merge', prompt: 'Merge approved PRs after the nightly build passes.', cron: '0 2 * * *', enabled: true, nextRun: H(26, 0), runs: [{ at: H(2, 0), ok: true, summary: 'Merged #84 and #86' }] },
  ],
  cmd_bots_memory: '',
}
TS.useTeamStore.setState({ focused: { root: CREW, agentId: 'rel' } })
W.useWorkspaceStore.setState({ folders: [CREW], branch: 'main' })
P.useProjectsStore.setState({ activeProjectId: 'p2' })
await new Promise((r) => setTimeout(r, 300))
PS.usePanelStore.getState().openRight('agents')
return 'w2'
