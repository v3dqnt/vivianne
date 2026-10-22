//@include trailer-common.js
// Acts in your world: the agent signs in to the team's dashboard in the
// user's own browser, with a password from the vault.
const T1 = 60000
const base = [
  user('Check the orders dashboard for failed refunds since Monday', T1),
  ...tools([
    ['user_browser', { action: 'open', url: 'https://dash.ordersapi.dev/login' }, 'Opened dash.ordersapi.dev/login'],
    ['user_browser', { action: 'snapshot' }, 'Sign in form: email [e4], password [e7], Sign in [e9]'],
  ], T1 + 6000),
]
