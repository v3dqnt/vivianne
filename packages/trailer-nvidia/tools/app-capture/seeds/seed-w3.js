//@include modes-common.js
// A whole company: a lead who hires, under one charter.
const hires = [
  agent('docs', 'Writer', 'Docs and release notes', 'pink', { reportsTo: 'master', proposedBy: 'master' }),
  agent('design', 'Designer', 'The site and the trailer', 'purple', { reportsTo: 'master', proposedBy: 'master' }),
  agent('qa', 'Tester', 'Builds on every platform', 'teal', { reportsTo: 'master', proposedBy: 'master' }),
]
MS.useMasterStore.setState({ byRoot: { [CO]: { name: 'Mira', charter: 'Ship the 1.9 release.', charterPath: '', created: '2026-10-01' } } })
const run = (id) => ({ id: 'r-' + id, root: CO, agentId: id, agentName: id, description: '', prompt: '', status: id === 'qa' ? 'completed' : 'running', startedAt: H(18, 40), endedAt: null, output: null, turns: 3, toolCalls: 9, comments: [] })
TS.useTeamStore.setState({ byRoot: { [CO]: hires }, runs: Object.fromEntries(hires.map((a) => [CO + '::' + a.id, [run(a.id)]])) })
TR.useTeamRunStore.setState({
  runs: { tr1: { id: 'tr1', task: 'Ship 1.9 by Friday', status: 'running', startedAt: H(18, 41), messages: [], members: [
    { id: 'docs', name: 'Writer', role: 'Docs and release notes', color: 'pink', status: 'working', taskId: null, output: null },
    { id: 'design', name: 'Designer', role: 'The site and the trailer', color: 'purple', status: 'working', taskId: null, output: null },
    { id: 'qa', name: 'Tester', role: 'Builds on every platform', color: 'teal', status: 'done', taskId: null, output: 'Windows, macOS and Linux builds pass.' },
  ] } }, latest: 'tr1',
})
D.useDaysStore.setState({ threads: [{ id: 'm1', name: 'Release 1.9', created_at: '2026-10-05T13:10:00Z', workspace_folders: [CO], access_mode: 'full' }], todayId: 'm1' })
W.useWorkspaceStore.setState({ folders: [CO], branch: 'main' })
const T1 = 60000
A.useAgentStore.setState({ initialized: true, isBusy: true, init: async () => {}, messages: [
  user('Ship 1.9 by Friday. You run it.', T1),
  say('On it. I’ve hired three specialists and split the work: docs, design and testing. I’ll review everything before it ships.', T1 + 40000),
  ...tools([['team_run', { task: 'Ship 1.9 by Friday' }, 'running', { teamRun: 'tr1' }]], T1 + 60000),
] })
P.useProjectsStore.setState({ activeProjectId: 'p3' })
await new Promise((r) => setTimeout(r, 300))
PS.usePanelStore.getState().openRight('agents')
return 'w3'
