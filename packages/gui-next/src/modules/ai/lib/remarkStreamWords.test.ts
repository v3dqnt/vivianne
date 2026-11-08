import { test } from 'node:test'
import assert from 'node:assert/strict'
import { remarkStreamWords, wrapWords } from './remarkStreamWords.ts'

interface TestNode {
  type: string
  value?: string
  children?: TestNode[]
  position?: { start: { offset: number } }
}
const text = (value: string, offset?: number): TestNode => ({ type: 'text', value, ...(offset === undefined ? {} : { position: { start: { offset } } }) })

test('words are wrapped and the whitespace between them is kept as text', () => {
  const parts = wrapWords('one  two\nthree')
  assert.deepEqual(parts.map((p) => (p.type === 'text' ? `_${p.value}_` : p.children?.[0].value)), ['one', '_  _', 'two', '_\n_', 'three'])
  assert.deepEqual(wrapWords('x')[0].data, { hName: 'span', hProperties: { className: ['viv-w'] } })
  assert.deepEqual(wrapWords(''), [])
})

test('only text near the end is wrapped; code, links text before the tail and earlier words are left alone', () => {
  const tree: TestNode = {
    type: 'root',
    children: [
      { type: 'paragraph', children: [text('early words here', 0), { type: 'strong', children: [text('late bold', 500)] }] },
      { type: 'code', value: 'let a = 1', position: { start: { offset: 600 } } },
      { type: 'paragraph', children: [text('tail text', 700), { type: 'inlineCode', value: 'x y', position: { start: { offset: 720 } } }] },
    ],
  }
  remarkStreamWords({ from: 400 })(tree)
  const para1 = tree.children![0].children!
  assert.equal(para1[0].type, 'text', 'an early text node stays text')
  assert.ok(para1[1].children!.some((c) => c.type === 'streamWord'), 'bold text in the tail is wrapped')
  assert.equal(tree.children![1].type, 'code')
  const para3 = tree.children![2].children!
  assert.equal(para3.filter((n) => n.type === 'streamWord').length, 2)
  assert.equal(para3.at(-1)!.type, 'inlineCode')
})

test('text without a position takes its parent\'s place in the reply', () => {
  const tree: TestNode = { type: 'root', children: [{ type: 'paragraph', position: { start: { offset: 900 } }, children: [{ type: 'text', value: 'a b' }] }] }
  remarkStreamWords({ from: 400 })(tree)
  assert.equal(tree.children![0].children!.filter((n) => n.type === 'streamWord').length, 2)
})
