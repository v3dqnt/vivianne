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

const FONT_FAMILY = '"Geist", system-ui, -apple-system, "Segoe UI", sans-serif'
const MONO_FAMILY = '"Geist Mono", ui-monospace, "Cascadia Code", monospace'

export function buildTheme(mode: Mode, paletteId: string): Theme {
  const preset = PALETTES.find((p) => p.id === paletteId) ?? PALETTES[0]
  return createTheme({
    mode,
    tokens: { ...NEUTRAL[mode], ...preset[mode] },
    fontFamily: FONT_FAMILY,
    monoFamily: MONO_FAMILY,
    radius: 10,
  })
}

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
    border: '1px solid',
    borderColor: 'divider',
    boxShadow: mode === 'dark' ? 'inset 0 1px 0 rgba(255,255,255,0.035)' : 'none',
  } as const
}

/** A pane with its radius. */
export function surfaceCard(mode: Mode) {
  return { ...glassSurface(mode), borderRadius: `${SURFACE_RADIUS}px` } as const
}
