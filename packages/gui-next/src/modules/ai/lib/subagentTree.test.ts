import { test } from 'node:test'
import assert from 'node:assert/strict'
import { stopReasonLabel, subagentTree } from './subagentTree.ts'

const a = (id: string, parent: string | null, at: number) => ({ id, name: id, parent, startedAtEpochMs: at })

test('children sit under their parent, newest first, and orphans stay at the top', () => {
  const list = [a('root', null, 1), a('kid-old', 'root', 2), a('kid-new', 'root', 5), a('grand', 'kid-new', 6), a('orphan', 'gone', 3)]
  assert.deepEqual(
    subagentTree(list).map((n) => [n.agent.id, n.depth]),
    [['orphan', 0], ['root', 0], ['kid-new', 1], ['grand', 2], ['kid-old', 1]],
  )
})

test('a parent can be named by its name as well as its id', () => {
  const list = [{ id: 'x1', name: 'audit', parent: null, startedAtEpochMs: 1 }, { id: 'x2', name: 'helper', parent: 'audit', startedAtEpochMs: 2 }]
  assert.deepEqual(subagentTree(list).map((n) => n.depth), [0, 1])
})

test('a cycle or a self-parent cannot hang the list or drop anyone', () => {
  const list = [a('p', 'q', 1), a('q', 'p', 2), a('self', 'self', 3)]
  assert.equal(subagentTree(list).length, 3)
})

test('stop reasons read in plain words', () => {
  assert.equal(stopReasonLabel('max-tokens'), 'ran out of tokens')
  assert.equal(stopReasonLabel('completed'), 'finished')
  assert.equal(stopReasonLabel(null), null)
})
