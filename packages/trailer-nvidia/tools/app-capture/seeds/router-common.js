//@include trailer-common.js
A.useAgentStore.setState({ initialized: true, messages: [], isBusy: false, init: async () => {} })
const ov = (id, name) => ({ id, name, provider: 'nebius', api: 'openai-chat', baseUrl: null, maxTokens: 16384, contextWindow: 131072 })
const routing = (auto) => ({
  enabled: true,
  quick: ov('nvidia/nemotron-3-nano', 'Nemotron 3 Nano'),
  auto,
  tiers: auto ? { easy: ov('nvidia/nemotron-3-nano', 'Nemotron 3 Nano'), medium: ov('nvidia/nemotron-3-super', 'Nemotron 3 Super'), hard: ov('nvidia/nemotron-3-ultra-550b', 'Nemotron 3 Ultra 550B') } : {},
})
