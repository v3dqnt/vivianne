import { test } from 'node:test'
import assert from 'node:assert/strict'
import { scanAll } from '../../scripts/design-scale.mjs'
import { snapControlHeight, snapFontSize, snapRadius, snapWeight, pxOf, toRem } from './scale.ts'

test('the interface stays on the design scale', () => {
  const off = scanAll()
  const shown = off.slice(0, 12).map((o: { file: string; rule: string; from: unknown; to: unknown }) => `${o.file}: ${o.rule} ${o.from} → ${o.to}`)
  assert.equal(off.length, 0, `${off.length} value(s) off the scale. Run \`node scripts/design-scale.mjs --fix\`, or pick a step from src/ui/scale.ts:\n${shown.join('\n')}`)
})

test('sizes snap to the nearest step, ties to the larger one', () => {
  assert.equal(snapFontSize(12.5), 13)
  assert.equal(snapFontSize(15), 16)
  assert.equal(snapFontSize(10), 11)
  assert.equal(snapFontSize(11.2), 11)
  assert.equal(snapFontSize(21), 20)
})

test('weights: 400 reads, 500 labels, 600 titles', () => {
  assert.equal(snapWeight(600, 13, false), 500, 'a bold label becomes a label')
  assert.equal(snapWeight(700, 16, false), 600, 'a heading stays a heading, not bolder')
  assert.equal(snapWeight(700, 11, true), 600, 'small caps keep their weight')
  assert.equal(snapWeight(400, 13, false), 400)
})

test('radii and control heights snap, bars and circles are left', () => {
  assert.equal(snapRadius(10), 8)
  assert.equal(snapRadius(14), 12)
  assert.equal(snapRadius(2), 2)
  assert.equal(snapRadius(40), 40)
  assert.equal(snapControlHeight(26), 28)
  assert.equal(snapControlHeight(34), 32)
  assert.equal(snapControlHeight(6), 6)
})

test('units are read and written in rem', () => {
  assert.equal(pxOf('0.8125rem'), 13)
  assert.equal(pxOf('12px'), 12)
  assert.equal(pxOf('1.2em'), null)
  assert.equal(toRem(13), '0.8125rem')
})
