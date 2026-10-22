//@include acts-common.js
A.useAgentStore.setState({ initialized: true, isBusy: true, init: async () => {}, messages: base,
  pendingPermission: { requestId: 'q1', toolCallId: 'v1', toolName: 'vault', args: { action: 'fill', secret: 'Orders dashboard', ref: 'e7' },
    reason: "wants to fill your saved 'Orders dashboard' into a field on dash.ordersapi.dev. The page and the model never see the value." } })
await new Promise((r) => setTimeout(r, 400))
return 'v1'
