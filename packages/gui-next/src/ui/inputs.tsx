import {
  createContext,
  forwardRef,
  useCallback,
  useContext,
  useEffect,
  useId,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type ChangeEvent,
  type FocusEvent,
  type KeyboardEvent,
  type ReactNode,
  type Ref,
} from 'react'
import { Checkbox as RadixCheckbox, Select as RadixSelect, Slider as RadixSlider, Switch as RadixSwitch } from 'radix-ui'
import { useTheme } from './ThemeProvider'
import { render, type BoxProps } from './layout'
import type { SxObject, SxProps } from './sx'
import { alpha, fade } from './color'
import type { Theme } from './theme'
import { focusRing } from './buttons'

// biome-ignore lint/suspicious/noExplicitAny: form values are open-ended
type Any = any

const cx = (...c: Array<string | false | null | undefined>) => c.filter(Boolean).join(' ')

// ── Autosizing textarea ───────────────────────────────────────────────────

/**
 * Grows a textarea with its content between `minRows` and `maxRows`, by
 * measuring a hidden twin. The twin shares the textarea's computed font,
 * padding and width, so the measurement matches exactly.
 */
function useAutosize(ref: React.RefObject<HTMLTextAreaElement | null>, minRows: number, maxRows: number | undefined) {
  const shadow = useRef<HTMLTextAreaElement | null>(null)
  const sync = useCallback(() => {
    const input = ref.current
    if (!input) return
    if (!shadow.current) {
      const s = document.createElement('textarea')
      s.setAttribute('aria-hidden', 'true')
      s.tabIndex = -1
      s.readOnly = true
      Object.assign(s.style, { visibility: 'hidden', position: 'absolute', overflow: 'hidden', height: '0', top: '0', left: '0', transform: 'translateZ(0)' })
      document.body.appendChild(s)
      shadow.current = s
    }
    const s = shadow.current
    const cs = window.getComputedStyle(input)
    for (const k of ['fontFamily', 'fontSize', 'fontWeight', 'fontStyle', 'letterSpacing', 'lineHeight', 'paddingTop', 'paddingBottom', 'paddingLeft', 'paddingRight', 'boxSizing', 'borderTopWidth', 'borderBottomWidth', 'whiteSpace', 'wordBreak', 'tabSize'] as const) {
      s.style[k] = cs[k]
    }
    s.style.width = cs.width
    s.value = input.value || input.placeholder || 'x'
    if (s.value.endsWith('\n')) s.value += ' '
    const boxSizing = cs.boxSizing
    const padding = parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom)
    const border = parseFloat(cs.borderTopWidth) + parseFloat(cs.borderBottomWidth)
    const inner = s.scrollHeight - padding
    s.value = 'x'
    const row = s.scrollHeight - padding
    let height = inner
    if (minRows) height = Math.max(minRows * row, height)
    if (maxRows) height = Math.min(maxRows * row, height)
    height = Math.max(height, row)
    const outer = height + (boxSizing === 'border-box' ? padding + border : 0)
    input.style.height = `${outer}px`
    input.style.overflow = maxRows && inner > maxRows * row ? '' : 'hidden'
  }, [ref, minRows, maxRows])

  useLayoutEffect(() => {
    sync()
  })
  useEffect(() => {
    const input = ref.current
    if (!input) return
    const ro = new ResizeObserver(() => sync())
    ro.observe(input)
    return () => {
      ro.disconnect()
      shadow.current?.remove()
      shadow.current = null
    }
  }, [ref, sync])
  return sync
}

const Autosize = forwardRef<HTMLTextAreaElement, React.TextareaHTMLAttributes<HTMLTextAreaElement> & { minRows?: number; maxRows?: number }>(
  function Autosize({ minRows = 1, maxRows, onChange, rows, ...rest }, ref) {
    const inner = useRef<HTMLTextAreaElement | null>(null)
    useImperativeHandle(ref, () => inner.current as HTMLTextAreaElement)
    const sync = useAutosize(inner, rows ?? minRows, rows ?? maxRows)
    return (
      <textarea
        ref={inner}
        rows={rows ?? minRows}
        onChange={(e) => {
          onChange?.(e)
          if (rest.value === undefined) sync()
        }}
        {...rest}
      />
    )
  },
)

// ── InputBase ─────────────────────────────────────────────────────────────

type InputEl = HTMLInputElement | HTMLTextAreaElement

export interface InputSlotProps {
  sx?: SxProps
  readOnly?: boolean
  startAdornment?: ReactNode
  endAdornment?: ReactNode
  disableUnderline?: boolean
}

export type InputBaseProps = Omit<BoxProps, 'onChange' | 'onKeyDown' | 'onBlur' | 'onFocus' | 'defaultValue' | 'label'> & {
  value?: unknown
  defaultValue?: unknown
  onChange?: (e: ChangeEvent<InputEl>) => void
  // Keyboard/focus handlers see the field's own events; typed loosely so a
  // handler written against the wrapper element still fits.
  onKeyDown?: (e: KeyboardEvent<Any>) => void
  onBlur?: (e: FocusEvent<Any>) => void
  onFocus?: (e: FocusEvent<Any>) => void
  placeholder?: string
  fullWidth?: boolean
  multiline?: boolean
  minRows?: number
  maxRows?: number
  rows?: number
  type?: string
  disabled?: boolean
  readOnly?: boolean
  autoFocus?: boolean
  name?: string
  inputRef?: Ref<Any>
  startAdornment?: ReactNode
  endAdornment?: ReactNode
  inputProps?: Record<string, unknown>
  /** Extra styles for the root element, from a wrapping TextField. */
  rootStyles?: SxObject
  /** Extra styles for the `<input>`/`<textarea>`. */
  inputStyles?: SxObject
  /** Rendered last inside the root (a TextField's outline). */
  decoration?: ReactNode
}

function inputBaseStyles(theme: Theme, disabled?: boolean, fullWidth?: boolean, multiline?: boolean): SxObject {
  const p = theme.palette
  return {
    ...theme.typography.body1,
    color: p.text.primary,
    lineHeight: '1.4375em',
    boxSizing: 'border-box',
    position: 'relative',
    cursor: disabled ? 'default' : 'text',
    display: 'inline-flex',
    alignItems: 'center',
    ...(disabled ? { opacity: 0.5 } : {}),
    ...(multiline ? { padding: '4px 0 5px' } : {}),
    ...(fullWidth ? { width: '100%' } : {}),
    '& .ui-InputBase-input': {
      font: 'inherit',
      letterSpacing: 'inherit',
      color: 'currentColor',
      padding: '4px 0 5px',
      border: 0,
      boxSizing: 'content-box',
      background: 'none',
      height: '1.4375em',
      margin: 0,
      WebkitTapHighlightColor: 'transparent',
      display: 'block',
      minWidth: 0,
      width: '100%',
      '&::placeholder': { color: p.tokens.mutedForeground, opacity: 1 },
      '&:focus': { outline: 0 },
      '&:invalid': { boxShadow: 'none' },
      '&::-webkit-search-decoration': { WebkitAppearance: 'none' },
      ...(multiline ? { height: 'auto', resize: 'none', padding: 0, paddingTop: 0 } : {}),
    },
    '& input[type=search]': { MozAppearance: 'textfield' },
  }
}

/** A bare text input (or autosizing textarea) with no border or label. */
export const InputBase = forwardRef<Any, InputBaseProps>(function InputBase(props, ref) {
  const {
    value,
    defaultValue,
    onChange,
    onKeyDown,
    onBlur,
    onFocus,
    placeholder,
    fullWidth,
    multiline,
    minRows,
    maxRows,
    rows,
    type,
    disabled,
    readOnly,
    autoFocus,
    name,
    id,
    inputRef,
    startAdornment,
    endAdornment,
    inputProps,
    rootStyles,
    inputStyles,
    decoration,
    sx,
    className,
    ...rest
  } = props
  const theme = useTheme()
  const [focused, setFocused] = useState(false)
  const inputProps2 = {
    ref: inputRef,
    id,
    name,
    value: value as string | undefined,
    defaultValue: defaultValue as string | undefined,
    placeholder,
    disabled,
    readOnly,
    autoFocus,
    className: 'ui-InputBase-input',
    onChange,
    onKeyDown,
    onFocus: (e: FocusEvent<InputEl>) => {
      setFocused(true)
      onFocus?.(e)
    },
    onBlur: (e: FocusEvent<InputEl>) => {
      setFocused(false)
      onBlur?.(e)
    },
    ...inputProps,
  }
  const field = multiline ? (
    <Autosize minRows={minRows} maxRows={maxRows} rows={rows} {...(inputProps2 as Any)} />
  ) : (
    <input type={type ?? 'text'} {...(inputProps2 as Any)} />
  )
  return render(
    'div',
    {
      ref,
      className: cx('ui-InputBase-root', focused && 'ui-focused', disabled && 'ui-disabled', className),
      'data-focused': focused || undefined,
      ...rest,
      children: (
        <>
          {startAdornment}
          {field}
          {endAdornment}
          {decoration}
        </>
      ),
    },
    [inputBaseStyles(theme, disabled, fullWidth, multiline), rootStyles ?? {}, inputStyles ? { '& .ui-InputBase-input': inputStyles } : {}],
    sx,
    theme,
  )
})

// ── Filled field (TextField / Select) ─────────────────────────────────────

/**
 * Terax fields are filled, not outlined: `input/50` fill, a transparent
 * 1px border, a pill for single-line fields (`3xl`) and a softer `2xl`
 * corner for text areas. Focus shows the ring colour on the border plus a
 * 3px `ring/30` halo; errors swap both for the destructive colour.
 */
function fieldStyles(theme: Theme, o: { small: boolean; focused: boolean; error: boolean; multiline: boolean; start: boolean; end: boolean }): SxObject {
  const p = theme.palette
  const t = p.tokens
  const h = o.small ? 32 : 36
  const ring = o.error ? p.error.main : t.ring
  return {
    ...theme.typography.body2,
    fontSize: theme.typography.pxToRem(o.small ? 13 : 14),
    lineHeight: 1.5,
    backgroundColor: fade(t.input, 0.5),
    border: `1px solid ${o.error ? p.error.main : o.focused ? t.ring : 'transparent'}`,
    boxShadow: o.focused ? `0 0 0 3px ${alpha(ring, o.error ? 0.2 : 0.3)}` : 'none',
    borderRadius: o.multiline ? theme.radius['2xl'] : theme.radius['3xl'],
    transition: `border-color ${theme.transitions.duration.fast}ms, box-shadow ${theme.transitions.duration.fast}ms, background-color ${theme.transitions.duration.fast}ms`,
    ...(o.multiline ? { padding: o.small ? '8px 12px' : '10px 12px', alignItems: 'flex-start' } : { minHeight: h, paddingLeft: o.start ? 12 : 0, paddingRight: o.end ? 12 : 0, gap: o.start || o.end ? 6 : 0 }),
    '& .ui-InputBase-input': o.multiline
      ? { padding: 0 }
      : { padding: `0 ${o.end ? 0 : 12}px 0 ${o.start ? 0 : 12}px`, height: h - 2, lineHeight: `${h - 2}px` },
  }
}

/** A field's label: above the field, 13px/500 — no floating labels. */
function FieldLabel({ label, htmlFor, id, error, disabled }: { label: ReactNode; htmlFor?: string; id?: string; error?: boolean; disabled?: boolean }) {
  const theme = useTheme()
  const p = theme.palette
  return render(
    'label',
    { htmlFor, id, className: 'ui-InputLabel', children: label },
    {
      ...theme.typography.subtitle2,
      display: 'block',
      marginBottom: 6,
      color: error ? p.error.main : p.text.primary,
      opacity: disabled ? 0.5 : 1,
      whiteSpace: 'nowrap',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
    },
    undefined,
    theme,
  )
}

// ── FormControl / InputLabel ──────────────────────────────────────────────

interface FormCtx {
  size: 'small' | 'medium'
  fullWidth: boolean
  label: ReactNode
  labelId?: string
  setLabel: (label: ReactNode, id?: string) => void
}
const FormContext = createContext<FormCtx | null>(null)

const formControlBase = (fullWidth: boolean): SxObject => ({
  display: 'inline-flex',
  flexDirection: 'column',
  position: 'relative',
  minWidth: 0,
  padding: 0,
  margin: 0,
  border: 0,
  verticalAlign: 'top',
  ...(fullWidth ? { width: '100%' } : {}),
})

export type FormControlProps = BoxProps & { size?: 'small' | 'medium'; fullWidth?: boolean }

/** Groups an `InputLabel` with the `Select` it labels. */
export function FormControl({ size = 'medium', fullWidth = false, sx, children, ...rest }: FormControlProps) {
  const theme = useTheme()
  const [label, setLabelState] = useState<{ label: ReactNode; id?: string }>({ label: null })
  const setLabel = useCallback((l: ReactNode, id?: string) => setLabelState({ label: l, id }), [])
  return (
    <FormContext.Provider value={{ size, fullWidth, label: label.label, labelId: label.id, setLabel }}>
      {render('div', { ...rest, children }, formControlBase(fullWidth), sx, theme)}
    </FormContext.Provider>
  )
}

/** The label of the `Select` in the same `FormControl`, drawn above it. */
export function InputLabel({ id, children }: { id?: string; children?: ReactNode }) {
  const ctx = useContext(FormContext)
  useEffect(() => {
    ctx?.setLabel(children, id)
    return () => ctx?.setLabel(null)
    // biome-ignore lint/correctness/useExhaustiveDependencies: register once per label change
  }, [children, id])
  return null
}

function HelperText({ children, error }: { children: ReactNode; error?: boolean }) {
  const theme = useTheme()
  return render(
    'p',
    { className: 'ui-FormHelperText', children },
    { ...theme.typography.caption, color: error ? theme.palette.error.main : theme.palette.text.secondary, textAlign: 'left', margin: '6px 4px 0' },
    undefined,
    theme,
  )
}

// ── TextField ─────────────────────────────────────────────────────────────

export type TextFieldProps = InputBaseProps & {
  label?: ReactNode
  helperText?: ReactNode
  error?: boolean
  required?: boolean
  size?: 'small' | 'medium'
  variant?: 'outlined' | 'standard'
  select?: boolean
  slotProps?: { input?: InputSlotProps; htmlInput?: Record<string, unknown> }
}

/** A labelled text field: filled, label above, helper text below.
 * `variant="standard"` is a bare input; `select` turns it into a dropdown
 * of its `MenuItem` children. */
export const TextField = forwardRef<Any, TextFieldProps>(function TextField(props, ref) {
  const {
    label,
    helperText,
    error = false,
    required,
    size = 'medium',
    variant = 'outlined',
    select,
    slotProps,
    fullWidth = false,
    sx,
    id: idProp,
    value,
    onChange,
    disabled = false,
    multiline = false,
    children,
    className,
    ...rest
  } = props
  const theme = useTheme()
  const autoId = useId()
  const id = idProp ?? `ui-tf-${autoId}`
  const [focused, setFocused] = useState(false)
  const small = size === 'small'
  const input = slotProps?.input ?? {}
  const labelText = label && required ? <>{label}&thinsp;{'*'}</> : label

  if (select) {
    return render(
      'div',
      {
        ref,
        className: cx('ui-TextField', className),
        children: (
          <>
            {labelText && <FieldLabel label={labelText} htmlFor={id} error={error} disabled={disabled} />}
            <Select value={value} onChange={onChange as Any} size={size} disabled={disabled} fullWidth={fullWidth} error={error} id={id} sx={input.sx}>
              {children}
            </Select>
            {helperText && <HelperText error={error}>{helperText}</HelperText>}
          </>
        ),
      },
      formControlBase(fullWidth),
      sx,
      theme,
    )
  }

  const start = input.startAdornment
  const end = input.endAdornment
  let rootStyles: SxObject
  if (variant === 'standard') {
    const t = theme.palette.tokens
    rootStyles = input.disableUnderline
      ? {}
      : {
          borderBottom: `1px solid ${focused ? t.ring : t.border}`,
          transition: `border-color ${theme.transitions.duration.fast}ms`,
        }
  } else {
    rootStyles = fieldStyles(theme, { small, focused, error, multiline, start: !!start, end: !!end })
  }

  const field = (
    <InputBase
      id={id}
      value={value}
      onChange={onChange}
      disabled={disabled}
      multiline={multiline}
      fullWidth={fullWidth}
      readOnly={input.readOnly}
      startAdornment={start}
      endAdornment={end}
      inputProps={slotProps?.htmlInput}
      rootStyles={rootStyles}
      sx={input.sx}
      aria-invalid={error || undefined}
      {...rest}
      onFocus={(e) => {
        setFocused(true)
        rest.onFocus?.(e)
      }}
      onBlur={(e) => {
        setFocused(false)
        rest.onBlur?.(e)
      }}
    />
  )

  return render(
    'div',
    {
      ref,
      className: cx('ui-TextField', className),
      children: (
        <>
          {labelText && <FieldLabel label={labelText} htmlFor={id} error={error} disabled={disabled} />}
          {field}
          {helperText && <HelperText error={error}>{helperText}</HelperText>}
        </>
      ),
    },
    formControlBase(fullWidth),
    sx,
    theme,
  )
})

// ── Select (Radix Select) ─────────────────────────────────────────────────

const EMPTY = '__ui_empty__'
const keyOf = (v: unknown) => (v === '' || v === null || v === undefined ? EMPTY : typeof v === 'string' ? `s:${v}` : `j:${JSON.stringify(v)}`)

interface SelectCtx {
  register: (key: string, value: unknown) => void
}
const SelectContext = createContext<SelectCtx | null>(null)

export type SelectProps = Omit<BoxProps, 'onChange' | 'defaultValue' | 'label'> & {
  value: unknown
  onChange?: (e: { target: { value: Any; name?: string } }) => void
  size?: 'small' | 'medium'
  label?: ReactNode
  labelId?: string
  disabled?: boolean
  fullWidth?: boolean
  error?: boolean
  name?: string
}

const CHEVRON = 'M6 9l6 6 6-6'

/** A dropdown of `MenuItem` children (Radix Select), drawn as a filled
 * field with a chevron. */
export function Select({ value, onChange, size: sizeProp, label: labelProp, labelId, disabled = false, fullWidth: fullProp, error = false, name, id, sx, children, className, ...rest }: SelectProps) {
  const theme = useTheme()
  const ctx = useContext(FormContext)
  const size = sizeProp ?? ctx?.size ?? 'medium'
  const fullWidth = fullProp ?? ctx?.fullWidth ?? false
  const label = labelProp ?? ctx?.label ?? null
  const values = useRef(new Map<string, unknown>())
  const [open, setOpen] = useState(false)
  const [focused, setFocused] = useState(false)
  const small = size === 'small'
  const t = theme.palette.tokens
  const register = useCallback((k: string, v: unknown) => values.current.set(k, v), [])
  const autoId = useId()
  const triggerId = id ?? `ui-sel-${autoId}`
  const h = small ? 32 : 36

  const styles: SxObject = {
    ...inputBaseStyles(theme, disabled, fullWidth, false),
    ...fieldStyles(theme, { small, focused: open || focused, error, multiline: false, start: false, end: false }),
    cursor: disabled ? 'default' : 'pointer',
    '& .ui-Select-select': {
      font: 'inherit',
      color: 'inherit',
      background: 'none',
      border: 0,
      margin: 0,
      boxSizing: 'border-box',
      width: '100%',
      minWidth: 16,
      height: h - 2,
      display: 'block',
      textAlign: 'left',
      padding: '0 34px 0 12px',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
      whiteSpace: 'nowrap',
      cursor: 'inherit',
      userSelect: 'none',
      borderRadius: 'inherit',
      outline: 0,
    },
    '& .ui-Select-icon': {
      position: 'absolute',
      right: 12,
      top: 'calc(50% - 8px)',
      width: 16,
      height: 16,
      pointerEvents: 'none',
      color: t.mutedForeground,
      transition: `transform ${theme.transitions.duration.fast}ms ${theme.transitions.easing.soft}`,
      transform: open ? 'rotate(180deg)' : 'none',
    },
  }
  const select = (
    <RadixSelect.Root
      value={keyOf(value)}
      onValueChange={(k) => onChange?.({ target: { value: k === EMPTY ? '' : values.current.get(k), name } })}
      open={open}
      onOpenChange={setOpen}
      disabled={disabled}
      name={name}
    >
      {render(
        'div',
        {
          className: cx('ui-InputBase-root', 'ui-Select-root', className),
          ...rest,
          children: (
            <>
              <RadixSelect.Trigger
                id={triggerId}
                className="ui-Select-select"
                aria-labelledby={labelId ?? ctx?.labelId}
                onFocus={() => setFocused(true)}
                onBlur={() => setFocused(false)}
              >
                <RadixSelect.Value placeholder={'​'} />
              </RadixSelect.Trigger>
              <svg className="ui-Select-icon" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth={1.75} strokeLinecap="round" strokeLinejoin="round">
                <path d={CHEVRON} />
              </svg>
            </>
          ),
        },
        styles,
        sx,
        theme,
      )}
      <RadixSelect.Portal>
        <SelectContent>{children}</SelectContent>
      </RadixSelect.Portal>
    </RadixSelect.Root>
  )
  return (
    <SelectContext.Provider value={{ register }}>
      {label && <FieldLabel label={label} id={labelId ?? ctx?.labelId} htmlFor={triggerId} error={error} disabled={disabled} />}
      {select}
    </SelectContext.Provider>
  )
}

function SelectContent({ children }: { children: ReactNode }) {
  const theme = useTheme()
  const styles: SxObject = {
    ...menuPaper(theme),
    minWidth: 'max(var(--radix-select-trigger-width), 160px)',
    maxHeight: 'var(--radix-select-content-available-height)',
    zIndex: theme.zIndex.modal,
    overflow: 'hidden',
  }
  return (
    <RadixSelect.Content asChild position="item-aligned">
      {render('div', { className: 'ui-Select-menu', children: <RadixSelect.Viewport style={{ padding: 6 }}>{children}</RadixSelect.Viewport> }, styles, undefined, theme)}
    </RadixSelect.Content>
  )
}

/** The floating surface menus and select lists sit on: `popover` fill,
 * `3xl` corners, a 1px ring and a large soft shadow. */
export function menuPaper(theme: Theme): SxObject {
  return {
    backgroundColor: theme.palette.background.paper,
    color: theme.palette.text.primary,
    boxShadow: `${theme.elevation.ring}, ${theme.elevation.lg}`,
    borderRadius: theme.radius['3xl'],
    outline: 0,
  }
}

/** Menu rows and select options: `2xl` rounded rows, 14px/500, accent on
 * focus. */
export function menuItemStyles(theme: Theme): SxObject {
  const p = theme.palette
  const t = p.tokens
  return {
    ...theme.typography.body2,
    fontSize: theme.typography.pxToRem(13),
    fontWeight: 500,
    display: 'flex',
    justifyContent: 'flex-start',
    alignItems: 'center',
    gap: 10,
    position: 'relative',
    textDecoration: 'none',
    minHeight: 32,
    padding: '6px 12px',
    borderRadius: theme.radius['2xl'],
    boxSizing: 'border-box',
    whiteSpace: 'nowrap',
    cursor: 'pointer',
    outline: 0,
    userSelect: 'none',
    color: t.foreground,
    '& svg': { color: t.mutedForeground, flexShrink: 0 },
    '&:hover, &[data-highlighted]': { backgroundColor: t.accent, '& svg': { color: t.foreground } },
    '&[data-state="checked"], &.ui-selected': { '& .ui-MenuItem-check': { opacity: 1 } },
    '&[data-disabled], &[aria-disabled="true"]': { opacity: 0.5, pointerEvents: 'none' },
    '& .ui-ListItemIcon': { minWidth: 0 },
    '& .ui-MenuItem-check': { marginLeft: 'auto', paddingLeft: 12, display: 'inline-flex', opacity: 0 },
  }
}

const CHECK = 'M5 12.5l4.5 4.5L19 7.5'

/** A `MenuItem` rendered inside a `Select`: its label, and a check on the
 * selected one. */
export function SelectOption({ value, disabled, sx, children, className }: { value: unknown; disabled?: boolean; sx?: SxProps; children?: ReactNode; className?: string }) {
  const theme = useTheme()
  const ctx = useContext(SelectContext)
  const key = keyOf(value)
  ctx?.register(key, value)
  return (
    <RadixSelect.Item value={key} disabled={disabled} asChild>
      {render(
        'div',
        {
          className: cx('ui-MenuItem', className),
          children: (
            <>
              <RadixSelect.ItemText>{children}</RadixSelect.ItemText>
              <span className="ui-MenuItem-check" aria-hidden="true">
                <svg viewBox="0 0 24 24" width={14} height={14} fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
                  <path d={CHECK} />
                </svg>
              </span>
            </>
          ),
        },
        menuItemStyles(theme),
        sx,
        theme,
      )}
    </RadixSelect.Item>
  )
}

// ── Switch / Checkbox / FormControlLabel ──────────────────────────────────

type CheckChange = (e: { target: { checked: boolean; name?: string } }, checked: boolean) => void

export type SwitchProps = Omit<BoxProps, 'onChange'> & { checked?: boolean; onChange?: CheckChange; size?: 'small' | 'medium'; disabled?: boolean; name?: string; color?: string }

/** An on/off switch (Radix Switch): a pill track with a pill thumb. */
export const Switch = forwardRef<Any, SwitchProps>(function Switch({ checked = false, onChange, size = 'medium', disabled, name, sx, className, ...rest }, ref) {
  const theme = useTheme()
  const p = theme.palette
  const t = p.tokens
  const dark = p.mode === 'dark'
  const small = size === 'small'
  // Terax: track h-5 w-11, thumb h-4 w-6. Small is the same shape, scaled.
  const trackW = small ? 34 : 44
  const trackH = small ? 18 : 20
  const thumbW = small ? 18 : 24
  const thumbH = small ? 14 : 16
  const inset = (trackH - thumbH) / 2
  const styles: SxObject = {
    all: 'unset',
    boxSizing: 'border-box',
    position: 'relative',
    display: 'inline-flex',
    flexShrink: 0,
    width: trackW,
    height: trackH,
    borderRadius: theme.radius.full,
    backgroundColor: dark ? fade(t.input, 0.8) : t.input,
    cursor: disabled ? 'default' : 'pointer',
    opacity: disabled ? 0.5 : 1,
    verticalAlign: 'middle',
    transition: `background-color ${theme.transitions.duration.fast}ms`,
    '& .ui-Switch-thumb': {
      position: 'absolute',
      top: inset,
      left: inset,
      width: thumbW,
      height: thumbH,
      borderRadius: theme.radius.full,
      backgroundColor: t.background,
      boxShadow: theme.elevation.sm,
      transition: `transform ${theme.transitions.duration.base}ms ${theme.transitions.easing.premium}`,
    },
    // `aria-checked` rather than Radix's `data-state`, which a wrapping
    // Tooltip would overwrite with its own open/closed value.
    '&[aria-checked="true"]': {
      backgroundColor: p.primary.main,
      '& .ui-Switch-thumb': { transform: `translateX(${trackW - thumbW - inset * 2}px)`, backgroundColor: dark ? t.background : '#fff' },
    },
    ...focusRing(theme),
  }
  return (
    <RadixSwitch.Root checked={checked} disabled={disabled} name={name} onCheckedChange={(c) => onChange?.({ target: { checked: c, name } }, c)} asChild>
      {render('button', { ref, type: 'button', className: cx('ui-Switch', className), ...rest, children: <RadixSwitch.Thumb className="ui-Switch-thumb" /> }, styles, sx, theme)}
    </RadixSwitch.Root>
  )
})

export type CheckboxProps = Omit<BoxProps, 'onChange'> & { checked?: boolean; indeterminate?: boolean; onChange?: CheckChange; size?: 'small' | 'medium'; disabled?: boolean; name?: string }

/** A checkbox (Radix Checkbox): a small rounded square, filled with the
 * ink colour when checked. */
export const Checkbox = forwardRef<Any, CheckboxProps>(function Checkbox({ checked = false, indeterminate, onChange, size = 'medium', disabled, name, sx, className, ...rest }, ref) {
  const theme = useTheme()
  const p = theme.palette
  const t = p.tokens
  const on = checked || indeterminate
  const box = size === 'small' ? 15 : 16
  const styles: SxObject = {
    all: 'unset',
    boxSizing: 'border-box',
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    flexShrink: 0,
    // A generous hit area around a small box.
    padding: 6,
    margin: -2,
    cursor: disabled ? 'default' : 'pointer',
    opacity: disabled ? 0.5 : 1,
    '& .ui-Checkbox-box': {
      width: box,
      height: box,
      boxSizing: 'border-box',
      borderRadius: 5,
      border: `1px solid ${on ? p.primary.main : t.input}`,
      backgroundColor: on ? p.primary.main : fade(t.input, 0.3),
      color: p.primary.contrastText,
      display: 'inline-flex',
      alignItems: 'center',
      justifyContent: 'center',
      transition: `background-color ${theme.transitions.duration.fast}ms, border-color ${theme.transitions.duration.fast}ms`,
    },
    '&:focus-visible': { outline: 'none', '& .ui-Checkbox-box': { boxShadow: `0 0 0 3px ${alpha(t.ring, 0.3)}`, borderColor: t.ring } },
  }
  return (
    <RadixCheckbox.Root
      checked={indeterminate ? 'indeterminate' : checked}
      disabled={disabled}
      name={name}
      onCheckedChange={(c) => onChange?.({ target: { checked: c === true, name } }, c === true)}
      asChild
    >
      {render(
        'button',
        {
          ref,
          type: 'button',
          className: cx('ui-Checkbox', className),
          ...rest,
          children: (
            <span className="ui-Checkbox-box">
              {on && (
                <svg viewBox="0 0 24 24" width={box - 4} height={box - 4} fill="none" stroke="currentColor" strokeWidth={3} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <path d={indeterminate ? 'M6 12h12' : CHECK} />
                </svg>
              )}
            </span>
          ),
        },
        styles,
        sx,
        theme,
      )}
    </RadixCheckbox.Root>
  )
})

export type FormControlLabelProps = Omit<BoxProps, 'label'> & { control: ReactNode; label: ReactNode; disabled?: boolean }

/** A control (Switch, Checkbox) with a clickable label beside it. */
export function FormControlLabel({ control, label, disabled, sx, className, ...rest }: FormControlLabelProps) {
  const theme = useTheme()
  const styles: SxObject = {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 10,
    cursor: disabled ? 'default' : 'pointer',
    verticalAlign: 'middle',
    WebkitTapHighlightColor: 'transparent',
    '& > .ui-FormControlLabel-label': { ...theme.typography.body2, fontSize: theme.typography.pxToRem(14), ...(disabled ? { opacity: 0.5 } : {}) },
  }
  return render(
    'label',
    {
      className: cx('ui-FormControlLabel', className),
      ...rest,
      children: (
        <>
          {control}
          <span className="ui-FormControlLabel-label">{label}</span>
        </>
      ),
    },
    styles,
    sx,
    theme,
  )
}

// ── Slider (Radix Slider) ─────────────────────────────────────────────────

export type SliderProps = Omit<BoxProps, 'onChange' | 'value' | 'defaultValue'> & {
  value: number
  min?: number
  max?: number
  step?: number
  marks?: boolean | Array<{ value: number; label?: ReactNode }>
  onChange?: (e: null, value: number) => void
  disabled?: boolean
}

/**
 * A single-thumb slider (Radix Slider): a thin muted track, an ink range,
 * a round thumb on the pane colour. Parts carry stable classes —
 * `ui-Slider-rail`, `-track`, `-thumb`, `-mark`, `-markActive` — for
 * styling from `sx`.
 */
export function Slider({ value, min = 0, max = 100, step = 1, marks, onChange, disabled, sx, className, 'aria-label': ariaLabel, ...rest }: SliderProps) {
  const theme = useTheme()
  const p = theme.palette
  const t = p.tokens
  const markList = marks === true ? Array.from({ length: Math.floor((max - min) / step) + 1 }, (_, i) => ({ value: min + i * step })) : marks || []
  const pct = (v: number) => ((v - min) / (max - min)) * 100
  const styles: SxObject = {
    borderRadius: theme.radius.full,
    boxSizing: 'content-box',
    display: 'inline-block',
    position: 'relative',
    cursor: disabled ? 'default' : 'pointer',
    touchAction: 'none',
    color: p.primary.main,
    opacity: disabled ? 0.5 : 1,
    WebkitTapHighlightColor: 'transparent',
    height: 6,
    width: '100%',
    padding: '7px 0',
    '& .ui-Slider-root': { position: 'relative', display: 'flex', alignItems: 'center', width: '100%', height: 'inherit', userSelect: 'none', touchAction: 'none' },
    '& .ui-Slider-railInner': { position: 'relative', flexGrow: 1, height: 'inherit', borderRadius: 'inherit' },
    '& .ui-Slider-rail': { display: 'block', position: 'absolute', borderRadius: 'inherit', backgroundColor: t.muted, width: '100%', height: 'inherit', top: '50%', transform: 'translateY(-50%)', boxShadow: `inset 0 0 0 1px ${t.border}` },
    '& .ui-Slider-track': { display: 'block', position: 'absolute', borderRadius: 'inherit', backgroundColor: 'currentColor', height: 'inherit', top: '50%', transform: 'translateY(-50%)' },
    '& .ui-Slider-thumb': {
      position: 'relative',
      display: 'block',
      width: 16,
      height: 16,
      boxSizing: 'border-box',
      borderRadius: '50%',
      outline: 0,
      border: '1px solid currentColor',
      backgroundColor: t.background,
      boxShadow: theme.elevation.sm,
      transition: `box-shadow ${theme.transitions.duration.fast}ms`,
      '&:hover, &:focus-visible': { boxShadow: `0 0 0 4px ${alpha(t.ring, 0.3)}` },
    },
    '& .ui-Slider-mark': { position: 'absolute', width: 2, height: 2, borderRadius: 1, backgroundColor: t.mutedForeground, top: '50%', transform: 'translate(-1px, -50%)', pointerEvents: 'none' },
    '& .ui-Slider-markActive': { backgroundColor: t.background, opacity: 0.8 },
  }
  return render(
    'span',
    {
      className: cx('ui-Slider', className),
      ...rest,
      children: (
        <RadixSlider.Root className="ui-Slider-root" value={[value]} min={min} max={max} step={step} disabled={disabled} onValueChange={(v) => onChange?.(null, v[0])} aria-label={ariaLabel as string | undefined}>
          <RadixSlider.Track className="ui-Slider-railInner">
            <span className="ui-Slider-rail" />
            <RadixSlider.Range className="ui-Slider-track" />
            {markList.map((m, i) => (
              <span key={m.value} data-index={i} className={cx('ui-Slider-mark', m.value <= value && 'ui-Slider-markActive')} style={{ left: `${pct(m.value)}%` }} />
            ))}
          </RadixSlider.Track>
          <RadixSlider.Thumb className="ui-Slider-thumb" aria-label={ariaLabel as string | undefined} />
        </RadixSlider.Root>
      ),
    },
    styles,
    sx,
    theme,
  )
}

/** Whether a `MenuItem` is rendered inside a `Select`. */
export function useInSelect() {
  return useContext(SelectContext) !== null
}
