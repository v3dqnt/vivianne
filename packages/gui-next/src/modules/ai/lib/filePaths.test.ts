import { test } from 'node:test'
import assert from 'node:assert/strict'
import { isFilePath, parseAnchor, splitAnchor, splitFilePaths } from './filePaths.ts'

test('a line or a range after a path is read as an anchor', () => {
  assert.deepEqual(parseAnchor(':24'), { line: 24 })
  assert.deepEqual(parseAnchor(':24:5'), { line: 24 })
  assert.deepEqual(parseAnchor(':24-31'), { line: 24, endLine: 31 })
  assert.deepEqual(parseAnchor('#L24'), { line: 24 })
  assert.deepEqual(parseAnchor('#L24-L30'), { line: 24, endLine: 30 })
  assert.equal(parseAnchor('nonsense'), null)
})

test('a reference splits into its path and its anchor', () => {
  assert.deepEqual(splitAnchor('src/app.ts:24-31'), { path: 'src/app.ts', anchor: { line: 24, endLine: 31 } })
  assert.deepEqual(splitAnchor('src/app.ts'), { path: 'src/app.ts', anchor: null })
  assert.equal(isFilePath('src/app.ts:24'), true)
  assert.equal(isFilePath('example.com:80'), false)
})

test('a path with an anchor in running text is one piece, shown as written', () => {
  const pieces = splitFilePaths('See src/app.ts:24 and lib/x.rs#L3-L9 for it.')
  const files = pieces.filter((p) => typeof p !== 'string')
  assert.equal(files.length, 2)
  assert.deepEqual(files[0], { path: 'src/app.ts', anchor: { line: 24 }, text: 'src/app.ts:24' })
  assert.deepEqual(files[1], { path: 'lib/x.rs', anchor: { line: 3, endLine: 9 }, text: 'lib/x.rs#L3-L9' })
})

test('paths without anchors and non-paths behave as before', () => {
  const pieces = splitFilePaths('Open README.md, but v1.8.2 and e.g. are not files.')
  assert.equal(pieces.filter((p) => typeof p !== 'string').length, 1)
})
