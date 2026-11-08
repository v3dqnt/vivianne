import { test } from 'node:test'
import assert from 'node:assert/strict'
import { filterTree, visibleChildren, type TreeNode } from './fileTree.ts'

const f = (name: string): TreeNode => ({ name, path: `/w/${name}` })
const d = (name: string, children: TreeNode[] = []): TreeNode => ({ name, path: `/w/${name}`, type: 'directory', children })

test('folders come first, names sort naturally, and noise folders are hidden', () => {
  const names = visibleChildren([f('b.ts'), d('src'), f('a10.ts'), f('a2.ts'), d('node_modules'), d('.git'), d('docs')]).map((n) => n.name)
  assert.deepEqual(names, ['docs', 'src', 'a2.ts', 'a10.ts', 'b.ts'])
  assert.deepEqual(visibleChildren(undefined), [])
})

test('a filter keeps matches and the folders that lead to them', () => {
  const tree = [d('src', [d('ui', [f('Button.tsx'), f('Card.tsx')]), f('main.ts')]), f('README.md')]
  const kept = filterTree(tree, 'butt')
  assert.equal(kept.length, 1)
  assert.equal(kept[0].name, 'src')
  assert.deepEqual(kept[0].children?.map((n) => n.name), ['ui'])
  assert.deepEqual(kept[0].children?.[0].children?.map((n) => n.name), ['Button.tsx'])
  assert.deepEqual(filterTree(tree, '').length, 2)
  assert.deepEqual(filterTree(tree, 'zzz'), [])
})
