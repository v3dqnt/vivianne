import { test } from 'node:test'
import assert from 'node:assert/strict'
import { composerButton } from './messenger.ts'

test('working with nothing typed: the button stops it', () => {
  assert.equal(composerButton({ chatBusy: false, agentState: 'working', typed: '' }), 'stop')
  assert.equal(composerButton({ chatBusy: true, agentState: 'idle', typed: '  ' }), 'stop')
})

test('a run started by a routine or another agent can be stopped too', () => {
  // The chat's own store never saw that run start; the agent's status did.
  assert.equal(composerButton({ chatBusy: false, agentState: 'working', typed: '' }), 'stop')
})

test('with something typed it sends, working or not', () => {
  assert.equal(composerButton({ chatBusy: true, agentState: 'working', typed: 'also check the inbox' }), 'send')
  assert.equal(composerButton({ chatBusy: false, agentState: 'idle', typed: 'hi' }), 'send')
})

test('idle with nothing typed: send, disabled', () => {
  assert.equal(composerButton({ chatBusy: false, agentState: 'idle', typed: '' }), 'send-disabled')
  assert.equal(composerButton({ chatBusy: false, agentState: 'paused', typed: '' }), 'send-disabled')
})

import { toItems } from './messenger.ts'
import type { AgentMessage } from '../../ai/lib/types.ts'

const DAY = new Date('2026-10-30T10:00:00').getTime()
const user = (text: string, ts = DAY): AgentMessage => ({ type: 'user', content: [{ type: 'text', text }], timestamp: ts })
const reply = (text: string, ts = DAY): AgentMessage => ({
  type: 'assistant',
  content: [{ type: 'thinking', thinking: 'let me think' }, { type: 'text', text }],
  stopReason: 'stop',
  errorMessage: null,
  timestamp: ts,
})
const bubbles = (items: ReturnType<typeof toItems>) => items.filter((i) => i.kind === 'bubble').map((i) => (i.kind === 'bubble' ? `${i.from}${i.by ? `:${i.by}` : ''}: ${i.text}` : ''))

test('what each side said becomes a bubble, reasoning left out', () => {
  assert.deepEqual(bubbles(toItems([user('hi'), reply('hello!')])), ['you: hi', 'agent: hello!'])
})

test('a mention from another chat shows where it came from, then what you said', () => {
  const relayed = "The user mentioned you in Ada's chat:\n\nhey @bo can you check the calendar\n\nYour answer goes into that chat, to the user and Ada."
  const items = toItems([user(relayed)])
  const shown = items.filter((i) => i.kind !== 'day').map((i) => (i.kind === 'note' ? `note: ${i.text}` : i.kind === 'bubble' ? `${i.from}: ${i.text}` : i.kind))
  assert.deepEqual(shown, ["note: In Ada's chat", 'you: hey @bo can you check the calendar'])
})

test("another agent's answer is its bubble, with who said it", () => {
  const answer = '<agent_message from="bo" name="Bo">\nThe calendar is free at 3.\n</agent_message>\n\nBo answered. Use it for what you are doing.'
  assert.deepEqual(bubbles(toItems([user(answer)])), ['agent:bo: The calendar is free at 3.'])
})

test('a failed turn is left out (the errors icon lists it)', () => {
  const failed: AgentMessage = { type: 'assistant', content: [{ type: 'text', text: 'Error: 429' }], stopReason: 'error', errorMessage: '429', timestamp: DAY }
  assert.deepEqual(bubbles(toItems([user('hi'), failed])), ['you: hi'])
})

test('tool work between two messages folds into one activity line', () => {
  const tool = (id: string): AgentMessage => ({ type: 'toolResult', toolCallId: id, toolName: 'web_search', content: [], details: null, isError: false, timestamp: DAY })
  const kinds = toItems([user('find it'), tool('a'), tool('b'), reply('found it')]).map((i) => i.kind).filter((k) => k !== 'day')
  assert.deepEqual(kinds, ['bubble', 'activity', 'bubble'])
})

test("the app's own notes never show as your bubble", () => {
  const items = toItems([user('<compacted_history>\nGoal: ...\n</compacted_history>'), user('<system-reminder>\nbe brief\n</system-reminder>')])
  assert.deepEqual(bubbles(items), [])
  assert.ok(items.some((i) => i.kind === 'note' && i.text === 'Earlier messages were summarised'))
})

test("a check-in or a morning brief shows as a quiet note, not as something you said", () => {
  const items = toItems([user('Check-in. Look at what has changed since your last check'), reply('All quiet.'), user('Morning brief. Greet the user')])
  assert.deepEqual(
    items.filter((i) => i.kind !== 'day').map((i) => (i.kind === 'note' ? `note:${i.text}` : i.kind)),
    ['note:Check-in', 'bubble', 'note:Morning brief'],
  )
})

test('a reply ending in SUGGEST: becomes its message, then a suggestion card', () => {
  const items = toItems([reply('Your flight moved to 9am.\nSUGGEST: Move the airport pickup to 7:30')]).filter((i) => i.kind !== 'day')
  assert.equal(items.length, 2)
  assert.ok(items[0].kind === 'bubble' && items[0].text === 'Your flight moved to 9am.')
  assert.ok(items[1].kind === 'suggestion' && items[1].text === 'Move the airport pickup to 7:30')
})
