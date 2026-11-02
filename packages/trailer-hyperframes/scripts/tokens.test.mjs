import { test } from 'node:test'
import assert from 'node:assert/strict'
import { tokensCss } from './tokens.mjs'

const css = await tokensCss()

test('dark monochrome primary is the app ink', () => {
  assert.match(css, /--primary: #f7f7f7;/)
  assert.match(css, /--background: #141414;/)
})

test('every app token is emitted, plus the derived frame and pane radius', () => {
  for (const k of ['foreground', 'card', 'popover', 'primary-foreground', 'secondary', 'muted', 'muted-foreground', 'accent', 'destructive', 'border', 'input', 'ring', 'frame'])
    assert.match(css, new RegExp(`--${k}: `), k)
  assert.match(css, /--surface-radius: 14px;/)
})

test('frame is card mixed 3% toward foreground (the app window background)', () => {
  // card #1d1d1d, fg #f7f7f7 -> round(29*.97 + 247*.03) = 36 = 0x24
  assert.match(css, /--frame: #242424;/)
})
