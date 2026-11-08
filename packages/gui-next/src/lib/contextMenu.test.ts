import { test } from 'node:test'
import assert from 'node:assert/strict'
import { groupItems, registerMenu, resolveMenu } from './contextMenu.ts'

const item = (id: string, group?: string) => ({ id, label: id, run: () => {}, group })

test('items from every provider for the kind are gathered in order, duplicates dropped', async () => {
  const off1 = registerMenu<string>('t-gather', (t) => [item(`open ${t}`), item('shared')])
  const off2 = registerMenu<string>('t-gather', () => [item('shared'), item('more')])
  const ids = (await resolveMenu('t-gather', 'x')).map((i) => i.id)
  assert.deepEqual(ids, ['open x', 'shared', 'more'])
  off1()
  assert.deepEqual((await resolveMenu('t-gather', 'x')).map((i) => i.id), ['shared', 'more'])
  off2()
  assert.deepEqual(await resolveMenu('t-gather', 'x'), [])
  assert.deepEqual(await resolveMenu('nobody-registered', 1), [])
})

test('a provider that throws or is too slow is dropped without losing the others', async () => {
  registerMenu('t-slow', () => new Promise(() => {}))
  registerMenu('t-slow', () => {
    throw new Error('boom')
  })
  registerMenu('t-slow', async () => [item('fine')])
  const started = Date.now()
  const ids = (await resolveMenu('t-slow', null, 50)).map((i) => i.id)
  assert.deepEqual(ids, ['fine'])
  assert.ok(Date.now() - started < 1500)
})

test('groups keep first-seen order and unlabelled items form one group', () => {
  const groups = groupItems([item('a', 'x'), item('b'), item('c', 'x'), item('d', 'y')])
  assert.deepEqual(groups.map((g) => g.map((i) => i.id)), [['a', 'c'], ['b'], ['d']])
})
