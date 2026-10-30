import { test } from 'node:test'
import assert from 'node:assert/strict'
import { describeApproval } from './widgets.ts'

test('a shell command asks to run that command', () => {
  const w = describeApproval({ toolName: 'bash', args: { command: 'npm install' }, reason: null }, 'Ada')
  assert.equal(w.kind, 'shell')
  assert.equal(w.title, 'Ada wants to run a command')
  assert.deepEqual(w.fields, [{ label: 'Command', value: 'npm install', mono: true }])
})

test('sending an email shows who it goes to, the subject and the message', () => {
  const w = describeApproval({ toolName: 'email', args: { action: 'send', to: 'sam@acme.io', subject: 'Invoice', text: 'Hi Sam, attached.' }, reason: null }, 'Ada')
  assert.equal(w.kind, 'email')
  assert.equal(w.title, 'Ada wants to send an email')
  assert.deepEqual(w.fields, [
    { label: 'To', value: 'sam@acme.io' },
    { label: 'Subject', value: 'Invoice' },
  ])
  assert.equal(w.body, 'Hi Sam, attached.')
})

test('filling a card is a payment, high risk, naming the card and the part filled', () => {
  const w = describeApproval({ toolName: 'vault', args: { action: 'fill', secret: 'Visa ending 4242', field: 'number', ref: 12 }, reason: null }, 'Ada')
  assert.equal(w.kind, 'payment')
  assert.equal(w.title, 'Ada wants to pay with your card')
  assert.equal(w.danger, 'high')
  assert.deepEqual(w.fields, [
    { label: 'Card', value: 'Visa ending 4242' },
    { label: 'Fills', value: 'card number' },
  ])
})

test('details the agent left out are not shown as blank rows', () => {
  const w = describeApproval({ toolName: 'email', args: { action: 'send', to: 'sam@acme.io' }, reason: null }, 'Ada')
  assert.deepEqual(w.fields, [{ label: 'To', value: 'sam@acme.io' }])
  assert.equal(w.body, undefined)
})

test('a very long value is clipped, keeping its start and end', () => {
  const long = 'rm -rf ./build && ' + 'x'.repeat(600) + ' && echo done'
  const w = describeApproval({ toolName: 'bash', args: { command: long }, reason: null }, 'Ada')
  const value = w.fields[0].value
  assert.ok(value.length <= 300, `clipped to 300, got ${value.length}`)
  assert.ok(value.startsWith('rm -rf ./build && '))
  assert.ok(value.endsWith(' && echo done'))
  assert.ok(value.includes('…'))
})

test('a text message shows the number and what it says; a call shows who', () => {
  const sms = describeApproval({ toolName: 'phone', args: { action: 'send_sms', to: '+1 312 847 1928', text: 'Running late' }, reason: null }, 'Ada')
  assert.equal(sms.kind, 'message')
  assert.equal(sms.title, 'Ada wants to send a text')
  assert.deepEqual(sms.fields, [{ label: 'To', value: '+1 312 847 1928' }])
  assert.equal(sms.body, 'Running late')
  const call = describeApproval({ toolName: 'phone', args: { action: 'call', to: '+1 312 847 1928' }, reason: null }, 'Ada')
  assert.equal(call.kind, 'message')
  assert.equal(call.title, 'Ada wants to make a call')
})

test('changing a file names the file', () => {
  const w = describeApproval({ toolName: 'write', args: { path: 'C:/notes/todo.md', content: '- milk' }, reason: null }, 'Ada')
  assert.equal(w.kind, 'file')
  assert.equal(w.title, 'Ada wants to change a file')
  assert.deepEqual(w.fields, [{ label: 'File', value: 'C:/notes/todo.md', mono: true }])
})

test('acting in the browser says what it will do there', () => {
  const w = describeApproval({ toolName: 'user_browser_driver', args: { action: 'type', ref: 4, text: 'hello' }, reason: null }, 'Ada')
  assert.equal(w.kind, 'browser')
  assert.equal(w.title, 'Ada wants to type in your browser')
  assert.deepEqual(w.fields, [{ label: 'Text', value: 'hello' }])
})

test('using the screen says what it will do there', () => {
  const w = describeApproval({ toolName: 'dip', args: { action: 'left_click', target: 'Send button' }, reason: null }, 'Ada')
  assert.equal(w.kind, 'screen')
  assert.equal(w.title, 'Ada wants to use your screen')
  assert.deepEqual(w.fields, [
    { label: 'Action', value: 'left click' },
    { label: 'On', value: 'Send button' },
  ])
})

test('a tool it has no card for still says what and with which details', () => {
  const w = describeApproval({ toolName: 'mcp__notion__create_page', args: { title: 'Weekly notes', parent: 'Team', archived: false, blocks: [1, 2] }, reason: null }, 'Ada')
  assert.equal(w.kind, 'generic')
  assert.equal(w.title, 'Ada wants to use Notion: create page')
  assert.deepEqual(w.fields, [
    { label: 'Title', value: 'Weekly notes' },
    { label: 'Parent', value: 'Team' },
    { label: 'Archived', value: 'false' },
  ])
})

test('the reason the agent gave is shown as the note', () => {
  const w = describeApproval({ toolName: 'bash', args: { command: 'ls' }, reason: "Ada's rules say to ask before this" }, 'Ada')
  assert.equal(w.note, "Ada's rules say to ask before this")
})
