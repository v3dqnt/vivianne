import { test } from 'node:test'
import assert from 'node:assert/strict'
import { gitApplyCommand, hideImports, parseDiff, wordDiffs, wordSegments } from './diffView.ts'

const DIFF = [
  'diff --git a/a.ts b/a.ts',
  '--- a/a.ts',
  '+++ b/a.ts',
  '@@ -1,4 +1,4 @@',
  " import { a } from './a'",
  "-import { b } from './b'",
  "+import { b, c } from './b'",
  ' const x = 1',
  '-const y = 2',
  '+const y = 3',
  '@@ -20,2 +20,2 @@',
  '-export * from "./old"',
  '+export * from "./new"',
].join('\n')

test('a diff becomes numbered lines, with its hunk headers', () => {
  const lines = parseDiff(DIFF)
  assert.equal(lines[0].kind, 'hunk')
  assert.deepEqual(
    lines.slice(1, 4).map((l) => [l.kind, l.oldNo, l.newNo]),
    [['ctx', 1, 1], ['del', 2, null], ['add', null, 2]],
  )
})

test('hiding imports drops import-only changes and the hunk left empty', () => {
  const shown = hideImports(parseDiff(DIFF), 'a.ts')
  assert.deepEqual(
    shown.filter((l) => l.kind === 'add' || l.kind === 'del').map((l) => l.text),
    ['const y = 2', 'const y = 3'],
  )
  assert.equal(shown.filter((l) => l.kind === 'hunk').length, 1, 'the second hunk was only an export-from')
})

test('an import over several lines is hidden whole, other code stays', () => {
  const lines = parseDiff(['@@ -1,3 +1,5 @@', '+import {', '+  a,', '+  b,', "+} from './x'", '+const z = 1'].join('\n'))
  assert.deepEqual(hideImports(lines, 'a.tsx').filter((l) => l.kind === 'add').map((l) => l.text), ['const z = 1'])
})

test('imports are recognised per language, and left alone elsewhere', () => {
  const rust = parseDiff(['@@ -1,1 +1,2 @@', '+use std::fmt;', '+fn main() {}'].join('\n'))
  assert.deepEqual(hideImports(rust, 'm.rs').filter((l) => l.kind === 'add').map((l) => l.text), ['fn main() {}'])
  const text = parseDiff(['@@ -1,1 +1,1 @@', '+import is a word here'].join('\n'))
  assert.equal(hideImports(text, 'notes.txt').filter((l) => l.kind === 'add').length, 1)
})

test('word segments mark only the words that changed', () => {
  const seg = wordSegments('const y = 2', 'const y = 3')!
  assert.deepEqual(seg.old.filter((s) => s.changed).map((s) => s.text), ['2'])
  assert.deepEqual(seg.new.filter((s) => s.changed).map((s) => s.text), ['3'])
  assert.equal(seg.old.map((s) => s.text).join(''), 'const y = 2')
})

test('a line replaced wholesale gets no word marks, a small edit does', () => {
  const edit = wordDiffs(parseDiff(['@@ -1,1 +1,1 @@', '-let total = a + b', '+let total = a + c'].join('\n')))
  assert.equal(edit.size, 2)
  const whole = wordDiffs(parseDiff(['@@ -1,1 +1,1 @@', '-alpha', '+omega'].join('\n')))
  assert.equal(whole.size, 0)
})

test('the apply command carries the whole patch in a quoted heredoc', () => {
  const cmd = gitApplyCommand('--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b')
  assert.ok(cmd.startsWith("git apply <<'VIV_PATCH'\n--- a/x"))
  assert.ok(cmd.endsWith('+b\nVIV_PATCH'))
})
