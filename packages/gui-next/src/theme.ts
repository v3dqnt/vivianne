import { createTheme, type Mode, type Theme, type Tokens } from './ui'
import './assets/fonts/fonts.css'

/**
 * The single source of visual truth: Terax's design language (see
 * `ui/theme.ts`). The default theme is monochrome — "primary" is the ink
 * colour. Presets add a colour identity by swapping only `primary` and
 * `ring`; every neutral stays put, so the chrome reads the same in all of
 * them. Picked from Settings → Theme & layout.
 */

type TokenSet = Omit<Tokens, 'frame'>

/** Terax's default neutrals. */
const NEUTRAL: Record<Mode, TokenSet> = {
  light: {
    background: '#ffffff',
    foreground: '#111111',
    card: '#f3f3f3',
    popover: '#ffffff',
    primary: '#111111',
    primaryForeground: '#ffffff',
    secondary: '#ededed',
    muted: '#ededed',
    mutedForeground: '#656565',
    accent: '#e5e5e5',
    destructive: '#c0392f',
    border: 'rgba(0, 0, 0, 0.09)',
    input: 'rgba(0, 0, 0, 0.13)',
    ring: '#8a8a8a',
  },
  dark: {
    background: '#141414',
    foreground: '#f7f7f7',
    card: '#1d1d1d',
    popover: '#1d1d1d',
    primary: '#f7f7f7',
    primaryForeground: '#141414',
    secondary: '#2a2a2a',
    muted: '#1d1d1d',
    mutedForeground: '#a3a3a3',
    accent: '#2a2a2a',
    destructive: '#e35d5d',
    border: 'rgba(255, 255, 255, 0.09)',
    input: 'rgba(255, 255, 255, 0.13)',
    ring: '#8a8a8a',
  },
}

export interface PaletteDef {
  id: string
  label: string
  symbol: string
  swatches: string[]
  /** Per mode: the tokens this preset changes. Usually just the accent. */
  light: Partial<TokenSet>
  dark: Partial<TokenSet>
}

const accent = (color: string, onColor: string) => ({ primary: color, primaryForeground: onColor, ring: color })

export const PALETTES: PaletteDef[] = [
  {
    id: 'nvidia',
    label: 'NVIDIA',
    symbol: '▲',
    swatches: ['#76b900', '#0a0a0a', '#1f1f1f', '#f5f5f5'],
    light: {
      foreground: '#111111',
      mutedForeground: '#5c6155',
      ...accent('#4c7a00', '#ffffff'),
    },
    dark: {
      background: '#0a0a0a',
      card: '#121212',
      popover: '#121212',
      secondary: '#1c1c1c',
      muted: '#121212',
      accent: '#1f2a10',
      border: 'rgba(255, 255, 255, 0.08)',
      ...accent('#76b900', '#0a0a0a'),
    },
  },
  {
    id: 'nemotron',
    label: 'Nemotron',
    symbol: '◆',
    swatches: ['#76b900', '#0b140a', '#17250f', '#e9f1dc'],
    light: {
      background: '#f7faf1',
      card: '#edf3e0',
      popover: '#f7faf1',
      foreground: '#121a0b',
      mutedForeground: '#5b6a4a',
      secondary: '#e3ebd2',
      muted: '#e3ebd2',
      accent: '#dbe6c4',
      border: 'rgba(18, 26, 11, 0.10)',
      ...accent('#4c7a00', '#ffffff'),
    },
    dark: {
      background: '#0b140a',
      card: '#111d0d',
      popover: '#111d0d',
      foreground: '#e9f1dc',
      mutedForeground: '#9db086',
      secondary: '#182713',
      muted: '#111d0d',
      accent: '#1c2f14',
      border: 'rgba(118, 185, 0, 0.16)',
      input: 'rgba(118, 185, 0, 0.22)',
      ...accent('#76b900', '#0b140a'),
    },
  },
  {
    id: 'terax',
    label: 'Monochrome',
    symbol: '◐',
    swatches: ['#f7f7f7', '#a3a3a3', '#2a2a2a', '#141414'],
    light: {},
    dark: {},
  },
  {
    id: 'vivianne',
    label: 'Vivianne',
    symbol: '☀',
    swatches: ['#4a9eff', '#1a73e8', '#1d1d1d', '#f7f7f7'],
    light: accent('#1a73e8', '#ffffff'),
    dark: accent('#4a9eff', '#0a0a0a'),
  },
  {
    id: 'claude',
    label: 'Claude',
    symbol: '✳',
    swatches: ['#d97757', '#262624', '#b0a89a', '#f5f4ee'],
    light: {
      background: '#faf9f5',
      card: '#f0eee6',
      popover: '#faf9f5',
      foreground: '#1f1e1d',
      mutedForeground: '#6b675f',
      secondary: '#e8e6dc',
      muted: '#e8e6dc',
      accent: '#e3e0d4',
      border: 'rgba(31, 30, 29, 0.10)',
      ...accent('#c96442', '#ffffff'),
    },
    dark: {
      background: '#1f1e1d',
      card: '#262624',
      popover: '#262624',
      foreground: '#f5f4ee',
      mutedForeground: '#b0a89a',
      secondary: '#30302e',
      muted: '#262624',
      accent: '#30302e',
      border: 'rgba(245, 244, 238, 0.10)',
      input: 'rgba(245, 244, 238, 0.13)',
      ...accent('#d97757', '#1f1e1d'),
    },
  },
  {
    id: 'indigo',
    label: 'Space Indigo',
    symbol: '✦',
    swatches: ['#ef233c', '#2b2d42', '#8d99ae', '#edf2f4'],
    light: accent('#c41e30', '#ffffff'),
    dark: accent('#ef233c', '#ffffff'),
  },
  {
    id: 'graphite',
    label: 'Graphite',
    symbol: '◈',
    swatches: ['#a9a29c', '#5a5550', '#28262b', '#e8e4e0'],
    light: accent('#5a5550', '#ffffff'),
    dark: accent('#a9a29c', '#141414'),
  },
]

export const FONT_FAMILY = '"Geist", system-ui, -apple-system, "Segoe UI", sans-serif'
export const MONO_FAMILY = '"Geist Mono", ui-monospace, "Cascadia Code", monospace'

export type FontChoice = 'geist' | 'inter' | 'system' | 'serif' | 'mono'

/** The UI font stacks offered in Settings → Appearance. */
export const FONTS: Record<FontChoice, { label: string; family: string }> = {
  geist: { label: 'Geist', family: FONT_FAMILY },
  inter: { label: 'Inter', family: '"Inter", system-ui, -apple-system, "Segoe UI", sans-serif' },
  system: { label: 'System', family: 'system-ui, -apple-system, "Segoe UI", Roboto, sans-serif' },
  serif: { label: 'Serif', family: '"Source Serif 4", Georgia, "Times New Roman", serif' },
  mono: { label: 'Mono', family: MONO_FAMILY },
}

/** What the user chose beyond the palette (see `themeStore`). */
export interface ThemeOptions {
  /** Themes the user made (`~/.viv/themes`), looked up after the built-ins. */
  custom?: PaletteDef[]
  /** Any colour, over the palette's primary. */
  accent?: string | null
  font?: FontChoice
  radius?: number
  /** A wallpaper is showing: the frame goes clear and panes let `glass`
   * (0 solid … 1 most see-through) of it through. */
  wallpaper?: boolean
  glass?: number
}

/** `hex` (#rgb / #rrggbb) or `rgb(a)` at `alpha` opacity. */
export function withAlpha(color: string, alpha: number): string {
  const c = color.trim()
  let m = /^#([0-9a-f]{3})$/i.exec(c)
  const hex = m ? m[1].split('').map((x) => x + x).join('') : /^#([0-9a-f]{6})$/i.exec(c)?.[1]
  if (hex) {
    const n = parseInt(hex, 16)
    return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`
  }
  m = /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)/i.exec(c)
  return m ? `rgba(${m[1]}, ${m[2]}, ${m[3]}, ${alpha})` : c
}

/** Black or white, whichever reads better on `hex`. */
export function onColor(hex: string): string {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim())
  if (!m) return '#ffffff'
  const n = parseInt(m[1], 16)
  const lin = (c: number) => {
    const v = c / 255
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4
  }
  const l = 0.2126 * lin((n >> 16) & 255) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255)
  return l > 0.4 ? '#111111' : '#ffffff'
}

/** Every palette: built-ins, then the user's own. */
export function allPalettes(custom: PaletteDef[] = []): PaletteDef[] {
  return [...PALETTES, ...custom.filter((c) => !PALETTES.some((p) => p.id === c.id))]
}

/** The full token set for a palette in a mode — what the app is painted
 * with, and what mod pages receive as `--viv-*` variables. */
export function themeTokens(mode: Mode, paletteId: string, opts: ThemeOptions = {}): TokenSet {
  const preset = allPalettes(opts.custom).find((p) => p.id === paletteId) ?? PALETTES[0]
  const tokens = { ...NEUTRAL[mode], ...preset[mode] }
  if (opts.accent) Object.assign(tokens, accent(opts.accent, onColor(opts.accent)))
  return tokens
}

export function buildTheme(mode: Mode, paletteId: string, opts: ThemeOptions = {}): Theme {
  const theme = createTheme({
    mode,
    tokens: themeTokens(mode, paletteId, opts),
    fontFamily: FONTS[opts.font ?? 'geist'].family,
    monoFamily: MONO_FAMILY,
    radius: opts.radius ?? 10,
  })
  if (opts.wallpaper) {
    // Solid at glass 0; at 1 a pane is 45% opaque (more and text over a busy
    // image stops being readable).
    const alpha = 1 - Math.min(1, Math.max(0, opts.glass ?? 0.3)) * 0.55
    const bg = theme.palette.background
    theme.palette.background = {
      ...bg,
      default: 'transparent',
      chrome: 'transparent',
      pane: withAlpha(bg.pane, alpha),
      card: withAlpha(bg.card, Math.min(1, alpha + 0.1)),
    }
  }
  return theme
}

/** The token names, for the theme builder. */
export const TOKEN_NAMES = Object.keys(NEUTRAL.dark) as (keyof TokenSet)[]
export type { TokenSet }

/** App-wide resets on top of the kit's baseline, passed to `CssBaseline`. */
export function baselineExtras(_mode: Mode) {
  return {
    // A desktop app is a single long-lived window, not a document — it should
    // never rubber-band or show a page scrollbar at the root.
    'html, body, #root': {
      height: '100%',
      // `clip`, not `hidden`. Both hide overflow, but a `hidden` box is
      // still a scroll container: focusing anything that sits below the
      // fold scrolls it to bring the element into view. When a docked
      // terminal first mounted, its input briefly sat off-screen and
      // xterm focused it — scrolling the entire app up by ~260px, with
      // no scrollbar to scroll it back. `clip` can't be scrolled at all.
      overflow: 'clip',
      margin: 0,
    },
    // No native scrollbars anywhere: they are chunky on Windows and flash
    // on macOS. Regions still scroll with the wheel, trackpad and keys.
    'html *': { scrollbarWidth: 'none' },
    'html *::-webkit-scrollbar': { display: 'none', width: 0, height: 0 },
    // Chrome isn't text. Selection is on only where there's something to
    // copy: fields, editors, the terminal, and regions marked `.select-text`.
    body: { userSelect: 'none' },
    'input, textarea, [contenteditable="true"], .select-text, .select-text *, .cm-editor, .xterm': { userSelect: 'text' },
    // One global snap for reduced motion rather than per-component opt-outs.
    '@media (prefers-reduced-motion: reduce)': {
      '*, *::before, *::after': { animationDuration: '0.01ms !important', animationIterationCount: '1 !important', transitionDuration: '0.01ms !important' },
    },
  }
}

/** Default export kept for any consumer that hasn't adopted the store yet. */
export const theme = buildTheme('dark', 'terax')

/** Corner radius of a pane — the sidebar, every dock panel, the composer
 * card (Terax's `--pane-radius`, base radius × 1.4). One constant because
 * panes sit side by side: different radii would read as unrelated objects.
 * A raw px value, not an `sx` radius multiple (`borderRadius: 1.4` would
 * also mean 14px, but reads as a mystery number). */
export const SURFACE_RADIUS = 14

/**
 * A pane: the working surfaces floating on the frame. Solid fill, a 9%
 * hairline, no drop shadow (resizable panes would clip it) — in dark mode
 * only a faint inset top highlight. Values are palette paths, for `sx`.
 */
export function glassSurface(mode: Mode) {
  return {
    bgcolor: 'background.pane',
    // Only blurs when a wallpaper is set (Settings → Appearance).
    backdropFilter: 'var(--viv-glass-filter, none)',
    border: '1px solid',
    borderColor: 'divider',
    boxShadow: mode === 'dark' ? 'inset 0 1px 0 rgba(255,255,255,0.035)' : 'none',
  } as const
}

/** A pane with its radius. */
export function surfaceCard(mode: Mode) {
  return { ...glassSurface(mode), borderRadius: `${SURFACE_RADIUS}px` } as const
}
