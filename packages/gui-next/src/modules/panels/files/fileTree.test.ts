import test from 'node:test'
import assert from 'node:assert/strict'
import { ancestorsOf, basename, changeFor, filterTree, mergeChanges, normalizePath, type TreeNode } from './fileTree.ts'

const tree: TreeNode[] = [
  {
    name: 'src',
    path: 'C:\\w\\src',
    type: 'directory',
    children: [
      { name: 'app.tsx', path: 'C:\\w\\src\\app.tsx', type: 'file' },
      { name: 'lib', path: 'C:\\w\\src\\lib', type: 'directory', children: [{ name: 'io.ts', path: 'C:\\w\\src\\lib\\io.ts', type: 'file' }] },
    ],
  },
  { name: 'README.md', path: 'C:\\w\\README.md', type: 'file' },
]

test('basename handles both separators', () => {
  assert.equal(basename('C:\\w\\src\\app.tsx'), 'app.tsx')
  assert.equal(basename('/home/me/app.tsx'), 'app.tsx')
  assert.equal(basename('C:\\w\\src\\'), 'src')
})

test('filterTree keeps matching files inside their folders', () => {
  const out = filterTree(tree, 'io')
  assert.equal(out.length, 1)
  assert.equal(out[0].name, 'src')
  assert.equal(out[0].children?.[0].name, 'lib')
  assert.equal(out[0].children?.[0].children?.[0].name, 'io.ts')
})

test('filterTree matches a folder name and keeps everything under it', () => {
  const out = filterTree(tree, 'lib')
  assert.equal(out[0].children?.[0].name, 'lib')
})

test('normalizePath unifies separators and case', () => {
  assert.equal(normalizePath('C:\\W\\src\\app.tsx'), 'c:/w/src/app.tsx')
})

test('mergeChanges adds staged and unstaged counts for the same file', () => {
  const merged = mergeChanges(
    [{ path: 'src/app.tsx', added: 2, removed: 1, status: 'M', staged: true }],
    [{ path: 'src/app.tsx', added: 3, removed: 0, status: 'M', staged: false }],
  )
  assert.deepEqual(merged.get('src/app.tsx'), { additions: 5, deletions: 1, status: 'M' })
})

test('mergeChanges keeps untracked files', () => {
  const merged = mergeChanges([], [{ path: 'new.ts', added: 7, removed: 0, status: '??', staged: false }])
  assert.deepEqual(merged.get('new.ts'), { additions: 7, deletions: 0, status: '??' })
})

test('changeFor matches a repo-relative key against an absolute path', () => {
  const merged = mergeChanges([], [{ path: 'src/app.tsx', added: 1, removed: 1, status: 'M', staged: false }])
  assert.equal(changeFor(merged, 'C:\\w\\src\\app.tsx')?.additions, 1)
  assert.equal(changeFor(merged, 'C:\\w\\other\\app.tsx'), undefined)
})

test('ancestorsOf lists folders between the root and the file', () => {
  assert.deepEqual(ancestorsOf('C:\\w\\src\\lib\\io.ts', 'C:\\w'), ['C:\\w\\src', 'C:\\w\\src\\lib'])
  assert.deepEqual(ancestorsOf('/w/src/io.ts', '/w'), ['/w/src'])
  assert.deepEqual(ancestorsOf('/other/io.ts', '/w'), [])
})
