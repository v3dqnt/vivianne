import type { ApprovalInput } from './widgets.ts'

/** Where a card stands: waiting on you, or how it ended. */
export type WidgetState = 'pending' | 'approved' | 'denied' | 'expired'

export interface LabCard {
  /** What this card is here to show. */
  label: string
  state: WidgetState
  approval: ApprovalInput
}

/**
 * The Widget Lab's gallery: one card of every kind, in every state, and the
 * edge cases (a missing detail, a very long value, a stated reason, a tool
 * it has no card for), so how each looks can be checked in one place.
 */
export const WIDGET_LAB: LabCard[] = [
  { label: 'Payment with a saved card', state: 'pending', approval: { toolName: 'vault', args: { action: 'fill', secret: 'Visa ending 4242', field: 'number', ref: 12 }, reason: null } },
  { label: 'Payment: security code', state: 'approved', approval: { toolName: 'vault', args: { action: 'fill', secret: 'Amex ending 1005', field: 'cvv', ref: 14 }, reason: null } },
  { label: 'Email', state: 'pending', approval: { toolName: 'email', args: { action: 'send', to: 'priya.raman@northwind.co', subject: 'Thursday’s review', text: 'Hi Priya,\n\nThe deck is attached. Could we move the review to 4pm?\n\nThanks' }, reason: null } },
  { label: 'Email with no subject', state: 'denied', approval: { toolName: 'email', args: { action: 'send', to: 'tomas@studio-larsen.dk', text: 'Running ten minutes late.' }, reason: null } },
  { label: 'Text message', state: 'pending', approval: { toolName: 'phone', args: { action: 'send_sms', to: '+1 (312) 847-1928', text: 'Your order is ready for pickup.' }, reason: null } },
  { label: 'Phone call', state: 'expired', approval: { toolName: 'phone', args: { action: 'call', to: '+44 20 7946 0532' }, reason: null } },
  { label: 'Shell command', state: 'pending', approval: { toolName: 'bash', args: { command: 'npm install --save-dev vitest' }, reason: "Lab's rules say to ask before this" } },
  { label: 'Very long command (clipped)', state: 'pending', approval: { toolName: 'powershell', args: { command: `Get-ChildItem -Recurse C:\Users\me\Projects | Where-Object { $_.Length -gt 10MB } | ${'Select-Object Name, Length, LastWriteTime | '.repeat(12)}Export-Csv big-files.csv` }, reason: null } },
  { label: 'Change a file', state: 'approved', approval: { toolName: 'write', args: { path: 'C:\Users\me\Documents\Groceries.md', content: '- oat milk\n- eggs' }, reason: null } },
  { label: 'Type in the browser', state: 'pending', approval: { toolName: 'user_browser_driver', args: { action: 'type', ref: 31, text: 'Flights from Lisbon to Oslo, 14 Nov' }, reason: null } },
  { label: 'Open a page', state: 'denied', approval: { toolName: 'user_browser_driver', args: { action: 'open', url: 'https://accounts.example-bank.com/transfer' }, reason: null } },
  { label: 'Use the screen', state: 'pending', approval: { toolName: 'dip', args: { action: 'left_click', target: 'Send button in Outlook' }, reason: null } },
  { label: 'A connected app (MCP)', state: 'pending', approval: { toolName: 'mcp__notion__create_page', args: { title: 'Weekly notes', parent: 'Team space', archived: false }, reason: null } },
  { label: 'A tool with no details', state: 'expired', approval: { toolName: 'nebius_job', args: {}, reason: null } },
]
