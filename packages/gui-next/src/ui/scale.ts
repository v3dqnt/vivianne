/**
 * The design scale: the only font sizes, weights, corner radii and control
 * heights the interface uses. Components pick a step; they do not invent one.
 *
 * Why a fixed scale: with forty sizes and thirty radii, no two screens agree
 * and nothing reads as more important than anything else. Seven sizes, five
 * weights' worth of meaning and a handful of radii make the hierarchy
 * visible: the eye can tell a title from a label from a caption because they
 * differ by whole steps, not by a pixel.
 *
 * `scale.test.ts` reads the source and fails when a component steps off it.
 */

/** Type, in px. Smallest to largest. */
export const FONT_SIZES = [11, 12, 13, 14, 16, 18, 20, 24] as const
export type FontSize = (typeof FONT_SIZES)[number]

/**
 * What each size is for. 13 is the working size of the interface (rows,
 * buttons, fields); 12 is secondary text; 11 is the smallest anything gets
 * (captions, counters, badges); 14 is reading text; 16 and up are titles.
 */
export const TYPE_ROLES: Record<FontSize, string> = {
  11: 'caption: counters, badges, timestamps, overlines',
  12: 'secondary: descriptions, helper text, metadata',
  13: 'ui: rows, buttons, fields, menus, labels',
  14: 'body: messages and long-form text',
  16: 'title: dialog and section titles',
  18: 'heading: page headings',
  20: 'heading: large page headings',
  24: 'display: empty states and hero text',
}

/**
 * Weight carries meaning, so there are three: 400 for text you read, 500 for
 * labels and controls, 600 for titles. Bolder is never needed; if something
 * must stand out, make it larger or give it the accent colour.
 */
export const WEIGHTS = [400, 500, 600] as const

/** Corner radii, in px. Inner radius = outer radius minus the padding between. */
export const RADII = [4, 6, 8, 12, 16, 24] as const
/** Left alone: circles and pills are shapes, not steps on the scale. */
export const ROUND_RADII = new Set(['50%', '999px', '9999px', '999', '9999', '100%'])

/** Control heights, in px. Compact toolbar, standard control, large field, prominent button. */
export const CONTROL_HEIGHTS = [24, 28, 32, 36] as const

const nearest = <T extends number>(steps: readonly T[], value: number, tie: 'up' | 'down'): T => {
  let best = steps[0]
  let bestDist = Math.abs(value - best)
  for (const s of steps) {
    const d = Math.abs(value - s)
    if (d < bestDist || (d === bestDist && tie === 'up' && s > best)) {
      best = s
      bestDist = d
    }
  }
  return best
}

/** A px size as the step it belongs to: nearest, ties go larger (legibility first). */
export const snapFontSize = (px: number): FontSize => nearest(FONT_SIZES, px, 'up')

/** A radius as the nearest step, ties go smaller (a calmer shape). Tiny and huge ones are left. */
export const snapRadius = (px: number): number => (px <= 3 || px > 28 ? px : nearest(RADII, px, 'down'))

/** A control height as its step; heights that are not controls (bars, dots, rows) are left. */
export const snapControlHeight = (px: number): number => {
  if (px < 21 || px > 37) return px
  if (px <= 25) return 24
  if (px <= 29) return 28
  if (px <= 34) return 32
  return 36
}

/**
 * A weight as the step it should be, given the text it styles: titles are 600,
 * smaller text that was bold is a label, 500, and uppercase captions stay 600
 * because small caps need the weight to stay legible.
 */
export const snapWeight = (weight: number, sizePx: number | null, uppercase: boolean): number => {
  if (weight <= 400) return 400
  if (weight === 500) return 500
  if (sizePx !== null && sizePx <= 13 && !uppercase) return 500
  return 600
}

/** `1rem` → 16, `13px` → 13, `12` → 12; anything relative (`em`, `inherit`, calc) → null. */
export function pxOf(value: string | number): number | null {
  if (typeof value === 'number') return value
  const m = value.trim().match(/^(-?[0-9]*\.?[0-9]+)(px|rem)?$/)
  if (!m) return null
  return m[2] === 'rem' ? parseFloat(m[1]) * 16 : parseFloat(m[1])
}

/** A px size written the way the theme writes type: in rem, so it follows the user's zoom. */
export const toRem = (px: number): string => `${Number((px / 16).toFixed(4))}rem`
