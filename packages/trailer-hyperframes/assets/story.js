// Every line of invented demo copy in the trailer. Edit words here; scenes
// read window.STORY and never hard-code copy.
window.STORY = {
  user: 'Sam',
  agent: { name: 'Ava', role: 'Chief of staff' },
  greeting: 'Morning Ava, what did I miss?',
  briefing: {
    intro: 'Morning, Sam. Here’s what came in overnight.',
    email: '3 emails need you: the Northwind contract is ready to sign, Priya moved Thursday’s review to 2pm, and your flight to Austin is confirmed.',
    slack: 'In Slack, #launch wants a go/no-go by noon, and Leo asked you to look at the checkout bug on the site.',
  },
  whatsapp: { from: 'Maya ❤️', text: 'Can you grab groceries on the way home? We’re out of basically everything 😅' },
  groceriesIntro: 'Maya asked for groceries. I made you a list from what you usually buy:',
  groceries: ['Oat milk', 'Eggs (dozen)', 'Sourdough bread', 'Spinach', 'Coffee beans'],
  issue: {
    number: 142,
    title: 'Checkout button does nothing on mobile Safari',
    body: 'Tapping “Pay now” on iOS Safari does nothing. Works on Chrome. Reported by 14 customers since Friday.',
    labels: ['bug', 'checkout', 'p1'],
  },
  otherIssues: [
    { number: 139, title: 'Footer links overlap on small screens', labels: ['ui'] },
    { number: 137, title: 'Add dark mode to the pricing page', labels: ['enhancement'] },
  ],
  prompt: 'Fix #142 — checkout is broken on mobile Safari. Find the cause, fix it, and add a test.',
  model: 'Claude Opus 5.5',
  subagents: [
    { name: 'Explorer', task: 'Trace the checkout click handler', steps: ['Read src/checkout/PayButton.tsx', 'Read src/checkout/usePayment.ts', 'Searched for "pointerdown"'] },
    { name: 'Reproducer', task: 'Reproduce on WebKit', steps: ['Ran playwright --browser webkit', 'Captured failing tap on #pay-now'] },
    { name: 'Test writer', task: 'Write a regression test', steps: ['Created tests/checkout.webkit.spec.ts'] },
  ],
  fixSummary: 'Fixed #142. Safari never fired the button’s click because an invisible overlay sat above it on touch devices. I removed the overlay’s pointer capture, added a WebKit regression test, and all 48 tests pass.',
  elapsed: '3m 12s',
  invite: { url: 'https://viv.sh/join/northwind-7Kq2', friendName: 'Leo' },
  collabPrompts: [
    { who: 'Sam', text: 'Leo, can you take the footer overlap (#139)? I’ll do dark mode.' },
    { who: 'Leo', text: 'On it. @Vivianne fix the footer links overlapping on small screens.' },
    { who: 'Sam', text: '@Vivianne add dark mode to the pricing page, match the site theme.' },
  ],
}
