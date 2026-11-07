import { test } from 'node:test'
import assert from 'node:assert/strict'
import { buildTrajectory, isReminder, type TraceEvent } from './trajectoryModel.ts'

const ev = (kind: string, step: number, t: number, rest: Record<string, unknown> = {}): TraceEvent => ({ kind, step, t, ...rest })

const RUN: TraceEvent[] = [
  ev('run_start', 0, 1000),
  ev('request', 1, 1100, {
    system_prompt: 'You are Vivianne.',
    tools: [{ name: 'read', description: 'Read a file' }],
    new_messages: [{ role: 'user', text: 'what is this' }, { role: 'user', text: '<system-reminder> use the skills </system-reminder>' }],
  }),
  ev('turn', 1, 3100, { input_ms: 100, model_ms: 2000, answered_by: 'openai/gpt', stop_reason: 'toolUse', input_tokens: 500, output_tokens: 40 }),
  ev('tool', 1, 3400, { tool: 'read', args: '{"path":"a.ts"}', result: 'contents', failed: false }),
  ev('tools_done', 1, 3500, { ms: 400, count: 1 }),
  ev('request', 2, 3600, { new_messages: [{ role: 'tool', tool: 'read', text: 'contents', failed: false }] }),
  ev('turn', 2, 5600, { input_ms: 100, model_ms: 1900, answered_by: 'openai/gpt', stop_reason: 'stop', input_tokens: 700, output_tokens: 90 }),
]

test('a prompt is a row once, user text and loop reminders are told apart, and results come back as rows', () => {
  const t = buildTrajectory(RUN)
  assert.deepEqual(
    t.rows.map((r) => r.kind),
    ['system', 'user', 'reminder', 'assistant', 'call', 'result', 'assistant'],
  )
  assert.equal(t.rows[0].title, 'Initial System Prompt')
  assert.equal(t.rows[0].tools?.[0].name, 'read')
  assert.equal(t.rows[1].summary, 'what is this')
})

test('each turn gets its three phases laid out in time, in order', () => {
  const { turns } = buildTrajectory(RUN)
  assert.equal(turns.length, 2)
  const [a] = turns
  assert.equal(a.inputMs, 100)
  assert.equal(a.modelMs, 2000)
  assert.equal(a.toolsMs, 400)
  assert.ok(a.inputStart < a.modelStart && a.modelStart < a.toolsStart)
  assert.equal(a.modelStart, 1100)
  assert.equal(a.toolsStart, 3100)
})

test('what is a reminder and what is not', () => {
  assert.equal(isReminder('<system-reminder> x'), true)
  assert.equal(isReminder('  <vivianne_goal phase="active">'), true)
  assert.equal(isReminder('please remind me'), false)
})

test('an empty trace is empty, not an error', () => {
  const t = buildTrajectory([])
  assert.deepEqual([t.rows.length, t.turns.length, t.start, t.end], [0, 0, 0, 0])
})
