//@include trailer-common.js
// The three ways to work, in the trailer's world: Orders API (threads),
// Storefront (an agent team in a room), Vivianne (a company under a lead).
const TS = await mod('/src/modules/team/store/teamStore.ts')
const BS = await mod('/src/modules/team/store/botsStore.ts')
const MS = await mod('/src/modules/sessions/store/masterStore.ts')
const TR = await mod('/src/modules/team/store/teamRunStore.ts')
const PS = await mod('/src/modules/dock/store/panelStore.ts')
const CREW = 'C:/dev/storefront'
const CO = 'C:/dev/vivianne'
TS.useTeamStore.setState({ load: async () => {}, loadRuns: async () => [] })
BS.useBotsStore.setState({ loadRoom: async () => {}, loadDm: async () => {}, loadStatus: async () => {}, init: () => {} })
MS.useMasterStore.setState({ load: async () => null })
TR.useTeamRunStore.setState({ fetch: async () => {} })
const agent = (id, name, role, color, extra = {}) => ({
  id, name, role, status: 'active', reportsTo: null, model: null, allowedTools: null, owns: [], color,
  created: '2026-10-01', proposedBy: 'user', reason: null, instructions: '', feedback: [], path: '', ...extra,
})
const H = (h, m) => Date.UTC(2026, 9, 5, h - 5, m - 30)
