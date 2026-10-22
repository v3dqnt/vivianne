const P = await mod('/src/modules/sessions/store/projectsStore.ts')
const A = await mod('/src/modules/ai/store/agentStore.ts')
const O = await mod('/src/modules/settings/store/optionalComponentsStore.ts')
const PL = await mod('/src/modules/plan/store/planStore.ts')
const L = await mod('/src/modules/workspace/layout.ts')
O.useOptionalComponentsStore.setState({ prompted: true, init: async () => {} })
const T0 = Date.UTC(2026, 9, 5, 3, 30) // 9:00 IST
P.useProjectsStore.setState({
  projects: [
    { id: 'p1', name: 'Storefront', created_at: '2026-09-01', path: 'C:/dev/storefront', color: '#4f8fe0', multiplayer: null, mode: 'threads' },
    { id: 'p2', name: 'Docs site', created_at: '2026-09-01', path: 'C:/dev/docs', color: '#8a8a8a', multiplayer: null, mode: 'threads' },
  ],
  activeProjectId: 'p1',
  refresh: async () => {},
})
const STEPS = ['Reproduce the failed payment', 'Trace it through cart → checkout', 'Fix it, with a regression test', 'Open the pull request']
let n = 0
const id = () => 'c' + ++n
const user = (text, t) => ({ type: 'user', content: [{ type: 'text', text }], timestamp: T0 + t })
const say = (text, t) => ({ type: 'assistant', content: [{ type: 'text', text }], stopReason: 'stop', errorMessage: null, timestamp: T0 + t })
// One assistant turn of tool calls followed by their results.
const tools = (calls, t) => {
  const out = [{ type: 'assistant', content: calls.map(([name, args]) => ({ type: 'toolCall', id: id(), name, arguments: args })), stopReason: 'toolUse', errorMessage: null, timestamp: T0 + t }]
  out[0].content.forEach((c, i) => {
    const [, args, result, details] = calls[i]
    out.push({ type: 'toolResult', toolCallId: c.id, toolName: c.name, content: [{ type: 'text', text: result || 'ok' }], details: details ?? null, isError: false, timestamp: T0 + t + 500, arguments: args })
  })
  return out
}
const todos = (states, summaries = {}) => ({ todos: STEPS.map((content, i) => ({ content, status: states[i], ...(summaries[i] ? { summary: summaries[i] } : {}) })) })
const plan = {
  id: 'plan-1', version: 1, title: 'Fix checkout',
  summary: 'Payments fail at checkout when a coupon is applied. Reproduce it, find the cause, fix it with a test, and open a PR.',
  sections: [
    { id: 's-approach', heading: 'Approach', body: 'Reproduce with the failing coupon first, so the fix is aimed at a located line rather than a symptom. The cart total is recomputed in two places; the suspicion is that checkout reads a stale one.' },
  ],
  steps: STEPS.map((title, i) => ({ id: 'st' + (i + 1), title, detail: ['Run the checkout flow with SPRING20 and capture the 402.', 'Follow the total from cart.ts into checkout/session.ts.', 'Recompute the total in one place; add a test for coupon + retry.', 'Push the branch and open a PR with the repro and the fix.'][i], owner: null, dependsOn: i ? ['st' + i] : [] })),
  changes: null, createdAt: T0 + 60000,
}
const planState = (status, stepStates) => ({
  plan, versions: [1],
  state: { id: 'plan-1', status, latest: 1, approvedVersion: status === 'review' ? null : 1, updatedAt: T0, steps: Object.fromEntries(stepStates.map((s, i) => ['st' + (i + 1), { owner: null, status: s, taskId: null, output: null, startedAt: null, endedAt: null }])) },
})
const D = await mod('/src/modules/sessions/store/daysStore.ts')
const W = await mod('/src/modules/ai/store/workspaceStore.ts')
const M = await mod('/src/modules/ai/store/modelStore.ts')
D.useDaysStore.setState({
  threads: [
    { id: 't1', name: 'Fix checkout', created_at: '2026-10-05T03:30:00Z', workspace_folders: ['C:/dev/storefront'], access_mode: 'full' },
    { id: 't2', name: 'Coupon edge cases', created_at: '2026-10-03T08:00:00Z', workspace_folders: [], access_mode: null },
    { id: 't3', name: 'Shipping rates API', created_at: '2026-10-01T08:00:00Z', workspace_folders: [], access_mode: null },
  ],
  todayId: 't1',
})
W.useWorkspaceStore.setState({ folders: ['C:/dev/storefront'], accessMode: 'full', branch: 'fix/checkout-total', refresh: async () => {} })
M.useModelStore.setState({ stats: { ...(M.useModelStore.getState().stats || {}), modelName: 'Qwythos 9B', modelId: 'qwythos-9b', provider: 'local' } })
