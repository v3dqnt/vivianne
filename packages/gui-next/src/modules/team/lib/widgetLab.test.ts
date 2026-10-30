import { test } from 'node:test'
import assert from 'node:assert/strict'
import { WIDGET_LAB } from './widgetLab.ts'
import { describeApproval, WIDGET_KINDS } from './widgets.ts'

const cards = WIDGET_LAB.map((f) => ({ ...f, widget: describeApproval(f.approval, 'Lab') }))

test('the lab shows every kind of card', () => {
  const kinds = new Set(cards.map((c) => c.widget.kind))
  for (const k of WIDGET_KINDS) assert.ok(kinds.has(k), `no ${k} card in the lab`)
})

test('the lab shows every state a card can be in', () => {
  const states = new Set(cards.map((c) => c.state))
  for (const s of ['pending', 'approved', 'denied', 'expired'] as const) assert.ok(states.has(s), `no ${s} card in the lab`)
})

test('the lab covers the edge cases: a missing detail, a very long value, a stated reason', () => {
  assert.ok(cards.some((c) => c.widget.kind === 'email' && !c.widget.fields.some((f) => f.label === 'Subject')), 'an email with no subject')
  assert.ok(cards.some((c) => c.widget.fields.some((f) => f.value.includes('…'))), 'a value long enough to be clipped')
  assert.ok(cards.some((c) => c.widget.note), 'a card with the agent’s reason')
})

test('every lab card has a label saying what it shows', () => {
  for (const c of cards) assert.ok(c.label.trim().length > 0)
})
