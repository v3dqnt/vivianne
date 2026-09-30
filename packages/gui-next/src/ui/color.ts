/**
 * Color math for the theme — `alpha`, `darken`, `lighten`, `emphasize`,
 * `getContrastText`. Same semantics (and the same output strings) as the
 * helpers the app used from MUI, so every tint computed from the palette is
 * identical after the move to Radix.
 */

interface Rgba {
  type: 'rgb' | 'rgba' | 'hsl' | 'hsla'
  values: [number, number, number, number?]
}

function hexToRgb(hex: string): string {
  const h = hex.slice(1)
  const size = h.length >= 6 ? 2 : 1
  const parts = h.match(new RegExp(`.{${size}}`, 'g')) ?? []
  const nums = parts.map((p) => parseInt(size === 1 ? p + p : p, 16))
  if (nums.length === 4) return `rgba(${nums[0]}, ${nums[1]}, ${nums[2]}, ${Math.round((nums[3] / 255) * 1000) / 1000})`
  return `rgb(${nums.join(', ')})`
}

function decompose(color: string): Rgba | null {
  const c = color.trim()
  if (c.startsWith('#')) return decompose(hexToRgb(c))
  const m = c.match(/^(rgba?|hsla?)\((.*)\)$/)
  if (!m) return null
  const type = m[1] as Rgba['type']
  const parts = m[2].split(/[\s,/]+/).filter(Boolean).map((p) => parseFloat(p))
  if (parts.length < 3 || parts.some((n) => Number.isNaN(n))) return null
  return { type, values: [parts[0], parts[1], parts[2], parts[3]] }
}

function recompose({ type, values }: Rgba): string {
  // rgb channels are truncated to integers, as MUI did.
  const [a, b, c] = type.startsWith('rgb') ? values.slice(0, 3).map((v) => Math.trunc(v as number)) : values
  const d = values[3]
  const body = type.startsWith('hsl') ? `${a}, ${b}%, ${c}%` : `${a}, ${b}, ${c}`
  return d === undefined ? `${type.replace(/a$/, '')}(${body})` : `${type.replace(/a?$/, 'a')}(${body}, ${d})`
}

const clamp = (v: number, min = 0, max = 1) => Math.min(Math.max(v, min), max)

function toRgb(color: Rgba): Rgba {
  if (color.type.startsWith('rgb')) return color
  const [h, s0, l0, a] = color.values
  const s = s0 / 100
  const l = l0 / 100
  const k = (n: number) => (n + h / 30) % 12
  const f = (n: number) => l - s * Math.min(l, 1 - l) * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1))
  return { type: a === undefined ? 'rgb' : 'rgba', values: [Math.round(f(0) * 255), Math.round(f(8) * 255), Math.round(f(4) * 255), a] }
}

/** `color` at `opacity`. Unparseable input (a CSS variable, a keyword) falls
 * back to `color-mix`, which the WebView handles natively. */
export function alpha(color: string, opacity: number): string {
  const c = decompose(color)
  if (!c) return `color-mix(in srgb, ${color} ${clamp(opacity) * 100}%, transparent)`
  const values: Rgba['values'] = [c.values[0], c.values[1], c.values[2], clamp(opacity)]
  return recompose({ type: c.type.startsWith('hsl') ? 'hsla' : 'rgba', values })
}

export function darken(color: string, amount: number): string {
  const c = decompose(color)
  if (!c) return color
  const k = 1 - clamp(amount)
  if (c.type.startsWith('hsl')) c.values[2] *= k
  else for (let i = 0; i < 3; i += 1) c.values[i] = (c.values[i] as number) * k
  return recompose(c)
}

export function lighten(color: string, amount: number): string {
  const c = decompose(color)
  if (!c) return color
  const k = clamp(amount)
  if (c.type.startsWith('hsl')) c.values[2] += (100 - c.values[2]) * k
  else for (let i = 0; i < 3; i += 1) c.values[i] = (c.values[i] as number) + (255 - (c.values[i] as number)) * k
  return recompose(c)
}

export function getLuminance(color: string): number {
  const c = decompose(color)
  if (!c) return 0
  const rgb = toRgb(c).values.slice(0, 3).map((v) => {
    const x = (v as number) / 255
    return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4
  })
  return Number((0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]).toFixed(3))
}

export function getContrastRatio(a: string, b: string): number {
  const la = getLuminance(a)
  const lb = getLuminance(b)
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05)
}

/** Darken light colors, lighten dark ones. */
export function emphasize(color: string, amount = 0.15): string {
  return getLuminance(color) > 0.5 ? darken(color, amount) : lighten(color, amount)
}

/** `color` with its existing opacity scaled by `k` — Tailwind's `input/50`
 * on a token that is itself translucent (`rgba(255,255,255,0.13)` at 50%
 * is 0.065, not 0.5). */
export function fade(color: string, k: number): string {
  const c = decompose(color)
  if (!c) return `color-mix(in srgb, ${color} ${clamp(k) * 100}%, transparent)`
  const a = c.values[3] ?? 1
  return recompose({ type: c.type.startsWith('hsl') ? 'hsla' : 'rgba', values: [c.values[0], c.values[1], c.values[2], Math.round(a * clamp(k) * 1000) / 1000] })
}
