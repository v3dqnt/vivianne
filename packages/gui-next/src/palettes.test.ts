import { test } from 'node:test'
import assert from 'node:assert/strict'
import { PALETTES, NEUTRAL, type PaletteDef } from './palettes.ts'

const hex = (c: string): [number, number, number] => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16)) as [number, number, number]
const lin = (v: number) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4)
const lum = ([r, g, b]: number[]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
const ratio = (a: number[], b: number[]) => {
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x)
  return (hi + 0.05) / (lo + 0.05)
}
const over = (fg: number[], bg: number[], a: number) => fg.map((f, i) => Math.round(f * a + bg[i] * (1 - a)))

// The alphas `createTheme` uses for the tertiary text level and the selected row.
const TERTIARY = { dark: 0.57, light: 0.64 }
const SELECTED = { dark: 0.1, light: 0.08 }

for (const p of PALETTES as PaletteDef[]) {
  for (const mode of ['dark', 'light'] as const) {
    const t = { ...NEUTRAL[mode], ...p[mode] }
    const surfaces = { pane: t.background, card: t.card, popover: t.popover }
    const fg = hex(t.foreground)
    const selected = over(fg, hex(t.background), SELECTED[mode])

    test(`${p.id} ${mode}: every text level and the accent clear 4.5:1 on every surface`, () => {
      for (const [name, bg] of Object.entries({ ...surfaces, 'selected row': '' })) {
        const bgc = name === 'selected row' ? selected : hex(bg)
        const checks: [string, number[]][] = [
          ['primary text', fg],
          ['secondary text', hex(t.mutedForeground)],
          ['tertiary text', over(fg, bgc, TERTIARY[mode])],
          ['accent text', hex(t.primary)],
        ]
        for (const [what, color] of checks) {
          const r = ratio(color, bgc)
          assert.ok(r >= 4.5, `${p.id}/${mode}: ${what} on ${name} is ${r.toFixed(2)}:1, needs 4.5`)
        }
      }
    })

    test(`${p.id} ${mode}: the label on the accent button is readable`, () => {
      const r = ratio(hex(t.primaryForeground), hex(t.primary))
      assert.ok(r >= 4.5, `${p.id}/${mode}: on-accent label is ${r.toFixed(2)}:1`)
    })
  }
}
