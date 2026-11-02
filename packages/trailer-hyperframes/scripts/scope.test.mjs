import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
const { scopeSelector } = createRequire(import.meta.url)('../tools/capture/scope.cjs')

test('root selectors map to the app wrappers', () => {
  assert.equal(scopeSelector('html'), '.viv-app')
  assert.equal(scopeSelector(':root'), '.viv-app')
  assert.equal(scopeSelector('body'), '.viv-app .viv-body')
  assert.equal(scopeSelector('html *'), '.viv-app *')
  assert.equal(scopeSelector('#root'), '.viv-app #root')
})

test('every other selector is confined to the app', () => {
  assert.equal(scopeSelector('*, ::before, ::after'), '.viv-app *, .viv-app ::before, .viv-app ::after')
  assert.equal(scopeSelector('strong, b'), '.viv-app strong, .viv-app b')
  assert.equal(scopeSelector('.css-abc:hover > span'), '.viv-app .css-abc:hover > span')
})

test('already-scoped selectors are left alone', () => {
  assert.equal(scopeSelector('.viv-app .x'), '.viv-app .x')
})

test('commas inside :is()/:not() do not split the selector', () => {
  assert.equal(scopeSelector('a:not(.b, .c), d'), '.viv-app a:not(.b, .c), .viv-app d')
})
