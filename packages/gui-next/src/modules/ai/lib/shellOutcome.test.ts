import { test } from 'node:test'
import assert from 'node:assert/strict'
import { shellOutcome } from './shellOutcome.ts'

test('a clean exit shows nothing; a non-zero exit, a timeout and a kill each say so', () => {
  assert.equal(shellOutcome({ exit_code: 0, streamed: true }), null)
  assert.deepEqual(shellOutcome({ exit_code: 2, streamed: true }), { label: 'exit 2', tone: 'warn' })
  assert.deepEqual(shellOutcome({ timed_out: true, streamed: true }), { label: 'timed out', tone: 'bad' })
  assert.deepEqual(shellOutcome({ exit_code: null, streamed: true }), { label: 'killed', tone: 'bad' })
})

test('details from other tools are ignored', () => {
  assert.equal(shellOutcome(undefined), null)
  assert.equal(shellOutcome({ exit_code: 1 }), null)
  assert.equal(shellOutcome({ path: 'a.ts' }), null)
})
