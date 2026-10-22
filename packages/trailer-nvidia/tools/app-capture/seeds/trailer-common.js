//@include common.js
// The trailer's world: an Orders API project, on the NVIDIA edition's Nebius models.
P.useProjectsStore.setState({
  projects: [
    { id: 'p1', name: 'Orders API', created_at: '2026-10-01', path: 'C:/dev/orders-api', color: '#76b900', multiplayer: null, mode: 'threads' },
    { id: 'p2', name: 'Storefront', created_at: '2026-09-01', path: 'C:/dev/storefront', color: '#4f8fe0', multiplayer: null, mode: 'team' },
    { id: 'p3', name: 'Vivianne', created_at: '2026-08-01', path: 'C:/dev/vivianne', color: '#8a8a8a', multiplayer: null, mode: 'company' },
  ],
  activeProjectId: 'p1',
  refresh: async () => {},
})
D.useDaysStore.setState({
  threads: [
    { id: 't1', name: 'Database migration', created_at: '2026-10-20T08:00:00Z', workspace_folders: ['C:/dev/orders-api'], access_mode: 'full' },
    { id: 't2', name: 'Refund webhooks', created_at: '2026-10-18T08:00:00Z', workspace_folders: [], access_mode: null },
    { id: 't3', name: 'Rate limits', created_at: '2026-10-15T08:00:00Z', workspace_folders: [], access_mode: null },
  ],
  todayId: 't1',
})
W.useWorkspaceStore.setState({ folders: ['C:/dev/orders-api'], accessMode: 'full', branch: 'feat/orders-v2', refresh: async () => {} })
const nm = (id, name) => ({ id, name, provider: 'nebius', api: 'openai-chat', contextWindow: 131072, maxOutputTokens: 16384, inputCostPerMillion: null, outputCostPerMillion: null, isImageModel: false, isAudioModel: false, supportsVision: false })
M.useModelStore.setState({
  initialized: true,
  init: async () => {},
  models: [
    nm('nvidia/nemotron-3-ultra-550b', 'Nemotron 3 Ultra 550B'),
    nm('nvidia/nemotron-3-super', 'Nemotron 3 Super'),
    nm('nvidia/nemotron-3-nano', 'Nemotron 3 Nano'),
  ],
  stats: { ...(M.useModelStore.getState().stats || {}), modelName: 'Auto (cost-aware router)', modelId: 'viv-auto', provider: 'router' },
})
