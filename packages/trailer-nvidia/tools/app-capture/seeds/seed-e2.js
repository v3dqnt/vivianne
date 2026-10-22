//@include trailer-common.js
A.useAgentStore.setState({ initialized: true, messages: [], isBusy: false, init: async () => {} })
await new Promise((r) => setTimeout(r, 400))
const btn = [...document.querySelectorAll('button')].find((b) => /Auto \(cost-aware router\)/.test(b.textContent || ''))
if (btn) btn.click()
await new Promise((r) => setTimeout(r, 700))
return btn ? 'e2: picker open' : 'e2: picker button not found'
