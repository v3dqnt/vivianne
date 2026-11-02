// Demo data shared by every capture: one project, models, a quiet backend.
;(() => {
  const model = (id, name, provider, ctx) => ({ id, name, provider, api: 'anthropic-messages', contextWindow: ctx, maxOutputTokens: 64000, inputCostPerMillion: 3, outputCostPerMillion: 15, isImageModel: false, isAudioModel: false, supportsVision: true })
  window.__MOCK__ = Object.assign(window.__MOCK__ || {}, {
    cmd_get_settings: { optionalComponentsPrompted: true },
    cmd_get_optional_components_status: { camoufoxInstalled: true, docker: { installed: true, running: true, supportedOs: true } },
    cmd_list_projects: [
      { id: 'p-site', name: 'northwind-site', created_at: '2026-10-01T09:00:00Z', path: '/Users/sam/code/northwind-site', color: 'violet', multiplayer: null, mode: 'threads' },
      { id: 'p-api', name: 'billing-api', created_at: '2026-09-12T09:00:00Z', path: '/Users/sam/code/billing-api', color: 'teal', multiplayer: null, mode: 'threads' },
    ],
    cmd_get_session_project_map: {},
    cmd_list_available_models: [
      model('claude-opus-5-5', 'Claude Opus 5.5', 'anthropic', 1000000),
      model('claude-sonnet-5-5', 'Claude Sonnet 5.5', 'anthropic', 1000000),
      model('gpt-6', 'GPT-6', 'openai', 400000),
      model('gemini-3-pro', 'Gemini 3 Pro', 'google', 1000000),
    ],
    cmd_agent_session_stats: { contextTokens: 18400, contextWindow: 1000000, contextPercent: 1.8, messageCount: 0, inputTokens: 0, outputTokens: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, modelId: 'claude-opus-5-5', modelName: 'Claude Opus 5.5', provider: 'anthropic', maxTokens: 64000, breakdown: { systemPrompt: 9000, toolSchemas: 9400, user: 0, assistant: 0, toolResults: 0 } },
    cmd_app_version: '1.8.2',
  })
})()
