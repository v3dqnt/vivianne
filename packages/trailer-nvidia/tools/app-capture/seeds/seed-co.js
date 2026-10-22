//@include common.js
//@include projects3.js
const hires = [
  agent('copy', 'Copywriter', 'Sale pages and emails', 'pink', { reportsTo: 'master', proposedBy: 'master' }),
  agent('design', 'Designer', 'Banners and the landing page', 'purple', { reportsTo: 'master', proposedBy: 'master' }),
  agent('analyst', 'Analyst', 'Pricing and the discount model', 'teal', { reportsTo: 'master', proposedBy: 'master' }),
]
MS.useMasterStore.setState({ byRoot: { [LAUNCH]: { name: 'Mira', charter: 'Launch the spring sale.', charterPath: '', created: '2026-09-20' } } })
const run = (id) => ({ id: 'r-' + id, root: LAUNCH, agentId: id, agentName: id, description: '', prompt: '', status: id === 'analyst' ? 'completed' : 'running', startedAt: H(18, 40), endedAt: null, output: null, turns: 3, toolCalls: 9, comments: [] })
TS.useTeamStore.setState({ byRoot: { [LAUNCH]: hires }, runs: Object.fromEntries(hires.map((a) => [LAUNCH + '::' + a.id, [run(a.id)]])) })
TR.useTeamRunStore.setState({
  runs: { tr1: { id: 'tr1', task: 'Spring sale, live Friday', status: 'running', startedAt: H(18, 41), messages: [], members: [
    { id: 'copy', name: 'Copywriter', role: 'Sale pages and emails', color: 'pink', status: 'working', taskId: null, output: null },
    { id: 'design', name: 'Designer', role: 'Banners and the landing page', color: 'purple', status: 'working', taskId: null, output: null },
    { id: 'analyst', name: 'Analyst', role: 'Pricing and the discount model', color: 'teal', status: 'done', taskId: null, output: 'Discounts modelled: 20% sitewide keeps margin above 31%.' },
  ] } }, latest: 'tr1',
})
D.useDaysStore.setState({ threads: [{ id: 'm1', name: 'Spring sale', created_at: '2026-10-05T13:10:00Z', workspace_folders: [LAUNCH], access_mode: 'full' }], todayId: 'm1' })
W.useWorkspaceStore.setState({ folders: [LAUNCH], branch: 'main' })
const T1 = H(18, 38) - T0
A.useAgentStore.setState({ initialized: true, isBusy: true, init: async () => {}, messages: [
  user('Launch the spring sale by Friday. You run it.', T1),
  say('On it. I’ve hired three specialists for this and split the work: copy, design and pricing. I’ll review everything before it ships.', T1 + 40000),
  ...tools([['team_run', { task: 'Spring sale, live Friday' }, 'running', { teamRun: 'tr1' }]], T1 + 60000),
] })
P.useProjectsStore.setState({ activeProjectId: 'p3' })
await new Promise((r) => setTimeout(r, 300))
PS.usePanelStore.getState().openRight('agents')
return 'co'
