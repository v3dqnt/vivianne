//@include acts-common.js
A.useAgentStore.setState({ initialized: true, isBusy: true, init: async () => {}, pendingPermission: null, messages: [
  ...base,
  ...tools([
    ['vault', { action: 'fill', secret: 'Orders dashboard', ref: 'e7' }, 'Filled the password field (e7) on dash.ordersapi.dev. You did not see the value, and neither did the page’s scripts.'],
    ['user_browser', { action: 'click', ref: 'e9' }, 'Clicked Sign in'],
  ], T1 + 20000),
] })
return 'v2'
