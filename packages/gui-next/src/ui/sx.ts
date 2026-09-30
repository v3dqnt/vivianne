import type { Breakpoint, Theme } from './theme'

/**
 * The `sx` prop: inline, theme-aware style objects.
 *
 * Kept from the MUI era because it is how the whole app is styled (~1,400
 * call sites) and it is a good fit for a component kit built on unstyled
 * Radix primitives. The rules are the familiar ones:
 *
 * - spacing keys (`p`, `px`, `mt`, `gap`…) take multiples of 8px
 * - color keys take palette paths (`'text.secondary'`, `'primary.main'`,
 *   `'divider'`) or any CSS color
 * - `border: 1` is `1px solid`; `borderRadius: 2` is 2× the theme radius;
 *   `boxShadow: 4` is elevation 4
 * - sizes in (0, 1] are fractions (`width: 0.5` is 50%)
 * - `{ xs, sm, md, lg, xl }` objects are responsive values
 * - any value, or the whole object, may be a function of the theme
 * - nested selectors (`'&:hover'`, `'& .x'`, `svg`) nest
 *
 * The output is an Emotion style object.
 */

/** A style value: a CSS value, a nested style object, a responsive array,
 * or a function of the theme returning any of those. */
export type SxValue = string | number | boolean | null | undefined | SxObject | ReadonlyArray<SxValue> | ((theme: Theme) => SxValue)
export interface SxObject {
  [key: string]: SxValue
}
type SxEntry = SxObject | ((theme: Theme) => SxObject) | boolean | null | undefined
export type SxProps = SxObject | ((theme: Theme) => SxObject) | ReadonlyArray<SxEntry | ReadonlyArray<SxEntry>>

const SPACING: Record<string, string[]> = {
  m: ['margin'],
  mt: ['marginTop'],
  mr: ['marginRight'],
  mb: ['marginBottom'],
  ml: ['marginLeft'],
  mx: ['marginLeft', 'marginRight'],
  my: ['marginTop', 'marginBottom'],
  p: ['padding'],
  pt: ['paddingTop'],
  pr: ['paddingRight'],
  pb: ['paddingBottom'],
  pl: ['paddingLeft'],
  px: ['paddingLeft', 'paddingRight'],
  py: ['paddingTop', 'paddingBottom'],
  margin: ['margin'],
  marginTop: ['marginTop'],
  marginRight: ['marginRight'],
  marginBottom: ['marginBottom'],
  marginLeft: ['marginLeft'],
  marginX: ['marginLeft', 'marginRight'],
  marginY: ['marginTop', 'marginBottom'],
  marginInline: ['marginInline'],
  marginBlock: ['marginBlock'],
  padding: ['padding'],
  paddingTop: ['paddingTop'],
  paddingRight: ['paddingRight'],
  paddingBottom: ['paddingBottom'],
  paddingLeft: ['paddingLeft'],
  paddingX: ['paddingLeft', 'paddingRight'],
  paddingY: ['paddingTop', 'paddingBottom'],
  paddingInline: ['paddingInline'],
  paddingBlock: ['paddingBlock'],
  gap: ['gap'],
  rowGap: ['rowGap'],
  columnGap: ['columnGap'],
}

const SIZING = new Set(['width', 'height', 'minWidth', 'maxWidth', 'minHeight', 'maxHeight'])
const COLORS = new Set([
  'color',
  'backgroundColor',
  'borderColor',
  'borderTopColor',
  'borderRightColor',
  'borderBottomColor',
  'borderLeftColor',
  'outlineColor',
])
const BORDERS = new Set(['border', 'borderTop', 'borderRight', 'borderBottom', 'borderLeft', 'outline'])
const BREAKPOINTS: Breakpoint[] = ['xs', 'sm', 'md', 'lg', 'xl']
const FONT_WEIGHTS: Record<string, number> = { fontWeightLight: 300, fontWeightRegular: 400, fontWeightMedium: 500, fontWeightBold: 700 }

const isPlainObject = (v: unknown): v is SxObject => typeof v === 'object' && v !== null && !Array.isArray(v)
const isSelector = (key: string) => /^[&@:.>#[*+~]/.test(key) || key.includes('&') || key.includes(' ') || key.includes(',')
const isResponsive = (v: SxObject) => {
  const keys = Object.keys(v)
  return keys.length > 0 && keys.every((k) => (BREAKPOINTS as string[]).includes(k))
}

/** Look a palette path up (`'text.secondary'`, `'grey.500'`); anything that
 * isn't one is returned as-is. */
export function paletteColor(theme: Theme, value: SxValue): SxValue {
  if (typeof value !== 'string' || !/^[a-zA-Z]+(\.[a-zA-Z0-9]+)*$/.test(value)) return value
  // biome-ignore lint/suspicious/noExplicitAny: walking an arbitrary path
  let cur: any = theme.palette
  for (const part of value.split('.')) {
    if (cur == null || typeof cur !== 'object' || !(part in cur)) return value
    cur = cur[part]
  }
  if (typeof cur === 'string') return cur
  if (isPlainObject(cur) && typeof cur.main === 'string') return cur.main
  return value
}

function spacing(value: SxValue): SxValue {
  return typeof value === 'number' ? `${value * 8}px` : value
}

/** One property after theme mapping. Returns style entries to merge. */
function property(key: string, value: SxValue, theme: Theme): SxObject {
  if (key === 'typography') {
    return typeof value === 'string' && value in theme.typography ? { ...(theme.typography[value as keyof Theme['typography']] as unknown as SxObject) } : {}
  }
  if (key === 'bgcolor') key = 'backgroundColor'
  if (key in SPACING) {
    const v = spacing(value)
    return Object.fromEntries(SPACING[key].map((k) => [k, v])) as SxObject
  }
  if (SIZING.has(key)) {
    if (typeof value === 'number') return { [key]: value > 0 && value <= 1 ? `${value * 100}%` : value }
    if (key === 'maxWidth' && typeof value === 'string' && value in theme.breakpoints.values) {
      return { [key]: theme.breakpoints.values[value as Breakpoint] }
    }
    return { [key]: value }
  }
  if (COLORS.has(key)) return { [key]: paletteColor(theme, value) }
  if (BORDERS.has(key)) return { [key]: typeof value === 'number' ? `${value}px solid` : value }
  if (key === 'borderRadius') return { [key]: typeof value === 'number' ? value * theme.shape.borderRadius : value }
  if (key === 'boxShadow' && typeof value === 'number') return { [key]: theme.shadows[value] ?? 'none' }
  if (key === 'zIndex' && typeof value === 'string' && value in theme.zIndex) {
    return { [key]: theme.zIndex[value as keyof Theme['zIndex']] }
  }
  if (key === 'fontWeight') {
    // Terax uses two weights: 400 and 500. Anything heavier is capped, so
    // emphasis comes from weight 500 and tone, never bold.
    const w = typeof value === 'string' ? (FONT_WEIGHTS[value] ?? (value === 'bold' ? 700 : Number(value))) : value
    return { [key]: typeof w === 'number' && w > 500 ? 500 : (w ?? value) }
  }
  if (key === 'fontFamily' && value === 'monospace') return { [key]: '"Geist Mono", ui-monospace, monospace' }
  return { [key]: value }
}

function deepMerge(target: SxObject, source: SxObject): SxObject {
  for (const [k, v] of Object.entries(source)) {
    if (isPlainObject(v) && isPlainObject(target[k])) target[k] = deepMerge({ ...target[k] }, v)
    else target[k] = v
  }
  return target
}

function resolveObject(obj: SxObject, theme: Theme): SxObject {
  const out: SxObject = {}
  for (const [key, raw] of Object.entries(obj)) {
    let value = typeof raw === 'function' ? raw(theme) : raw
    if (value === undefined || value === null || value === false) continue
    if (isSelector(key)) {
      deepMerge(out, { [key]: resolve(value, theme) })
      continue
    }
    if (Array.isArray(value)) {
      value = Object.fromEntries(value.map((v, i) => [BREAKPOINTS[i], v]).filter(([bp, v]) => bp && v != null))
    }
    if (isPlainObject(value)) {
      if (isResponsive(value)) {
        for (const bp of BREAKPOINTS) {
          const v = value[bp]
          if (v === undefined || v === null) continue
          const entry = property(key, typeof v === 'function' ? v(theme) : (v as SxValue), theme)
          if (bp === 'xs') deepMerge(out, entry)
          else deepMerge(out, { [theme.breakpoints.up(bp)]: entry })
        }
      } else {
        // A bare key with an object value is a nested selector (`svg: {…}`).
        deepMerge(out, { [key]: resolveObject(value, theme) })
      }
      continue
    }
    deepMerge(out, property(key, value, theme))
  }
  return out
}

/** Resolve an `sx` value (object, function, or array of either) against the
 * theme into a plain Emotion style object. */
export function resolve(sx: unknown, theme: Theme): SxObject {
  if (!sx) return {}
  if (typeof sx === 'function') return resolve(sx(theme), theme)
  if (Array.isArray(sx)) {
    const out: SxObject = {}
    for (const part of sx) deepMerge(out, resolve(part, theme))
    return out
  }
  if (isPlainObject(sx)) return resolveObject(sx, theme)
  return {}
}
