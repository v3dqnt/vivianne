//@include company-common.js
TS.useTeamStore.setState({ byRoot: { [CO]: [] }, runs: {} })
A.useAgentStore.setState({ initialized: true, isBusy: true, init: async () => {}, messages: [ASK] })
await new Promise((r) => setTimeout(r, 300))
PS.usePanelStore.getState().openRight('agents')
return 'c2'
