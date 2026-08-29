import { createTheme, type Theme } from '@mui/material/styles'
import './assets/fonts/fonts.css'

// A third background level, between `default` (canvas) and `paper` (floating
// cards) — used by docked shell chrome (rail, sidebar, side panel, bottom
// terminal) so those regions read as a distinct tone instead of relying on a
// 1px divider line to do all the separating work.
declare module '@mui/material/styles' {
  interface TypeBackground {
    chrome: string
  }
}

/**
 * The single source of visual truth. The previous frontend accumulated four
 * overlapping styling systems (Tailwind + daisyUI + Astryx + Radix), which is
 * what made it unmaintainable — every component picked a different one. This
 * app has exactly one: MUI's theme. No utility classes, no second component
 * library, no raw hex outside this file.
 *
 * Themes: a small set of named palettes, each with light + dark variants,
 * picked from the Settings → Themes tab (the old frontend's equivalent lived
 * in CSS variables on <html>; here it's just MUI palette overrides).
 */

export interface PaletteDef {
  id: string
  label: string
  symbol: string
  swatches: string[]
  light: PaletteColors
  dark: PaletteColors
}

export interface PaletteColors {
  background: string
  /** Docked shell chrome (rail, sidebar, side panel, terminal) \u2014 one step
   * from `background`, one step before `paper`. See the `TypeBackground`
   * augmentation above. */
  chrome: string
  paper: string
  primary: string
  textPrimary: string
  textSecondary: string
}

export const PALETTES: PaletteDef[] = [
  {
    id: 'vivianne',
    label: 'Vivianne',
    symbol: '\u2600',
    swatches: ['#4a9eff', '#0a5a9c', '#0a0a0a', '#ededed'],
    light: { background: '#f4f6fb', chrome: '#f8f9fc', paper: '#ffffff', primary: '#1a73e8', textPrimary: '#14171c', textSecondary: '#5b6470' },
    dark: { background: '#0a0a0a', chrome: '#101010', paper: '#141414', primary: '#4a9eff', textPrimary: '#ededed', textSecondary: '#a1a1a1' },
  },
  {
    id: 'indigo',
    label: 'Space Indigo',
    symbol: '\u2726',
    swatches: ['#ef233c', '#2b2d42', '#8d99ae', '#edf2f4'],
    light: { background: '#f0f2f8', chrome: '#f5f6fb', paper: '#ffffff', primary: '#c41e30', textPrimary: '#2b2d42', textSecondary: '#5d6078' },
    dark: { background: '#1a1b2e', chrome: '#212236', paper: '#2b2d42', primary: '#ef233c', textPrimary: '#edf2f4', textSecondary: '#8d99ae' },
  },
  {
    id: 'graphite',
    label: 'Graphite',
    symbol: '\u25c8',
    swatches: ['#a9a29c', '#333333', '#d5ccc7', '#28262b'],
    light: { background: '#f5f3f1', chrome: '#f9f7f5', paper: '#ffffff', primary: '#5a5550', textPrimary: '#333333', textSecondary: '#6b6560' },
    dark: { background: '#1a1819', chrome: '#201e20', paper: '#28262b', primary: '#a9a29c', textPrimary: '#e8e4e0', textSecondary: '#b0a8a2' },
  },
]

const BASE = {
  shape: { borderRadius: 10 },
  typography: {
    fontFamily: '"Sora", system-ui, -apple-system, sans-serif',
    fontSize: 13,
    button: { textTransform: 'none' as const, fontWeight: 500 },
  },
}

export function buildTheme(mode: 'light' | 'dark', paletteId: string): Theme {
  const palette = PALETTES.find((p) => p.id === paletteId) ?? PALETTES[0]
  const colors = mode === 'dark' ? palette.dark : palette.light
  return createTheme({
    ...BASE,
    palette: {
      mode,
      background: { default: colors.background, chrome: colors.chrome, paper: colors.paper },
      primary: { main: colors.primary },
      text: { primary: colors.textPrimary, secondary: colors.textSecondary },
      divider: mode === 'dark' ? 'rgba(255,255,255,0.09)' : 'rgba(0,0,0,0.12)',
    },
    components: {
      // A desktop app is a single long-lived window, not a document — it should
      // never rubber-band or show a page scrollbar at the root.
      MuiCssBaseline: {
        styleOverrides: {
          'html, body, #root': {
            height: '100%',
            overflow: 'hidden',
            margin: 0,
          },
          '*': {
            scrollbarWidth: 'thin',
            scrollbarColor: mode === 'dark' ? 'rgba(255,255,255,0.18) transparent' : 'rgba(0,0,0,0.25) transparent',
          },
          '*::-webkit-scrollbar': { width: 8, height: 8 },
          '*::-webkit-scrollbar-track': { background: 'transparent' },
          '*::-webkit-scrollbar-thumb': {
            backgroundColor: mode === 'dark' ? 'rgba(255,255,255,0.14)' : 'rgba(0,0,0,0.2)',
            borderRadius: 8,
            border: '2px solid transparent',
            backgroundClip: 'padding-box',
          },
          '*::-webkit-scrollbar-thumb:hover': {
            backgroundColor: mode === 'dark' ? 'rgba(255,255,255,0.24)' : 'rgba(0,0,0,0.32)',
            border: '1px solid transparent',
            backgroundClip: 'padding-box',
          },
        },
      },
    },
  })
}

/** Default export kept for any consumer that hasn't adopted the store yet. */
export const theme = buildTheme('dark', 'vivianne')

/**
 * The one glass-surface treatment used by every piece of floating/docked
 * chrome — sidebar, dock, composer, settings dialog. Originally hand-rolled
 * separately in the composer and the dock with identical dark-only rgba
 * values; centralized here so the whole app reads as one surface language
 * instead of a composer that happens to look like the dock. Mode-aware
 * (the old hardcoded white-tinted values were invisible against a light
 * background) — dark mode lightens with translucent white, light mode
 * lightens with translucent white too but shifts the shadow and border to
 * dark-tinted so it still reads as glass rather than a flat white card.
 */
/** Corner radius for every floating chrome surface — the sidebar, both docks,
 * and the Settings dialog. One constant because these sit next to each other
 * on screen: a card at 30px beside one at 12px reads as two unrelated
 * components rather than one system. Deliberately a raw px value, not a
 * `theme.shape.borderRadius` multiple — `borderRadius: 3` against this
 * theme's base of 10 silently means 30px, which is what made these look
 * like blobs. */
export const SURFACE_RADIUS = 12

/** The full card treatment: glass fill plus the shared radius. Prefer this
 * over hand-pairing `glassSurface` with a local radius. */
export function surfaceCard(mode: 'light' | 'dark') {
  return { ...glassSurface(mode), borderRadius: `${SURFACE_RADIUS}px` } as const
}

export function glassSurface(mode: 'light' | 'dark') {
  const dark = mode === 'dark'
  return {
    bgcolor: dark ? 'rgba(255,255,255,0.045)' : 'rgba(255,255,255,0.6)',
    backdropFilter: 'blur(20px) saturate(150%)',
    WebkitBackdropFilter: 'blur(20px) saturate(150%)',
    border: '1px solid',
    borderColor: dark ? 'rgba(255,255,255,0.08)' : 'rgba(20,23,28,0.08)',
    boxShadow: dark
      ? '0 12px 32px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.06)'
      : '0 12px 32px rgba(20,23,28,0.12), inset 0 1px 0 rgba(255,255,255,0.7)',
  } as const
}
