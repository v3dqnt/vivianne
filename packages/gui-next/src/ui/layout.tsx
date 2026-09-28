import { forwardRef, type AllHTMLAttributes, type ElementType, type ReactNode } from 'react'
import { jsx } from '@emotion/react'
import { Separator } from 'radix-ui'
import { useTheme } from './ThemeProvider'
import { paletteColor, resolve, type SxObject, type SxProps } from './sx'
import type { Breakpoint, Theme, TypographyVariant } from './theme'

// biome-ignore lint/suspicious/noExplicitAny: polymorphic element props
type Any = any

/** Props every kit component accepts: any DOM attribute, `sx`, and
 * `component` to change the rendered element. */
export type BoxProps = Omit<AllHTMLAttributes<HTMLElement>, 'color' | 'size' | 'wrap' | 'ref'> & {
  component?: ElementType
  sx?: SxProps
  children?: ReactNode
  color?: string
  /** For `component="svg"`. */
  viewBox?: string
}

/** Render `component` with the kit's own styles followed by the caller's
 * `sx` (which wins). */
export function render(component: ElementType, props: Record<string, unknown>, base: SxObject | SxObject[], sx: SxProps | undefined, theme: Theme) {
  const css = [...(Array.isArray(base) ? base : [base]), resolve(sx, theme)]
  return jsx(component, { ...props, css })
}

export const Box = forwardRef<Any, BoxProps>(function Box({ component = 'div', sx, ...rest }, ref) {
  const theme = useTheme()
  return render(component, { ref, ...rest }, [], sx, theme)
})

type Responsive<T> = T | Partial<Record<Breakpoint, T>> | T[]
type Direction = 'row' | 'row-reverse' | 'column' | 'column-reverse'

const BPS: Breakpoint[] = ['xs', 'sm', 'md', 'lg', 'xl']
function perBreakpoint<T>(v: Responsive<T> | undefined): Partial<Record<Breakpoint, T>> {
  if (v === undefined) return {}
  if (Array.isArray(v)) return Object.fromEntries(v.map((x, i) => [BPS[i], x]))
  if (typeof v === 'object' && v !== null && Object.keys(v).every((k) => (BPS as string[]).includes(k))) return v as Partial<Record<Breakpoint, T>>
  return { xs: v as T }
}

const MARGIN_SIDE: Record<Direction, string> = { row: 'marginLeft', 'row-reverse': 'marginRight', column: 'marginTop', 'column-reverse': 'marginBottom' }

export type StackProps = BoxProps & {
  direction?: Responsive<Direction>
  spacing?: Responsive<number | string>
}

/**
 * A flex column (or row). `spacing` separates children with margins on every
 * child after the first, so it survives children that are conditionally
 * rendered or `display: none`.
 */
export const Stack = forwardRef<Any, StackProps>(function Stack({ component = 'div', direction = 'column', spacing, sx, ...rest }, ref) {
  const theme = useTheme()
  const dirs = perBreakpoint(direction)
  const gaps = perBreakpoint(spacing)
  const base: SxObject = { display: 'flex' }
  let dir: Direction = 'column'
  let gap: number | string | undefined
  for (const bp of BPS) {
    if (!(bp in dirs) && !(bp in gaps)) continue
    dir = dirs[bp] ?? dir
    gap = gaps[bp] ?? gap
    const styles: SxObject = { flexDirection: dir }
    if (gap !== undefined) {
      styles['& > :not(style):not(style)'] = { margin: 0 }
      styles['& > :not(style) ~ :not(style)'] = { [MARGIN_SIDE[dir]]: typeof gap === 'number' ? `${gap * 8}px` : gap }
    }
    if (bp === 'xs') Object.assign(base, styles)
    else base[theme.breakpoints.up(bp)] = styles
  }
  return render(component, { ref, ...rest }, base, sx, theme)
})

const VARIANT_ELEMENT: Record<string, ElementType> = {
  h1: 'h1',
  h2: 'h2',
  h3: 'h3',
  h4: 'h4',
  h5: 'h5',
  h6: 'h6',
  subtitle1: 'h6',
  subtitle2: 'h6',
  body1: 'p',
  body2: 'p',
  caption: 'span',
  overline: 'span',
  button: 'span',
  inherit: 'p',
}

export type TypographyProps = BoxProps & {
  variant?: TypographyVariant | 'inherit'
  /** A palette path (`'text.secondary'`) or any CSS color. */
  color?: string
  noWrap?: boolean
  gutterBottom?: boolean
  align?: 'inherit' | 'left' | 'center' | 'right' | 'justify'
}

export const Typography = forwardRef<Any, TypographyProps>(function Typography(
  { variant = 'body1', component, color, noWrap, gutterBottom, align, sx, ...rest },
  ref,
) {
  const theme = useTheme()
  const base: SxObject = {
    margin: 0,
    ...(variant === 'inherit' ? { font: 'inherit' } : theme.typography[variant]),
    ...(color ? { color: paletteColor(theme, color) } : null),
    ...(align && align !== 'inherit' ? { textAlign: align } : null),
    ...(noWrap ? { overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } : null),
    ...(gutterBottom ? { marginBottom: '0.35em' } : null),
  }
  return render(component ?? VARIANT_ELEMENT[variant] ?? 'span', { ref, ...rest }, base, sx, theme)
})

export type PaperProps = BoxProps & { elevation?: number; square?: boolean; variant?: 'elevation' | 'outlined' }

/** A raised surface: a card (`elevation` 0-1) or a floating layer (2+),
 * wearing a 1px ring instead of a border, and a soft shadow by role. */
export function paperStyles(theme: Theme, elevation: number, opts?: { square?: boolean; outlined?: boolean }): SxObject {
  const p = theme.palette
  const floating = elevation >= 2
  const shadow = elevation <= 0 ? 'none' : elevation < 4 ? theme.elevation.md : elevation < 16 ? theme.elevation.lg : theme.elevation.xl
  return {
    backgroundColor: floating ? p.background.paper : p.background.card,
    color: p.text.primary,
    borderRadius: opts?.square ? 0 : theme.radius['3xl'],
    ...(opts?.outlined ? { border: `1px solid ${p.divider}` } : { boxShadow: [theme.elevation.ring, shadow].filter((x) => x !== 'none').join(', ') }),
  }
}

export const Paper = forwardRef<Any, PaperProps>(function Paper({ component = 'div', elevation = 1, square, variant, sx, ...rest }, ref) {
  const theme = useTheme()
  return render(component, { ref, ...rest }, paperStyles(theme, elevation, { square, outlined: variant === 'outlined' }), sx, theme)
})

export type DividerProps = BoxProps & { orientation?: 'horizontal' | 'vertical'; flexItem?: boolean }

export const Divider = forwardRef<Any, DividerProps>(function Divider({ orientation = 'horizontal', flexItem, sx, component, ...rest }, ref) {
  const theme = useTheme()
  const vertical = orientation === 'vertical'
  const base: SxObject = {
    margin: 0,
    flexShrink: 0,
    borderWidth: 0,
    borderStyle: 'solid',
    borderColor: theme.palette.divider,
    ...(vertical ? { height: flexItem ? 'auto' : '100%', borderRightWidth: 'thin', alignSelf: flexItem ? 'stretch' : undefined } : { borderBottomWidth: 'thin' }),
  }
  return (
    <Separator.Root decorative orientation={orientation} asChild>
      {render(component ?? (vertical ? 'div' : 'hr'), { ref, ...rest }, base, sx, theme)}
    </Separator.Root>
  )
})

export type LinkProps = TypographyProps & { href?: string; target?: string; rel?: string; underline?: 'always' | 'hover' | 'none' }

export const Link = forwardRef<Any, LinkProps>(function Link({ underline = 'hover', color = 'primary.main', variant = 'inherit', sx, ...rest }, ref) {
  // Links underline on hover only, offset clear of descenders.
  const decoration: SxObject =
    underline === 'none'
      ? { textDecoration: 'none' }
      : underline === 'always'
        ? { textDecoration: 'underline', textUnderlineOffset: 4 }
        : { textDecoration: 'none', textUnderlineOffset: 4, '&:hover': { textDecoration: 'underline' } }
  return <Typography ref={ref} component="a" variant={variant} color={color} sx={[decoration, ...(Array.isArray(sx) ? sx : [sx])]} {...rest} />
})
