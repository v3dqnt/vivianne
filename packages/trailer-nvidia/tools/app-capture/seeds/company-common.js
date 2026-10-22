//@include modes-common.js
// A company under one lead: the user briefs Mira, who proposes hires.
MS.useMasterStore.setState({ byRoot: { [CO]: { name: 'Mira', charter: 'Ship the 1.9 release.', charterPath: '', created: '2026-10-01' } } })
D.useDaysStore.setState({ threads: [{ id: 'm1', name: 'Release 1.9', created_at: '2026-10-05T13:10:00Z', workspace_folders: [CO], access_mode: 'full' }], todayId: 'm1' })
W.useWorkspaceStore.setState({ folders: [CO], branch: 'main' })
P.useProjectsStore.setState({ activeProjectId: 'p3' })
const HIRES = [
  agent('docs', 'Writer', 'Docs and release notes', 'pink', { status: 'proposed', reportsTo: 'master', proposedBy: 'master', reason: 'Every release needs notes and updated docs.', owns: ['docs/'] }),
  agent('design', 'Designer', 'The site and the trailer', 'purple', { status: 'proposed', reportsTo: 'master', proposedBy: 'master', reason: 'The launch page and trailer change with 1.9.', owns: ['site/'] }),
  agent('qa', 'Tester', 'Builds on every platform', 'teal', { status: 'proposed', reportsTo: 'master', proposedBy: 'master', reason: 'Windows, macOS and Linux must all pass first.', owns: ['tests/'] }),
]
const T1 = 60000
const ASK = user('Ship 1.9 by Friday. You run it.', T1)
