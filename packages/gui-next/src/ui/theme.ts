import { alpha, darken, getContrastRatio, lighten } from './color'

/**
 * The theme object every component styles from.
 *
 * The visual language is Terax's: quiet, monochrome pro-tool chrome.
 * Neutral greys from near-black to near-white, "primary" meaning the ink
 * colour, hierarchy from tone steps and weight 500 vs 400, 9% hairlines,
 * floating panes on a frame, and pill-shaped controls. Colour only enters
 * through a theme preset, and then only as `primary` and `ring`.
 *
 * The object keeps the palette paths the app's `sx` props already use
 * (`'text.secondary'`, `'action.hover'`, `'background.chrome'`, `'divider'`)
 * and maps them onto the Terax tokens, plus the tokens themselves under
 * `palette.tokens` for components that want them by name.
 */

export type Mode = 'light' | 'dark'

export interface PaletteColor {
  main: string
  light: string
  dark: string
  contrastText: string
}

export type ColorName = 'primary' | 'secondary' | 'error' | 'warning' | 'info' | 'success'

/** The Terax token set (shadcn naming). */
export interface Tokens {
  background: string
  foreground: string
  card: string
  popover: string
  primary: string
  primaryForeground: string
  secondary: string
  muted: string
  mutedForeground: string
  accent: string
  destructive: string
  border: string
  input: string
  ring: string
  /** The window background around the panes: one tone off `card`. */
  frame: string
}

export interface Palette extends Record<ColorName, PaletteColor> {
  mode: Mode
  tokens: Tokens
  common: { black: string; white: string }
  grey: Record<50 | 100 | 200 | 300 | 400 | 500 | 600 | 700 | 800 | 900 | 'A100' | 'A200' | 'A400' | 'A700', string>
  text: { primary: string; secondary: string; disabled: string }
  divider: string
  background: {
    /** The frame: window background, behind the panes. */
    default: string
    /** Floating layers: menus, popovers, dialogs. */
    paper: string
    /** Shell chrome that sits on the frame (rail, dialog side-nav). */
    chrome: string
    /** A pane: the working surfaces (sidebar, panels). */
    pane: string
    /** Cards inside a pane. */
    card: string
  }
  action: {
    active: string
    hover: string
    hoverOpacity: number
    selected: string
    selectedOpacity: number
    disabled: string
    disabledBackground: string
    disabledOpacity: number
    focus: string
    focusOpacity: number
    activatedOpacity: number
  }
  getContrastText: (background: string) => string
}

export type TypographyVariant =
  | 'h1'
  | 'h2'
  | 'h3'
  | 'h4'
  | 'h5'
  | 'h6'
  | 'subtitle1'
  | 'subtitle2'
  | 'body1'
  | 'body2'
  | 'button'
  | 'caption'
  | 'overline'

/** One step of the type scale. */
export interface TypeStyle {
  fontFamily: string
  fontWeight: number
  fontSize: string
  lineHeight: number
  textTransform?: 'none' | 'uppercase'
  letterSpacing?: string
}

export interface Typography extends Record<TypographyVariant, TypeStyle> {
  fontFamily: string
  monoFamily: string
  fontSize: number
  pxToRem: (px: number) => string
}

export interface Theme {
  palette: Palette
  typography: Typography
  /** `borderRadius` is the base radius (10px); `sx` radius numbers multiply it. */
  shape: { borderRadius: number }
  /** Terax's radius scale, derived from the base. */
  radius: { sm: number; md: number; lg: number; xl: number; '2xl': number; '3xl': number; '4xl': number; full: number }
  spacing: (...values: Array<number | string>) => string
  shadows: string[]
  /** Soft shadows by role, plus the 1px ring floating layers wear. */
  elevation: { sm: string; md: string; lg: string; xl: string; ring: string; pane: string }
  zIndex: Record<'appBar' | 'drawer' | 'modal' | 'snackbar' | 'tooltip' | 'popover', number>
  breakpoints: { values: Record<Breakpoint, number>; up: (key: Breakpoint) => string }
  transitions: {
    easing: Record<'easeInOut' | 'easeOut' | 'easeIn' | 'sharp' | 'premium' | 'soft', string>
    duration: Record<'shortest' | 'shorter' | 'short' | 'standard' | 'fast' | 'base' | 'slow', number>
  }
}

export type Breakpoint = 'xs' | 'sm' | 'md' | 'lg' | 'xl'

const GREY = {
  50: '#fafafa',
  100: '#f5f5f5',
  200: '#eeeeee',
  300: '#e0e0e0',
  400: '#bdbdbd',
  500: '#9e9e9e',
  600: '#757575',
  700: '#616161',
  800: '#424242',
  900: '#212121',
  A100: '#f5f5f5',
  A200: '#eeeeee',
  A400: '#bdbdbd',
  A700: '#616161',
} as const

/** Status colours, from Terax's terminal palette — the only colours the
 * default theme has. */
const STATUS: Record<Mode, Record<'success' | 'warning' | 'info', { main: string; light: string }>> = {
  dark: {
    success: { main: '#6ec48f', light: '#85d9a4' },
    warning: { main: '#e0b34d', light: '#eec469' },
    info: { main: '#6ea8e8', light: '#8dbcf2' },
  },
  light: {
    success: { main: '#2f7d4f', light: '#3d9c64' },
    warning: { main: '#a37413', light: '#bf8a1f' },
    info: { main: '#2f6bb5', light: '#3f82d4' },
  },
}

/** A legacy elevation scale for `boxShadow: <n>` in `sx`: soft, low shadows
 * rather than Material's, since Terax separates with borders and tone. */
export const SHADOWS = Array.from({ length: 25 }, (_, i) =>
  i === 0 ? 'none' : `0 ${Math.round(1 + i * 0.6)}px ${Math.round(2 + i * 1.6)}px rgba(0,0,0,${Math.min(0.08 + i * 0.012, 0.32).toFixed(3)})`,
)

export interface ThemeOptions {
  mode: Mode
  tokens: Omit<Tokens, 'frame'>
  fontFamily: string
  monoFamily: string
  /** Base radius in px (Terax: 10). */
  radius?: number
}

/** `color-mix(in oklab, card 97%, foreground 3%)`, precomputed — the frame
 * is always derived, never set, so every theme gets the pane separation. */
function frameOf(card: string, foreground: string): string {
  const mix = (a: string, b: string) => {
    const parse = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16))
    if (!/^#[0-9a-f]{6}$/i.test(a) || !/^#[0-9a-f]{6}$/i.test(b)) return `color-mix(in oklab, ${a} 97%, ${b} 3%)`
    const [x, y] = [parse(a), parse(b)]
    return `#${x.map((v, i) => Math.round(v * 0.97 + y[i] * 0.03).toString(16).padStart(2, '0')).join('')}`
  }
  return mix(card, foreground)
}

export function createTheme({ mode, tokens: t0, fontFamily, monoFamily, radius = 10 }: ThemeOptions): Theme {
  const dark = mode === 'dark'
  const tokens: Tokens = { ...t0, frame: frameOf(t0.card, t0.foreground) }
  const fg = tokens.foreground
  const getContrastText = (bg: string) => (getContrastRatio(bg, tokens.background) >= getContrastRatio(bg, fg) ? tokens.background : fg)
  const color = (main: string, light?: string, contrastText?: string): PaletteColor => ({
    main,
    light: light ?? lighten(main, 0.2),
    dark: darken(main, dark ? 0.15 : 0.2),
    contrastText: contrastText ?? getContrastText(main),
  })
  const hoverOpacity = dark ? 0.07 : 0.05
  const selectedOpacity = dark ? 0.1 : 0.08
  const status = STATUS[mode]

  const palette: Palette = {
    mode,
    tokens,
    common: { black: '#000', white: '#fff' },
    grey: GREY,
    primary: color(tokens.primary, undefined, tokens.primaryForeground),
    secondary: color(tokens.secondary, undefined, fg),
    error: color(tokens.destructive, lighten(tokens.destructive, 0.2), '#fff'),
    warning: color(status.warning.main, status.warning.light),
    info: color(status.info.main, status.info.light),
    success: color(status.success.main, status.success.light),
    text: { primary: fg, secondary: tokens.mutedForeground, disabled: alpha(fg, dark ? 0.4 : 0.38) },
    divider: tokens.border,
    background: { default: tokens.frame, paper: tokens.popover, chrome: tokens.frame, pane: tokens.background, card: tokens.card },
    action: {
      active: tokens.mutedForeground,
      hover: alpha(fg, hoverOpacity),
      hoverOpacity,
      selected: alpha(fg, selectedOpacity),
      selectedOpacity,
      disabled: alpha(fg, 0.3),
      disabledBackground: alpha(fg, 0.08),
      disabledOpacity: 0.5,
      focus: alpha(tokens.ring, 0.3),
      focusOpacity: 0.12,
      activatedOpacity: 0.12,
    },
    getContrastText,
  }

  // Terax's type scale: dense chrome, 14px controls, 16px titles, and only
  // two weights — 400 and 500. No bold.
  const pxToRem = (px: number) => `${px / 16}rem`
  const v = (weight: number, size: number, lineHeight: number, extra?: Pick<TypeStyle, 'textTransform' | 'letterSpacing'>): TypeStyle => ({
    fontFamily,
    fontWeight: weight,
    fontSize: pxToRem(size),
    lineHeight,
    ...extra,
  })
  const typography: Typography = {
    fontFamily,
    monoFamily,
    fontSize: 13,
    pxToRem,
    h1: v(500, 32, 1.2),
    h2: v(500, 26, 1.25),
    h3: v(500, 22, 1.3),
    h4: v(500, 20, 1.3),
    h5: v(500, 18, 1.35),
    h6: v(500, 16, 1.35),
    subtitle1: v(500, 15, 1.45),
    subtitle2: v(500, 13, 1.45),
    body1: v(400, 14, 1.55),
    body2: v(400, 13, 1.5),
    button: v(500, 14, 1.25, { textTransform: 'none' }),
    caption: v(400, 12, 1.45),
    overline: v(500, 10.5, 1.6, { textTransform: 'uppercase', letterSpacing: '0.06em' }),
  }

  const shadowAlpha = dark ? 0.45 : 0.1
  const bp: Record<Breakpoint, number> = { xs: 0, sm: 600, md: 900, lg: 1200, xl: 1536 }
  return {
    palette,
    typography,
    shape: { borderRadius: radius },
    radius: {
      sm: radius * 0.6,
      md: radius * 0.8,
      lg: radius,
      xl: radius * 1.4,
      '2xl': radius * 1.8,
      '3xl': radius * 2.2,
      '4xl': radius * 2.6,
      full: 9999,
    },
    spacing: (...values) => (values.length ? values : [1]).map((n) => (typeof n === 'number' ? `${n * 8}px` : n)).join(' '),
    shadows: SHADOWS,
    elevation: {
      sm: `0 1px 2px rgba(0,0,0,${dark ? 0.3 : 0.06})`,
      md: `0 4px 12px -2px rgba(0,0,0,${shadowAlpha * 0.8}), 0 2px 4px -2px rgba(0,0,0,${shadowAlpha * 0.5})`,
      lg: `0 12px 28px -6px rgba(0,0,0,${shadowAlpha}), 0 4px 8px -4px rgba(0,0,0,${shadowAlpha * 0.6})`,
      xl: `0 24px 56px -12px rgba(0,0,0,${shadowAlpha * 1.3}), 0 8px 16px -8px rgba(0,0,0,${shadowAlpha * 0.6})`,
      ring: `0 0 0 1px ${alpha(fg, dark ? 0.1 : 0.05)}`,
      pane: dark ? 'inset 0 1px 0 rgba(255, 255, 255, 0.035)' : 'none',
    },
    zIndex: { appBar: 1100, drawer: 1200, modal: 1300, popover: 1300, snackbar: 1400, tooltip: 1500 },
    breakpoints: { values: bp, up: (key) => `@media (min-width:${bp[key]}px)` },
    transitions: {
      easing: {
        easeInOut: 'cubic-bezier(0.4, 0, 0.2, 1)',
        easeOut: 'cubic-bezier(0.0, 0, 0.2, 1)',
        easeIn: 'cubic-bezier(0.4, 0, 1, 1)',
        sharp: 'cubic-bezier(0.4, 0, 0.6, 1)',
        premium: 'cubic-bezier(0.16, 1, 0.3, 1)',
        soft: 'cubic-bezier(0.4, 0, 0.2, 1)',
      },
      duration: { shortest: 150, shorter: 200, short: 250, standard: 300, fast: 160, base: 240, slow: 320 },
    },
  }
}
