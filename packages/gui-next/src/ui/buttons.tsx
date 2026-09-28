import { createContext, forwardRef, useContext, useRef, type ReactNode } from 'react'
import type React from 'react'
import { ToggleGroup as RadixToggleGroup } from 'radix-ui'
import { useTheme } from './ThemeProvider'
import { render, type BoxProps } from './layout'
import type { SxObject } from './sx'
import { alpha, fade } from './color'
import type { ColorName, Theme } from './theme'

// biome-ignore lint/suspicious/noExplicitAny: polymorphic element props
type Any = any

/** Keyboard focus: Terax's `border-ring` + 3px `ring/30`, drawn as a
 * shadow so it never shifts layout. */
export function focusRing(theme: Theme): SxObject {
  return { '&:focus-visible': { outline: 'none', boxShadow: `0 0 0 3px ${alpha(theme.palette.tokens.ring, 0.3)}` } }
}

const RESET: SxObject = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  position: 'relative',
  boxSizing: 'border-box',
  WebkitTapHighlightColor: 'transparent',
  backgroundColor: 'transparent',
  outline: 0,
  border: 0,
  margin: 0,
  borderRadius: 0,
  padding: 0,
  cursor: 'pointer',
  userSelect: 'none',
  verticalAlign: 'middle',
  MozAppearance: 'none',
  WebkitAppearance: 'none',
  textDecoration: 'none',
  color: 'inherit',
  font: 'inherit',
  '&::-moz-focus-inner': { borderStyle: 'none' },
  '&:disabled, &[aria-disabled="true"]': { pointerEvents: 'none', cursor: 'default' },
}

export type ButtonBaseProps = Omit<BoxProps, 'type'> & {
  disabled?: boolean
  type?: 'button' | 'submit' | 'reset'
  /** Styles from a kit component built on ButtonBase. Plain CSS — unlike
   * `sx`, numbers are pixels, not spacing or radius units. */
  baseStyles?: Array<SxObject | null | undefined>
}

/** Native buttons get `type` and `disabled`; anything else rendered as a
 * button gets the role, focusability and Enter/Space activation a native
 * one has. */
function buttonProps(component: unknown, disabled: boolean | undefined, type: string | undefined, rest: Record<string, unknown>) {
  if (component === 'button') return { type: type ?? 'button', disabled, ...rest }
  const onKeyDown = rest.onKeyDown as ((e: React.KeyboardEvent<HTMLElement>) => void) | undefined
  const onClick = rest.onClick as ((e: React.MouseEvent<HTMLElement>) => void) | undefined
  return {
    role: 'button',
    ...rest,
    tabIndex: disabled ? -1 : ((rest.tabIndex as number | undefined) ?? 0),
    ...(disabled ? { 'aria-disabled': true } : {}),
    onKeyDown: (e: React.KeyboardEvent<HTMLElement>) => {
      onKeyDown?.(e)
      if (e.defaultPrevented || disabled || e.target !== e.currentTarget) return
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault()
        onClick?.(e as unknown as React.MouseEvent<HTMLElement>)
      }
    },
  }
}

/** An unstyled, accessible button. */
export const ButtonBase = forwardRef<Any, ButtonBaseProps>(function ButtonBase({ component = 'button', disabled, type, baseStyles, sx, className, ...rest }, ref) {
  const theme = useTheme()
  return render(
    component,
    { ref, className: ['ui-ButtonBase', className].filter(Boolean).join(' '), ...buttonProps(component, disabled, type, rest) },
    [RESET, focusRing(theme), ...((baseStyles ?? []).filter(Boolean) as SxObject[])],
    sx,
    theme,
  )
})

type ButtonColor = ColorName | 'inherit'
type ButtonVariant = 'text' | 'outlined' | 'contained'
type ButtonSize = 'small' | 'medium' | 'large'

const ButtonGroupContext = createContext<{ size?: ButtonSize; grouped: SxObject } | null>(null)

export type ButtonProps = ButtonBaseProps & {
  /** `contained` = solid, `outlined` = hairline, `text` = ghost. */
  variant?: ButtonVariant
  color?: ButtonColor
  size?: ButtonSize
  startIcon?: ReactNode
  endIcon?: ReactNode
  /** Accepted for compatibility; buttons are always flat. */
  disableElevation?: boolean
  fullWidth?: boolean
}

const HEIGHT: Record<ButtonSize, number> = { small: 32, medium: 36, large: 40 }

/**
 * Terax buttons: pills, 14px/500, a 1px press. Solid is the ink colour;
 * a status colour is always a tint, never solid; `inherit` is the quiet
 * secondary.
 */
function buttonStyles(theme: Theme, variant: ButtonVariant, color: ButtonColor, size: ButtonSize, fullWidth: boolean): SxObject {
  const p = theme.palette
  const t = p.tokens
  const dark = p.mode === 'dark'
  const fast = `${theme.transitions.duration.fast}ms ${theme.transitions.easing.soft}`
  const base: SxObject = {
    ...theme.typography.button,
    fontSize: size === 'small' ? theme.typography.pxToRem(13) : theme.typography.button.fontSize,
    height: HEIGHT[size],
    minWidth: 0,
    padding: `0 ${size === 'small' ? 12 : size === 'large' ? 20 : 16}px`,
    gap: 6,
    whiteSpace: 'nowrap',
    borderRadius: theme.radius['4xl'],
    border: '1px solid transparent',
    transition: `background-color ${fast}, color ${fast}, border-color ${fast}, box-shadow ${fast}, transform ${fast}, opacity ${fast}`,
    width: fullWidth ? '100%' : undefined,
    '&:active': { transform: 'translateY(1px)' },
    '&:disabled, &[aria-disabled="true"]': { opacity: 0.5 },
    '& svg': { flexShrink: 0 },
  }
  if (color === 'error' || color === 'warning' || color === 'success' || color === 'info') {
    const c = p[color].main
    return {
      ...base,
      color: c,
      backgroundColor: variant === 'text' ? 'transparent' : alpha(c, dark ? 0.2 : 0.1),
      ...(variant === 'outlined' ? { borderColor: alpha(c, 0.3) } : {}),
      '&:hover': { backgroundColor: alpha(c, dark ? 0.3 : 0.2) },
    }
  }
  const accent = color === 'inherit' ? null : p[color]
  if (variant === 'contained') {
    // `color="inherit"` is the secondary button; primary is the ink.
    const bg = accent ? accent.main : t.secondary
    const fg = accent ? accent.contrastText : t.foreground
    return { ...base, backgroundColor: bg, color: fg, '&:hover': { backgroundColor: alpha(bg, 0.8) } }
  }
  if (variant === 'outlined') {
    return {
      ...base,
      borderColor: t.border,
      backgroundColor: dark ? 'transparent' : t.background,
      color: accent ? t.foreground : 'inherit',
      '&:hover': { backgroundColor: dark ? fade(t.input, 0.3) : t.muted },
    }
  }
  // Ghost. A primary ghost keeps the accent as its text colour — in the
  // monochrome theme that is simply the ink.
  return {
    ...base,
    color: accent ? accent.main : 'inherit',
    '&:hover': { backgroundColor: dark ? alpha(t.foreground, 0.07) : t.muted },
  }
}

function iconSlot(side: 'start' | 'end', size: ButtonSize): SxObject {
  const inset = size === 'small' ? -2 : -4
  return { display: 'inline-flex', ...(side === 'start' ? { marginLeft: inset } : { marginRight: inset }) }
}

/** A solid (`contained`), outlined or ghost (`text`) pill button. */
export const Button = forwardRef<Any, ButtonProps>(function Button(props, ref) {
  const group = useContext(ButtonGroupContext)
  const { variant = 'text', color = 'primary', size = group?.size ?? 'medium', startIcon, endIcon, disableElevation: _flat, fullWidth = false, children, sx, className, ...rest } = props
  const theme = useTheme()
  const styles = buttonStyles(theme, variant, color, size, fullWidth)
  // Inside a segmented ButtonGroup, a `contained` segment is the active one.
  const active = group && variant === 'contained'
  return (
    <ButtonBase
      ref={ref}
      className={['ui-Button', active && 'ui-selected', className].filter(Boolean).join(' ')}
      baseStyles={[styles, group?.grouped]}
      sx={sx}
      {...rest}
    >
      {startIcon && (
        <ButtonIcon className="ui-Button-startIcon" styles={iconSlot('start', size)}>
          {startIcon}
        </ButtonIcon>
      )}
      {children}
      {endIcon && (
        <ButtonIcon className="ui-Button-endIcon" styles={iconSlot('end', size)}>
          {endIcon}
        </ButtonIcon>
      )}
    </ButtonBase>
  )
})

function ButtonIcon({ styles, className, children }: { styles: SxObject; className: string; children: ReactNode }) {
  return render('span', { className, children }, styles, undefined, useTheme())
}

export type ButtonGroupProps = BoxProps & { size?: ButtonSize; variant?: ButtonVariant; color?: ButtonColor }

/**
 * A segmented control: a muted pill track holding pill segments. A
 * `contained` child is the active segment and is raised onto the pane
 * colour; the rest are quiet.
 */
export function ButtonGroup({ size = 'medium', sx, children, variant: _variant, color: _color, ...rest }: ButtonGroupProps) {
  const theme = useTheme()
  const t = theme.palette.tokens
  const dark = theme.palette.mode === 'dark'
  const h = HEIGHT[size]
  const grouped: SxObject = {
    height: h - 6,
    padding: `0 ${size === 'small' ? 10 : 12}px`,
    border: '1px solid transparent',
    backgroundColor: 'transparent',
    color: alpha(t.foreground, 0.6),
    '&:hover': { backgroundColor: 'transparent', color: t.foreground },
    '&.ui-selected, &.ui-selected:hover': {
      backgroundColor: dark ? fade(t.input, 0.3) : t.background,
      borderColor: dark ? t.input : 'transparent',
      color: t.foreground,
      boxShadow: theme.elevation.sm,
    },
  }
  return (
    <ButtonGroupContext.Provider value={{ size, grouped }}>
      {render(
        'div',
        { role: 'group', ...rest, children },
        { display: 'inline-flex', alignItems: 'center', gap: 2, height: h, padding: 3, borderRadius: theme.radius.full, backgroundColor: t.muted },
        sx,
        theme,
      )}
    </ButtonGroupContext.Provider>
  )
}

export type IconButtonProps = ButtonBaseProps & { size?: ButtonSize; color?: ButtonColor | 'default'; edge?: 'start' | 'end' | false }

/** An icon-only button: a small rounded square, muted at rest, ink on
 * hover (Terax's header buttons). */
export const IconButton = forwardRef<Any, IconButtonProps>(function IconButton({ size = 'medium', color = 'default', edge = false, sx, className, ...rest }, ref) {
  const theme = useTheme()
  const p = theme.palette
  const t = p.tokens
  const status = color === 'error' || color === 'warning' || color === 'success' || color === 'info' ? p[color].main : null
  const px = size === 'small' ? 28 : size === 'large' ? 40 : 32
  const styles: SxObject = {
    flex: '0 0 auto',
    width: px,
    height: px,
    fontSize: size === 'small' ? 16 : 18,
    borderRadius: size === 'small' ? theme.radius.md : theme.radius.lg,
    color: color === 'inherit' ? 'inherit' : (status ?? (color === 'primary' ? p.primary.main : t.mutedForeground)),
    transition: `background-color ${theme.transitions.duration.fast}ms, color ${theme.transitions.duration.fast}ms, transform ${theme.transitions.duration.fast}ms`,
    ...(edge === 'start' ? { marginLeft: -6 } : edge === 'end' ? { marginRight: -6 } : {}),
    '&:hover': { backgroundColor: status ? alpha(status, 0.12) : alpha(t.foreground, p.mode === 'dark' ? 0.07 : 0.06), color: status ?? t.foreground },
    '&:active': { transform: 'translateY(1px)' },
    '&:disabled, &[aria-disabled="true"]': { opacity: 0.4 },
  }
  return <ButtonBase ref={ref} className={['ui-IconButton', className].filter(Boolean).join(' ')} baseStyles={[styles]} sx={sx} {...rest} />
})

export type ChipProps = Omit<BoxProps, 'color' | 'type'> & {
  label?: ReactNode
  size?: 'small' | 'medium'
  variant?: 'filled' | 'outlined'
  color?: ColorName | 'default'
  disabled?: boolean
  icon?: ReactNode
}

/** A badge: a small pill label, tinted when coloured. Clickable when given
 * `onClick`. */
export const Chip = forwardRef<Any, ChipProps>(function Chip({ label, size = 'medium', variant = 'filled', color = 'default', disabled, icon, onClick, sx, className, ...rest }, ref) {
  const theme = useTheme()
  const p = theme.palette
  const t = p.tokens
  const small = size === 'small'
  const outlined = variant === 'outlined'
  const c = color === 'default' ? null : p[color].main
  const clickable = typeof onClick === 'function'
  const styles: SxObject = {
    maxWidth: '100%',
    fontFamily: theme.typography.fontFamily,
    fontSize: theme.typography.pxToRem(small ? 11 : 12),
    fontWeight: 500,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 4,
    height: small ? 20 : 24,
    lineHeight: 1,
    padding: `0 ${small ? 7 : 9}px`,
    color: c ?? (outlined ? alpha(t.foreground, 0.75) : t.foreground),
    backgroundColor: outlined ? alpha(t.foreground, 0.03) : c ? alpha(c, p.mode === 'dark' ? 0.2 : 0.1) : t.secondary,
    border: `1px solid ${outlined ? (c ? alpha(c, 0.35) : t.border) : 'transparent'}`,
    borderRadius: theme.radius['3xl'],
    whiteSpace: 'nowrap',
    transition: `background-color ${theme.transitions.duration.fast}ms`,
    cursor: clickable ? 'pointer' : 'default',
    outline: 0,
    textDecoration: 'none',
    verticalAlign: 'middle',
    boxSizing: 'border-box',
    ...(disabled ? { opacity: 0.5, pointerEvents: 'none' } : {}),
    ...(clickable ? { userSelect: 'none', '&:hover': { backgroundColor: c ? alpha(c, 0.25) : t.accent } } : {}),
    '& .ui-Chip-label': { overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' },
    '& .ui-Chip-icon': { display: 'inline-flex', marginLeft: -2 },
  }
  const content = (
    <>
      {icon && <span className="ui-Chip-icon">{icon}</span>}
      <span className="ui-Chip-label">{label}</span>
    </>
  )
  const cls = ['ui-Chip', className].filter(Boolean).join(' ')
  if (clickable) {
    return (
      <ButtonBase ref={ref} component="div" className={cls} disabled={disabled} onClick={onClick} baseStyles={[styles]} sx={sx} {...rest}>
        {content}
      </ButtonBase>
    )
  }
  return render('div', { ref, className: cls, ...rest, children: content }, styles, sx, theme)
})

// ── Toggle buttons (Radix ToggleGroup) ────────────────────────────────────

interface ToggleCtx {
  size: 'small' | 'medium' | 'large'
  register: (key: string, value: unknown) => void
  isSelected: (key: string) => boolean
}
const ToggleContext = createContext<ToggleCtx | null>(null)

const keyOf = (v: unknown) => (typeof v === 'string' ? `s:${v}` : `j:${JSON.stringify(v)}`)

export type ToggleButtonGroupProps = Omit<BoxProps, 'onChange' | 'defaultValue' | 'value'> & {
  value: unknown
  exclusive?: boolean
  onChange?: (event: null, value: Any) => void
  size?: 'small' | 'medium' | 'large'
  disabled?: boolean
  orientation?: 'horizontal' | 'vertical'
}

/**
 * Segmented tabs (Radix ToggleGroup): a muted pill track, the active
 * segment raised. `exclusive` makes it single-select; deselecting the
 * active segment reports `null`.
 */
export function ToggleButtonGroup({ value, exclusive, onChange, size = 'medium', disabled, orientation = 'horizontal', sx, children, className, ...rest }: ToggleButtonGroupProps) {
  const theme = useTheme()
  const values = useRef(new Map<string, unknown>())
  const register = (key: string, v: unknown) => values.current.set(key, v)
  const h = size === 'small' ? 32 : size === 'large' ? 40 : 36
  const styles: SxObject = {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 2,
    padding: 3,
    height: orientation === 'vertical' ? 'auto' : h,
    borderRadius: theme.radius.full,
    backgroundColor: theme.palette.tokens.muted,
    flexDirection: orientation === 'vertical' ? 'column' : 'row',
  }
  const cls = ['ui-ToggleButtonGroup', className].filter(Boolean).join(' ')
  const inner = render('div', { className: cls, ...rest, children }, styles, sx, theme)
  const selectedKeys = new Set(exclusive ? (value === null || value === undefined ? [] : [keyOf(value)]) : (Array.isArray(value) ? value : []).map(keyOf))
  const ctx = { size, register, isSelected: (k: string) => selectedKeys.has(k) }
  if (exclusive) {
    return (
      <ToggleContext.Provider value={ctx}>
        <RadixToggleGroup.Root
          asChild
          type="single"
          disabled={disabled}
          orientation={orientation}
          value={value === null || value === undefined ? '' : keyOf(value)}
          onValueChange={(k) => onChange?.(null, k ? (values.current.get(k) ?? null) : null)}
        >
          {inner}
        </RadixToggleGroup.Root>
      </ToggleContext.Provider>
    )
  }
  const list = Array.isArray(value) ? value : []
  return (
    <ToggleContext.Provider value={ctx}>
      <RadixToggleGroup.Root
        asChild
        type="multiple"
        disabled={disabled}
        orientation={orientation}
        value={list.map(keyOf)}
        onValueChange={(keys) => onChange?.(null, keys.map((k) => values.current.get(k)))}
      >
        {inner}
      </RadixToggleGroup.Root>
    </ToggleContext.Provider>
  )
}

export type ToggleButtonProps = Omit<BoxProps, 'value'> & { value: unknown; disabled?: boolean }

export const ToggleButton = forwardRef<Any, ToggleButtonProps>(function ToggleButton({ value, disabled, sx, className, children, ...rest }, ref) {
  const theme = useTheme()
  const ctx = useContext(ToggleContext)
  const t = theme.palette.tokens
  const dark = theme.palette.mode === 'dark'
  const key = keyOf(value)
  ctx?.register(key, value)
  const size = ctx?.size ?? 'medium'
  const styles: SxObject = {
    ...RESET,
    ...theme.typography.button,
    fontSize: theme.typography.pxToRem(size === 'small' ? 12 : 13),
    height: '100%',
    gap: 6,
    padding: `0 ${size === 'small' ? 10 : 12}px`,
    borderRadius: theme.radius.full,
    border: '1px solid transparent',
    color: alpha(t.foreground, 0.6),
    transition: `background-color ${theme.transitions.duration.fast}ms, color ${theme.transitions.duration.fast}ms`,
    '&:hover': { color: t.foreground },
    // Selection is marked with a class rather than Radix's `data-state`: a
    // Tooltip wrapped around the button would overwrite `data-state` with
    // its own open/closed value.
    '&.ui-selected': {
      color: t.foreground,
      backgroundColor: dark ? fade(t.input, 0.3) : t.background,
      borderColor: dark ? t.input : 'transparent',
      boxShadow: theme.elevation.sm,
    },
    '&:disabled': { opacity: 0.5 },
    ...focusRing(theme),
  }
  return (
    <RadixToggleGroup.Item value={key} disabled={disabled} asChild>
      {render('button', { ref, type: 'button', className: ['ui-ToggleButton', ctx?.isSelected(key) && 'ui-selected', className].filter(Boolean).join(' '), ...rest, children }, styles, sx, theme)}
    </RadixToggleGroup.Item>
  )
})
