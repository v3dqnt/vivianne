//@include company-common.js
TS.useTeamStore.setState({ byRoot: { [CO]: [] }, runs: {} })
A.useAgentStore.setState({ initialized: true, isBusy: false, init: async () => {}, messages: [] })
await new Promise((r) => setTimeout(r, 300))
PS.usePanelStore.getState().openRight('agents')
return 'c1'
