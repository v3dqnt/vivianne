import { test } from 'node:test'
import assert from 'node:assert/strict'
import { BANNER_H, BANNER_W, PRESETS, bannerBackground, bannerGradient, fitBanner, parseBanner, presetBanner, tint } from './banner.ts'

test('a wide picture is cropped at the sides, a tall one at the top and bottom, centred', () => {
  const wide = fitBanner(2000, 400)
  assert.equal(wide.sh, 400)
  assert.equal(wide.sw, Math.round(400 * (BANNER_W / BANNER_H)))
  assert.equal(wide.sx, Math.round((2000 - wide.sw) / 2))
  const tall = fitBanner(600, 900)
  assert.equal(tall.sw, 600)
  assert.equal(tall.sh, 200)
  assert.equal(tall.sy, 350)
  assert.deepEqual(fitBanner(960, 320), { sx: 0, sy: 0, sw: 960, sh: 320 })
})

test('a picture with no size is left alone, not divided by zero', () => {
  assert.deepEqual(fitBanner(0, 0), { sx: 0, sy: 0, sw: 0, sh: 0 })
})

test('what a project stores is read back', () => {
  assert.deepEqual(parseBanner('preset:dusk'), { kind: 'preset', id: 'dusk' })
  assert.deepEqual(parseBanner('file:jpg'), { kind: 'file' })
  assert.deepEqual(parseBanner(null), { kind: 'none' })
  assert.deepEqual(parseBanner('anything else'), { kind: 'none' })
})

test('the same colour always gives the same banner, and every preset paints', () => {
  assert.equal(tint('#4a9eff', 40, 0.5, 0.2), tint('#4a9eff', 40, 0.5, 0.2))
  assert.match(tint('#4a9eff', 0, 0.5, 0.2), /^hsl\(\d+ \d+% \d+%\)$/)
  assert.match(tint('not a colour'), /^hsl\(/, 'a bad colour falls back instead of throwing')
  for (const p of PRESETS) assert.ok(presetBanner(p.id, '#8b5cf6').length > 30, p.id)
  assert.equal(presetBanner('nope', '#8b5cf6'), bannerGradient('#8b5cf6'))
  assert.notEqual(presetBanner('aurora', '#8b5cf6'), presetBanner('aurora', '#14b8a6'), 'tinted by the project colour')
})

test('a banner paints its own image when it has one, else its preset, else the gradient', () => {
  assert.match(bannerBackground('file:jpg', '#8b5cf6', 'data:image/jpeg;base64,AAAA'), /url\("data:image\/jpeg/)
  assert.equal(bannerBackground('file:jpg', '#8b5cf6', null), bannerGradient('#8b5cf6'), 'image not loaded yet')
  assert.equal(bannerBackground('preset:grid', '#8b5cf6', null), presetBanner('grid', '#8b5cf6'))
  assert.equal(bannerBackground(undefined, '#8b5cf6', null), bannerGradient('#8b5cf6'))
})
