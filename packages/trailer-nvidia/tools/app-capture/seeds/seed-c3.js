//@include company-common.js
TS.useTeamStore.setState({ byRoot: { [CO]: HIRES }, runs: {} })
A.useAgentStore.setState({ initialized: true, isBusy: false, init: async () => {}, messages: [
  ASK,
  ...tools(HIRES.map((h) => ['hire_agent', { name: h.name, role: h.role, reason: h.reason }, 'Proposed ' + h.name + ' (waiting for approval)']), T1 + 20000),
  say('I’d hire three people for this: a writer for the docs and notes, a designer for the site and trailer, and a tester for the builds. Approve them and I’ll split the work.', T1 + 40000),
] })
await new Promise((r) => setTimeout(r, 300))
PS.usePanelStore.getState().openRight('agents')
return 'c3'
