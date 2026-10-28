import test from 'node:test'
import assert from 'node:assert/strict'
import { parseDiff } from './diff.ts'

const DIFF = `diff --git a/src/app.ts b/src/app.ts
index 111..222 100644
--- a/src/app.ts
+++ b/src/app.ts
@@ -1,3 +1,4 @@
 const a = 1
-const b = 2
+const b = 3
+const c = 4
 export {}
@@ -10,2 +11,2 @@ function f() {
-x
+y
`

test('parseDiff reads hunks and numbers', () => {
  const hunks = parseDiff(DIFF)
  assert.equal(hunks.length, 2)
  assert.equal(hunks[0].header, '@@ -1,3 +1,4 @@')
  assert.deepEqual(hunks[0].lines[0], { kind: 'ctx', oldNo: 1, newNo: 1, text: 'const a = 1' })
  assert.deepEqual(hunks[0].lines[1], { kind: 'del', oldNo: 2, newNo: null, text: 'const b = 2' })
  assert.deepEqual(hunks[0].lines[2], { kind: 'add', oldNo: null, newNo: 2, text: 'const b = 3' })
  assert.deepEqual(hunks[0].lines[4], { kind: 'ctx', oldNo: 3, newNo: 4, text: 'export {}' })
})

test('parseDiff ignores file headers and keeps context lines of a hunk', () => {
  const hunks = parseDiff(DIFF)
  assert.equal(hunks[1].header, '@@ -10,2 +11,2 @@ function f() {')
  assert.deepEqual(hunks[1].lines.map((l) => l.kind), ['del', 'add'])
})

test('parseDiff tolerates an empty diff', () => {
  assert.deepEqual(parseDiff(''), [])
})

test('parseDiff counts an added file from zero', () => {
  const hunks = parseDiff('@@ -0,0 +1,2 @@\n+a\n+b\n')
  assert.deepEqual(hunks[0].lines.map((l) => [l.kind, l.oldNo, l.newNo]), [['add', null, 1], ['add', null, 2]])
})
