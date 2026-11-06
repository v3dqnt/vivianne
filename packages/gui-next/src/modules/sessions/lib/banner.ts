/**
 * A project's banner: the picture across the top of its threads, like a
 * Discord server's. Pure helpers only (the canvas work that needs a browser
 * is in `bannerImage.ts`), so the shapes can be tested.
 */

export const BANNER_W = 960
export const BANNER_H = 320

/** The part of a `srcW`×`srcH` picture that fills a 3:1 banner (cover fit, centred). */
export function fitBanner(srcW: number, srcH: number): { sx: number; sy: number; sw: number; sh: number } {
  const target = BANNER_W / BANNER_H
  if (srcW <= 0 || srcH <= 0) return { sx: 0, sy: 0, sw: Math.max(srcW, 0), sh: Math.max(srcH, 0) }
  if (srcW / srcH >= target) {
    const sw = Math.round(srcH * target)
    return { sx: Math.round((srcW - sw) / 2), sy: 0, sw, sh: srcH }
  }
  const sh = Math.round(srcW / target)
  return { sx: 0, sy: Math.round((srcH - sh) / 2), sw: srcW, sh }
}

/** What is stored on a project, read. */
export type BannerRef = { kind: 'none' } | { kind: 'preset'; id: string } | { kind: 'file' }

export function parseBanner(ref: string | null | undefined): BannerRef {
  if (!ref) return { kind: 'none' }
  if (ref.startsWith('preset:')) return { kind: 'preset', id: ref.slice('preset:'.length) }
  if (ref.startsWith('file:')) return { kind: 'file' }
  return { kind: 'none' }
}

// ── Colour, just enough to tint presets from the project's colour ──

function hexToHsl(hex: string): [number, number, number] {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim())
  const n = m ? parseInt(m[1], 16) : 0x6c63ff
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => v / 255)
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const l = (max + min) / 2
  if (max === min) return [0, 0, l]
  const d = max - min
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
  const h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4
  return [h * 60, s, l]
}

/** `hsl()` for a colour turned `hue` degrees and set to the given saturation and lightness. */
export function tint(hex: string, hue = 0, s?: number, l?: number): string {
  const [h0, s0, l0] = hexToHsl(hex)
  const h = (((h0 + hue) % 360) + 360) % 360
  return `hsl(${Math.round(h)} ${Math.round((s ?? s0) * 100)}% ${Math.round((l ?? l0) * 100)}%)`
}

/** The banner of a project with none of its own: a quiet gradient from its colour. */
export function bannerGradient(color: string): string {
  return `linear-gradient(135deg, ${tint(color, 0, 0.55, 0.32)} 0%, ${tint(color, 28, 0.5, 0.2)} 100%)`
}

/** Built-in banners, drawn in CSS from the project colour: no bitmaps to ship. */
export const PRESETS = [
  { id: 'aurora', name: 'Aurora' },
  { id: 'dusk', name: 'Dusk' },
  { id: 'tide', name: 'Tide' },
  { id: 'ember', name: 'Ember' },
  { id: 'grove', name: 'Grove' },
  { id: 'dunes', name: 'Dunes' },
  { id: 'grid', name: 'Grid' },
  { id: 'dots', name: 'Dots' },
] as const

export type PresetId = (typeof PRESETS)[number]['id']

/** A preset as a CSS `background`, tinted by `color`. Unknown ids fall back to the gradient. */
export function presetBanner(id: string, color: string): string {
  const a = tint(color, 0, 0.6, 0.38)
  const b = tint(color, 40, 0.55, 0.26)
  const c = tint(color, -40, 0.5, 0.16)
  switch (id) {
    case 'aurora':
      return `radial-gradient(120% 90% at 15% 0%, ${tint(color, 60, 0.7, 0.5)} 0%, transparent 55%), radial-gradient(100% 100% at 90% 100%, ${a} 0%, transparent 60%), ${c}`
    case 'dusk':
      return `linear-gradient(180deg, ${tint(color, -30, 0.5, 0.2)} 0%, ${a} 55%, ${tint(color, 50, 0.7, 0.55)} 100%)`
    case 'tide':
      return `repeating-linear-gradient(172deg, transparent 0 14px, rgba(255,255,255,0.05) 14px 16px), linear-gradient(120deg, ${c} 0%, ${b} 100%)`
    case 'ember':
      return `radial-gradient(90% 120% at 50% 130%, ${tint(color, 20, 0.8, 0.55)} 0%, transparent 65%), linear-gradient(180deg, ${c}, ${tint(color, 0, 0.4, 0.1)})`
    case 'grove':
      return `radial-gradient(60% 80% at 20% 100%, ${b} 0%, transparent 70%), radial-gradient(50% 70% at 80% 100%, ${a} 0%, transparent 70%), ${c}`
    case 'dunes':
      return `radial-gradient(140% 60% at 30% 110%, ${tint(color, 30, 0.6, 0.5)} 0 45%, transparent 46%), radial-gradient(140% 60% at 85% 120%, ${a} 0 50%, transparent 51%), linear-gradient(180deg, ${c}, ${b})`
    case 'grid':
      return `linear-gradient(rgba(255,255,255,0.06) 1px, transparent 1px) 0 0 / 24px 24px, linear-gradient(90deg, rgba(255,255,255,0.06) 1px, transparent 1px) 0 0 / 24px 24px, linear-gradient(135deg, ${c}, ${a})`
    case 'dots':
      return `radial-gradient(rgba(255,255,255,0.14) 1.5px, transparent 1.6px) 0 0 / 18px 18px, linear-gradient(135deg, ${a}, ${c})`
    default:
      return bannerGradient(color)
  }
}

/** What a project's banner should paint: its own picture, a preset, or the gradient. */
export function bannerBackground(ref: string | null | undefined, color: string, imageUrl: string | null | undefined): string {
  const parsed = parseBanner(ref)
  if (parsed.kind === 'file' && imageUrl) return `center / cover no-repeat url("${imageUrl}")`
  if (parsed.kind === 'preset') return presetBanner(parsed.id, color)
  return bannerGradient(color)
}
