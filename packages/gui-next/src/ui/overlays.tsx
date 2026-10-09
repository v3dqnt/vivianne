import {
  createContext,
  forwardRef,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from 'react'
import { keyframes } from '@emotion/react'
import {
  Dialog as RadixDialog,
  DropdownMenu,
  Popover as RadixPopover,
  Portal,
  Progress,
  Toast,
  Tooltip as RadixTooltip,
  VisuallyHidden,
} from 'radix-ui'
import { useTheme } from './ThemeProvider'
import { render, type BoxProps } from './layout'
import type { SxObject, SxProps } from './sx'
import { alpha } from './color'
import type { ColorName } from './theme'
import { menuItemStyles, menuPaper, SelectOption, useInSelect } from './inputs'

// biome-ignore lint/suspicious/noExplicitAny: polymorphic element props
type Any = any

const cx = (...c: Array<string | false | null | undefined>) => c.filter(Boolean).join(' ')

type CloseReason = 'backdropClick' | 'escapeKeyDown'
type OnClose = (event: object, reason: CloseReason) => void

// ── Motion ────────────────────────────────────────────────────────────────

const fadeIn = keyframes({ from: { opacity: 0 }, to: { opacity: 1 } })
const fadeOut = keyframes({ from: { opacity: 1 }, to: { opacity: 0 } })
const growIn = keyframes({ from: { opacity: 0, transform: 'scale(0.95)' }, to: { opacity: 1, transform: 'none' } })
const growOut = keyframes({ from: { opacity: 1, transform: 'none' }, to: { opacity: 0, transform: 'scale(0.95)' } })
const PREMIUM = 'cubic-bezier(0.16, 1, 0.3, 1)'

/** Terax overlay motion: a quick fade + zoom from 95%, from the edge
 * nearest the anchor, keyed off Radix's `data-state`. */
function grow(origin: string, ms = 140): SxObject {
  return {
    transformOrigin: origin,
    '&[data-state="open"], &[data-state="delayed-open"], &[data-state="instant-open"]': { animation: `${growIn} ${ms}ms ${PREMIUM}` },
    '&[data-state="closed"]': { animation: `${growOut} 100ms cubic-bezier(0.4, 0, 0.2, 1)` },
  }
}
const fade: SxObject = {
  '&[data-state="open"]': { animation: `${fadeIn} 140ms cubic-bezier(0.4, 0, 0.2, 1)` },
  '&[data-state="closed"]': { animation: `${fadeOut} 100ms cubic-bezier(0.4, 0, 0.2, 1)` },
}

/**
 * Where focus was when an overlay opened, to hand it back on close. Radix
 * returns focus to its own Trigger, but these overlays are opened from
 * app state rather than a Trigger, so without this focus would fall to
 * <body> — and the next Tab would start from the top of the window.
 */
function useReturnFocus(open: boolean) {
  const saved = useRef<HTMLElement | null>(null)
  if (open && saved.current === null && typeof document !== 'undefined') {
    saved.current = document.activeElement as HTMLElement | null
  }
  return (e: Event) => {
    e.preventDefault()
    const el = saved.current
    saved.current = null
    if (el && el.isConnected && el !== document.body) el.focus({ preventScroll: true })
  }
}

// ── Anchored placement ────────────────────────────────────────────────────

export interface Origin {
  vertical: 'top' | 'center' | 'bottom'
  horizontal: 'left' | 'center' | 'right'
}

/**
 * Turn an anchor point + content point pair (where on the anchor, which
 * corner of the content goes there) into Radix's side/align/offsets.
 */
function placement(anchorEl: Element | null | undefined, anchor: Origin, transform: Origin) {
  const r = anchorEl?.getBoundingClientRect()
  const h = r?.height ?? 0
  const w = r?.width ?? 0
  const frac = { top: 0, left: 0, center: 0.5, bottom: 1, right: 1 } as const
  const ay = frac[anchor.vertical] * h
  const ax = frac[anchor.horizontal] * w
  let side: 'top' | 'bottom' = 'bottom'
  let sideOffset = 0
  if (transform.vertical === 'bottom') {
    side = 'top'
    sideOffset = -ay
  } else {
    side = 'bottom'
    sideOffset = ay - h
  }
  let align: 'start' | 'center' | 'end' = 'start'
  let alignOffset = 0
  if (transform.horizontal === 'right') {
    align = 'end'
    alignOffset = w - ax
  } else if (transform.horizontal === 'center') {
    align = 'center'
  } else {
    alignOffset = ax
  }
  const origin = `${transform.horizontal === 'center' ? 'center' : transform.horizontal} ${transform.vertical === 'center' ? 'center' : transform.vertical}`
  return { side, sideOffset, align, alignOffset, origin }
}

// ── Popover ───────────────────────────────────────────────────────────────

interface PaperSlot {
  paper?: { sx?: SxProps; className?: string }
  list?: { sx?: SxProps }
}

export interface PopoverProps {
  open: boolean
  anchorEl: Element | null | undefined
  onClose?: OnClose
  anchorOrigin?: Origin
  transformOrigin?: Origin
  slotProps?: PaperSlot
  children?: ReactNode
}

/** A floating card anchored to an element (Radix Popover). Modal: outside
 * clicks close it rather than reaching what's underneath. */
export function Popover({ open, anchorEl, onClose, anchorOrigin = { vertical: 'top', horizontal: 'left' }, transformOrigin = { vertical: 'top', horizontal: 'left' }, slotProps, children }: PopoverProps) {
  const theme = useTheme()
  const reason = useRef<CloseReason>('backdropClick')
  const virtual = useRef<Element | null>(null)
  virtual.current = anchorEl ?? null
  const pos = placement(anchorEl, anchorOrigin, transformOrigin)
  const returnFocus = useReturnFocus(open && !!anchorEl)
  const styles: SxObject = {
    ...menuPaper(theme),
    overflowY: 'auto',
    overflowX: 'hidden',
    minWidth: 16,
    minHeight: 16,
    maxWidth: 'min(calc(100vw - 32px), var(--radix-popover-content-available-width))',
    maxHeight: 'min(calc(100vh - 32px), var(--radix-popover-content-available-height))',
    outline: 0,
    zIndex: theme.zIndex.modal,
    ...grow(pos.origin),
  }
  return (
    <RadixPopover.Root
      open={open && !!anchorEl}
      modal
      onOpenChange={(o) => {
        if (!o) onClose?.({}, reason.current)
      }}
    >
      <RadixPopover.Anchor virtualRef={virtual as React.RefObject<Any>} />
      <RadixPopover.Portal>
        <RadixPopover.Content
          asChild
          side={pos.side}
          sideOffset={pos.sideOffset}
          align={pos.align}
          alignOffset={pos.alignOffset}
          collisionPadding={16}
          onCloseAutoFocus={returnFocus}
          onEscapeKeyDown={() => {
            reason.current = 'escapeKeyDown'
          }}
          onPointerDownOutside={() => {
            reason.current = 'backdropClick'
          }}
        >
          {render('div', { className: cx('ui-Popover-paper', slotProps?.paper?.className), children }, styles, slotProps?.paper?.sx, theme)}
        </RadixPopover.Content>
      </RadixPopover.Portal>
    </RadixPopover.Root>
  )
}

// ── Menu ──────────────────────────────────────────────────────────────────

const MenuContext = createContext(false)

export interface MenuProps extends PopoverProps {
  autoFocus?: boolean
}

/**
 * A menu anchored to an element (Radix DropdownMenu): arrow keys, typeahead,
 * Escape and outside clicks all work. Items don't close it by themselves —
 * the caller closes it from `onClick`, which is what lets a menu hold toggles.
 */
export function Menu({ open, anchorEl, onClose, anchorOrigin = { vertical: 'top', horizontal: 'left' }, transformOrigin = { vertical: 'top', horizontal: 'left' }, slotProps, children }: MenuProps) {
  const theme = useTheme()
  const reason = useRef<CloseReason>('backdropClick')
  const [rect, setRect] = useState<DOMRect | null>(null)
  const isOpen = open && !!anchorEl
  const returnFocus = useReturnFocus(isOpen)
  useLayoutEffect(() => {
    if (isOpen) {
      setRect(anchorEl!.getBoundingClientRect())
      return
    }
    // Keep the stand-in anchor through the close animation, then drop it — a
    // closed menu leaves nothing in the page.
    const t = window.setTimeout(() => setRect(null), 250)
    return () => window.clearTimeout(t)
  }, [isOpen, anchorEl])
  const pos = placement(anchorEl, anchorOrigin, transformOrigin)
  const styles: SxObject = {
    ...menuPaper(theme),
    minWidth: 16,
    maxHeight: 'min(calc(100vh - 96px), var(--radix-dropdown-menu-content-available-height))',
    overflowY: 'auto',
    overflowX: 'hidden',
    zIndex: theme.zIndex.modal,
    ...grow(pos.origin),
  }
  return (
    <MenuContext.Provider value={true}>
      <DropdownMenu.Root
        open={isOpen && !!rect}
        onOpenChange={(o) => {
          if (!o) onClose?.({}, reason.current)
        }}
      >
        {/* Radix positions a menu against its trigger. The real anchor lives
            elsewhere in the tree, so an invisible stand-in covers it. */}
        {rect && (
          <Portal.Root>
            <DropdownMenu.Trigger asChild>
              <span
                aria-hidden
                tabIndex={-1}
                style={{ position: 'fixed', left: rect.left, top: rect.top, width: rect.width, height: rect.height, pointerEvents: 'none', visibility: 'hidden' }}
              />
            </DropdownMenu.Trigger>
          </Portal.Root>
        )}
        <DropdownMenu.Portal>
          <DropdownMenu.Content
            asChild
            side={pos.side}
            sideOffset={pos.sideOffset}
            align={pos.align}
            alignOffset={pos.alignOffset}
            collisionPadding={16}
            loop
            onEscapeKeyDown={() => {
              reason.current = 'escapeKeyDown'
            }}
            onPointerDownOutside={() => {
              reason.current = 'backdropClick'
            }}
            onCloseAutoFocus={returnFocus}
          >
            {render(
              'div',
              {
                className: cx('ui-Menu-paper', slotProps?.paper?.className),
                children: render('div', { className: 'ui-Menu-list', children }, { padding: 6, outline: 0, minWidth: 192 }, slotProps?.list?.sx, theme),
              },
              styles,
              slotProps?.paper?.sx,
              theme,
            )}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </MenuContext.Provider>
  )
}

export type MenuItemProps = Omit<BoxProps, 'value'> & { value?: unknown; selected?: boolean; disabled?: boolean; dense?: boolean }

/** A row in a `Menu`, or an option in a `Select`. */
export const MenuItem = forwardRef<Any, MenuItemProps>(function MenuItem({ value, selected, disabled, dense, sx, className, children, ...rest }, ref) {
  const theme = useTheme()
  const inMenu = useContext(MenuContext)
  const inSelect = useInSelect()
  if (inSelect) {
    return (
      <SelectOption value={value} disabled={disabled} sx={sx} className={className}>
        {children}
      </SelectOption>
    )
  }
  const styles: SxObject = { ...menuItemStyles(theme), ...(dense ? { paddingTop: 4, paddingBottom: 4 } : {}) }
  // Inside a Menu, Radix supplies role, tabIndex and the disabled state
  // through its Slot — and a key set here, even to `undefined`, would
  // override it, leaving items unfocusable by the arrow keys.
  const own = inMenu ? {} : { role: 'menuitem', tabIndex: -1, ...(disabled ? { 'aria-disabled': true } : {}) }
  const el = render('div', { ref, className: cx('ui-MenuItem', selected && 'ui-selected', className), ...own, ...rest, children }, styles, sx, theme)
  if (!inMenu) return el
  return (
    <DropdownMenu.Item asChild disabled={disabled} onSelect={(e) => e.preventDefault()}>
      {el}
    </DropdownMenu.Item>
  )
})

export function ListItemIcon({ sx, children, className, ...rest }: BoxProps) {
  const theme = useTheme()
  return render('div', { className: cx('ui-ListItemIcon', className), ...rest, children }, { minWidth: 0, color: theme.palette.tokens.mutedForeground, flexShrink: 0, display: 'inline-flex' }, sx, theme)
}

export function ListSubheader({ sx, children, className, ...rest }: BoxProps) {
  const theme = useTheme()
  return render(
    'div',
    { role: 'presentation', className: cx('ui-ListSubheader', className), ...rest, children },
    {
      boxSizing: 'border-box',
      listStyle: 'none',
      color: theme.palette.text.secondary,
      fontFamily: theme.typography.fontFamily,
      fontWeight: 500,
      fontSize: theme.typography.pxToRem(12),
      padding: '10px 12px 6px',
      position: 'sticky',
      top: 0,
      zIndex: 1,
      backgroundColor: theme.palette.background.paper,
    },
    sx,
    theme,
  )
}

// ── Dialog ────────────────────────────────────────────────────────────────

const DialogContext = createContext<{ registerTitle: () => () => void } | null>(null)

export interface DialogProps {
  open: boolean
  onClose?: OnClose
  fullWidth?: boolean
  fullScreen?: boolean
  maxWidth?: 'xs' | 'sm' | 'md' | 'lg' | 'xl' | false
  slotProps?: PaperSlot & { backdrop?: { sx?: SxProps } }
  sx?: SxProps
  children?: ReactNode
  'aria-labelledby'?: string
}

const DIALOG_WIDTHS = { xs: 448, sm: 600, md: 900, lg: 1200, xl: 1536 }

/** A modal dialog (Radix Dialog): focus-trapped, Escape and backdrop close. */
export function Dialog({ open, onClose, fullWidth = false, fullScreen = false, maxWidth = 'sm', slotProps, sx, children }: DialogProps) {
  const theme = useTheme()
  const reason = useRef<CloseReason>('backdropClick')
  const content = useRef<HTMLDivElement | null>(null)
  const [titles, setTitles] = useState(0)
  const returnFocus = useReturnFocus(open)
  const registerTitle = () => {
    setTitles((n) => n + 1)
    return () => setTitles((n) => n - 1)
  }
  const container: SxObject = {
    position: 'fixed',
    inset: 0,
    zIndex: theme.zIndex.modal,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor: 'rgba(0, 0, 0, 0.3)',
    backdropFilter: 'blur(4px)',
    WebkitTapHighlightColor: 'transparent',
    outline: 0,
    ...fade,
  }
  const paper: SxObject = {
    backgroundColor: theme.palette.background.paper,
    color: theme.palette.text.primary,
    borderRadius: theme.radius['4xl'],
    boxShadow: `${theme.elevation.ring}, ${theme.elevation.xl}`,
    ...grow('center'),
    margin: 32,
    position: 'relative',
    overflowY: 'auto',
    display: 'flex',
    flexDirection: 'column',
    maxHeight: 'calc(100% - 64px)',
    outline: 0,
    ...(maxWidth ? { maxWidth: DIALOG_WIDTHS[maxWidth] } : { maxWidth: 'calc(100% - 64px)' }),
    ...(fullWidth ? { width: 'calc(100% - 64px)' } : {}),
    ...(fullScreen ? { margin: 0, width: '100%', maxWidth: '100%', height: '100%', maxHeight: 'none', borderRadius: 0 } : {}),
  }
  return (
    <DialogContext.Provider value={{ registerTitle }}>
      <RadixDialog.Root
        open={open}
        onOpenChange={(o) => {
          if (!o) onClose?.({}, reason.current)
        }}
      >
        <RadixDialog.Portal>
          <RadixDialog.Overlay asChild>
            {render(
              'div',
              {
                className: 'ui-Dialog-container',
                children: (
                  <RadixDialog.Content
                    asChild
                    aria-describedby={undefined}
                    onOpenAutoFocus={(e) => {
                      // Like the old dialogs: focus the dialog itself, not
                      // its first button — unless a field asked for
                      // `autoFocus`. React focuses that field on mount, but a
                      // dialog opened from inside another one can lose it to
                      // the outer focus trap, so it's focused again here.
                      e.preventDefault()
                      const el = content.current
                      const wanted = el?.querySelector<HTMLElement>('[data-autofocus]')
                      if (wanted) wanted.focus()
                      else if (el && !el.contains(document.activeElement)) el.focus()
                    }}
                    onCloseAutoFocus={returnFocus}
                    onEscapeKeyDown={() => {
                      reason.current = 'escapeKeyDown'
                    }}
                    onPointerDownOutside={() => {
                      reason.current = 'backdropClick'
                    }}
                  >
                    {render(
                      'div',
                      {
                        ref: content,
                        tabIndex: -1,
                        className: cx('ui-Dialog-paper', slotProps?.paper?.className),
                        children: (
                          <>
                            {titles === 0 && (
                              <VisuallyHidden.Root asChild>
                                <RadixDialog.Title>Dialog</RadixDialog.Title>
                              </VisuallyHidden.Root>
                            )}
                            {children}
                          </>
                        ),
                      },
                      paper,
                      slotProps?.paper?.sx,
                      theme,
                    )}
                  </RadixDialog.Content>
                ),
              },
              container,
              [slotProps?.backdrop?.sx, sx] as SxProps,
              theme,
            )}
          </RadixDialog.Overlay>
        </RadixDialog.Portal>
      </RadixDialog.Root>
    </DialogContext.Provider>
  )
}

export function DialogTitle({ sx, children, className, ...rest }: BoxProps) {
  const theme = useTheme()
  const ctx = useContext(DialogContext)
  // biome-ignore lint/correctness/useExhaustiveDependencies: register once
  useEffect(() => ctx?.registerTitle(), [])
  return (
    <RadixDialog.Title asChild>
      {render('h2', { className: cx('ui-DialogTitle', className), ...rest, children }, { ...theme.typography.h6, lineHeight: 1.2, margin: 0, padding: '24px 24px 8px', flex: '0 0 auto' }, sx, theme)}
    </RadixDialog.Title>
  )
}

export function DialogContent({ dividers, sx, children, className, ...rest }: BoxProps & { dividers?: boolean }) {
  const theme = useTheme()
  const styles: SxObject = {
    flex: '1 1 auto',
    WebkitOverflowScrolling: 'touch',
    overflowY: 'auto',
    padding: '20px 24px',
    ...(dividers ? { padding: '16px 24px', borderTop: `1px solid ${theme.palette.divider}`, borderBottom: `1px solid ${theme.palette.divider}` } : { '.ui-DialogTitle + &': { paddingTop: 8 } }),
  }
  return render('div', { className: cx('ui-DialogContent', className), ...rest, children }, styles, sx, theme)
}

export function DialogActions({ sx, children, className, ...rest }: BoxProps) {
  const theme = useTheme()
  return render(
    'div',
    { className: cx('ui-DialogActions', className), ...rest, children },
    { display: 'flex', alignItems: 'center', padding: '8px 24px 24px', justifyContent: 'flex-end', flex: '0 0 auto', gap: 8 },
    sx,
    theme,
  )
}

export function DialogContentText({ variant = 'body1', sx, children, ...rest }: BoxProps & { variant?: 'body1' | 'body2' }) {
  const theme = useTheme()
  return render('p', { ...rest, children }, { ...theme.typography[variant], margin: 0, color: theme.palette.text.secondary }, sx, theme)
}

// ── Tooltip ───────────────────────────────────────────────────────────────

type Placement = 'top' | 'bottom' | 'left' | 'right' | `${'top' | 'bottom' | 'left' | 'right'}-${'start' | 'end'}`

export interface TooltipProps {
  title: ReactNode
  children: ReactElement
  placement?: Placement
  enterDelay?: number
  disableInteractive?: boolean
  open?: boolean
  arrow?: boolean
}

/** A hover/focus label (Radix Tooltip). An empty `title` renders just the
 * child. The child must accept a ref. */
export function Tooltip({ title, children, placement = 'bottom', enterDelay, disableInteractive, open }: TooltipProps) {
  const theme = useTheme()
  if (title === '' || title === null || title === undefined || title === false) return children
  const [side, align] = placement.split('-') as ['top' | 'bottom' | 'left' | 'right', 'start' | 'end' | undefined]
  // Terax tooltips are inverted (ink on the page colour) and slide 2px in
  // from the side they sit on.
  const t = theme.palette.tokens
  const from = { top: 'translateY(2px)', bottom: 'translateY(-2px)', left: 'translateX(2px)', right: 'translateX(-2px)' }[side]
  const styles: SxObject = {
    backgroundColor: t.foreground,
    color: t.background,
    borderRadius: theme.radius.xl,
    fontFamily: theme.typography.fontFamily,
    padding: '6px 12px',
    fontSize: theme.typography.pxToRem(12),
    lineHeight: 1.35,
    maxWidth: 300,
    wordWrap: 'break-word',
    fontWeight: 500,
    zIndex: theme.zIndex.tooltip,
    '&[data-state="delayed-open"], &[data-state="instant-open"]': { animation: `${keyframes({ from: { opacity: 0, transform: from }, to: { opacity: 1, transform: 'none' } })} 140ms ${PREMIUM}` },
    '&[data-state="closed"]': { animation: `${fadeOut} 80ms linear` },
    '& kbd': { fontFamily: 'inherit', fontSize: '0.9em', padding: '1px 5px', borderRadius: theme.radius.sm, backgroundColor: alpha(t.background, 0.2) },
  }
  return (
    <RadixTooltip.Root delayDuration={enterDelay} disableHoverableContent={disableInteractive} open={open}>
      <RadixTooltip.Trigger asChild>{children}</RadixTooltip.Trigger>
      <RadixTooltip.Portal>
        <RadixTooltip.Content asChild side={side} align={align ?? 'center'} sideOffset={6} collisionPadding={8}>
          {render('div', { className: 'ui-Tooltip', children: title }, styles, undefined, theme)}
        </RadixTooltip.Content>
      </RadixTooltip.Portal>
    </RadixTooltip.Root>
  )
}

// ── Snackbar (Radix Toast) ────────────────────────────────────────────────

export interface SnackbarProps {
  open: boolean
  message: ReactNode
  autoHideDuration?: number | null
  onClose?: (event: object | null, reason: 'timeout' | 'clickaway' | 'escapeKeyDown') => void
  anchorOrigin?: { vertical: 'top' | 'bottom'; horizontal: 'left' | 'center' | 'right' }
  action?: ReactNode
}

/** A brief message at the edge of the window (Radix Toast). */
export function Snackbar({ open, message, autoHideDuration = null, onClose, anchorOrigin = { vertical: 'bottom', horizontal: 'left' }, action }: SnackbarProps) {
  const theme = useTheme()
  const { vertical, horizontal } = anchorOrigin
  const viewport: SxObject = {
    position: 'fixed',
    zIndex: theme.zIndex.snackbar,
    display: 'flex',
    margin: 0,
    padding: 0,
    listStyle: 'none',
    outline: 0,
    [vertical]: 24,
    ...(horizontal === 'center' ? { left: '50%', transform: 'translateX(-50%)' } : { [horizontal]: 24 }),
  }
  const content: SxObject = {
    ...theme.typography.body2,
    color: theme.palette.text.primary,
    backgroundColor: theme.palette.background.paper,
    boxShadow: `${theme.elevation.ring}, ${theme.elevation.lg}`,
    display: 'flex',
    alignItems: 'center',
    flexWrap: 'wrap',
    padding: '4px 16px',
    borderRadius: theme.radius['2xl'],
    minWidth: 240,
    ...grow('center'),
    '& .ui-Snackbar-message': { padding: '8px 0' },
    '& .ui-Snackbar-action': { display: 'flex', alignItems: 'center', marginLeft: 'auto', paddingLeft: 16, marginRight: -8 },
  }
  return (
    <Toast.Provider duration={autoHideDuration ?? Number.POSITIVE_INFINITY} swipeDirection={horizontal === 'right' ? 'right' : horizontal === 'left' ? 'left' : 'down'}>
      <Toast.Root asChild open={open} onOpenChange={(o) => !o && onClose?.(null, 'timeout')} onEscapeKeyDown={() => onClose?.(null, 'escapeKeyDown')}>
        {render(
          'li',
          {
            className: 'ui-Snackbar',
            children: (
              <>
                <Toast.Description className="ui-Snackbar-message">{message}</Toast.Description>
                {action && <div className="ui-Snackbar-action">{action}</div>}
              </>
            ),
          },
          content,
          undefined,
          theme,
        )}
      </Toast.Root>
      <Toast.Viewport asChild>{render('ol', {}, viewport, undefined, theme)}</Toast.Viewport>
    </Toast.Provider>
  )
}

// ── Collapse ──────────────────────────────────────────────────────────────

export interface CollapseProps extends BoxProps {
  in?: boolean
  unmountOnExit?: boolean
  timeout?: number
}

/** Animates its content's height open and closed. */
export function Collapse({ in: open = false, unmountOnExit = false, timeout = 250, sx, children, className, ...rest }: CollapseProps) {
  const theme = useTheme()
  const outer = useRef<HTMLDivElement | null>(null)
  const inner = useRef<HTMLDivElement | null>(null)
  const [mounted, setMounted] = useState(open)
  const [state, setState] = useState<'entered' | 'exited' | 'entering' | 'exiting'>(open ? 'entered' : 'exited')
  const first = useRef(true)

  useLayoutEffect(() => {
    if (first.current) {
      first.current = false
      return
    }
    if (open) {
      setMounted(true)
      setState('entering')
    } else {
      setState('exiting')
    }
  }, [open])

  useLayoutEffect(() => {
    const el = outer.current
    const content = inner.current
    if (!el || !content) return
    if (state === 'entering') {
      el.style.height = '0px'
      void el.offsetHeight
      el.style.height = `${content.offsetHeight}px`
      const t = window.setTimeout(() => setState('entered'), timeout)
      return () => window.clearTimeout(t)
    }
    if (state === 'exiting') {
      el.style.height = `${content.offsetHeight}px`
      void el.offsetHeight
      el.style.height = '0px'
      const t = window.setTimeout(() => {
        setState('exited')
        if (unmountOnExit) setMounted(false)
      }, timeout)
      return () => window.clearTimeout(t)
    }
    el.style.height = state === 'entered' ? 'auto' : '0px'
  }, [state, timeout, unmountOnExit])

  if (!mounted && unmountOnExit) return null
  return render(
    'div',
    {
      ref: outer,
      className: cx('ui-Collapse', className),
      ...rest,
      children: (
        <div ref={inner} style={{ display: 'flex', width: '100%' }}>
          <div style={{ width: '100%' }}>{children}</div>
        </div>
      ),
    },
    {
      height: 0,
      overflow: 'hidden',
      transition: `height ${timeout}ms cubic-bezier(0.4, 0, 0.2, 1) 0ms`,
      ...(state === 'entered' ? { height: 'auto', overflow: 'visible' } : {}),
      ...(state === 'exited' ? { visibility: 'hidden' } : {}),
    },
    sx,
    theme,
  )
}

// ── Alert ─────────────────────────────────────────────────────────────────

const ALERT_ICONS: Record<'success' | 'info' | 'warning' | 'error', string> = {
  success:
    'M20,12A8,8 0 0,1 12,20A8,8 0 0,1 4,12A8,8 0 0,1 12,4C12.76,4 13.5,4.11 14.2, 4.31L15.77,2.74C14.61,2.26 13.34,2 12,2A10,10 0 0,0 2,12A10,10 0 0,0 12,22A10,10 0 0,0 22,12M7.91,10.08L6.5,11.5L11,16L21,6L19.59,4.58L11,13.17L7.91,10.08Z',
  warning: 'M12 5.99L19.53 19H4.47L12 5.99M12 2L1 21h22L12 2zm1 14h-2v2h2v-2zm0-6h-2v4h2v-4z',
  error:
    'M11 15h2v2h-2zm0-8h2v6h-2zm.99-5C6.47 2 2 6.48 2 12s4.47 10 9.99 10C17.52 22 22 17.52 22 12S17.52 2 11.99 2zM12 20c-4.42 0-8-3.58-8-8s3.58-8 8-8 8 3.58 8 8-3.58 8-8 8z',
  info: 'M11,9H13V7H11M12,20C7.59,20 4,16.41 4,12C4,7.59 7.59,4 12,4C16.41,4 20,7.59 20, 12C20,16.41 16.41,20 12,20M12,2A10,10 0 0,0 2,12A10,10 0 0,0 12,22A10, 10 0 0,0 12,2M11,17H13V11H11V17Z',
}

export type AlertProps = Omit<BoxProps, 'action'> & { severity?: 'success' | 'info' | 'warning' | 'error'; action?: ReactNode; icon?: ReactNode | false }

/** An inline status message. */
export function Alert({ severity = 'success', action, icon, sx, children, className, ...rest }: AlertProps) {
  const theme = useTheme()
  // A Terax card with a coloured icon: the tone does the work, not a
  // tinted fill. Errors also colour their text.
  const p = theme.palette
  const c = severity === 'info' ? p.tokens.mutedForeground : p[severity as ColorName].main
  const styles: SxObject = {
    ...theme.typography.body2,
    backgroundColor: severity === 'error' ? alpha(p.error.main, p.mode === 'dark' ? 0.1 : 0.06) : p.background.card,
    color: severity === 'error' ? p.error.main : p.text.primary,
    border: `1px solid ${severity === 'error' ? alpha(p.error.main, 0.25) : p.divider}`,
    borderRadius: theme.radius['2xl'],
    display: 'flex',
    gap: 10,
    padding: '8px 14px',
    '& .ui-Alert-icon': { padding: '3px 0', display: 'flex', fontSize: 16, color: c, flexShrink: 0 },
    '& .ui-Alert-message': { padding: '2px 0', minWidth: 0, overflow: 'auto', lineHeight: 1.5 },
    '& .ui-Alert-action': { display: 'flex', alignItems: 'flex-start', paddingLeft: 12, marginLeft: 'auto', marginRight: -6 },
  }
  return render(
    'div',
    {
      role: 'alert',
      className: cx('ui-Alert', className),
      ...rest,
      children: (
        <>
          {icon !== false && (
            <div className="ui-Alert-icon">
              {icon ?? (
                <svg viewBox="0 0 24 24" width="1em" height="1em" fill="currentColor" aria-hidden="true">
                  <path d={ALERT_ICONS[severity]} />
                </svg>
              )}
            </div>
          )}
          <div className="ui-Alert-message">{children}</div>
          {action && <div className="ui-Alert-action">{action}</div>}
        </>
      ),
    },
    styles,
    sx,
    theme,
  )
}

// ── Progress ──────────────────────────────────────────────────────────────

const spin = keyframes({ '0%': { transform: 'rotate(0deg)' }, '100%': { transform: 'rotate(360deg)' } })
const dash = keyframes({
  '0%': { strokeDasharray: '1px, 200px', strokeDashoffset: 0 },
  '50%': { strokeDasharray: '100px, 200px', strokeDashoffset: '-15px' },
  '100%': { strokeDasharray: '1px, 200px', strokeDashoffset: '-126px' },
})

export type CircularProgressProps = BoxProps & { size?: number | string; thickness?: number; color?: ColorName | 'inherit'; value?: number; variant?: 'indeterminate' | 'determinate' }

/** A spinner, or a ring filled to `value` percent. */
export function CircularProgress({ size = 40, thickness = 3.6, color = 'primary', value = 0, variant = 'indeterminate', sx, className, ...rest }: CircularProgressProps) {
  const theme = useTheme()
  const SIZE = 44
  const r = (SIZE - thickness) / 2
  const circumference = 2 * Math.PI * r
  const determinate = variant === 'determinate'
  const styles: SxObject = {
    display: 'inline-block',
    width: size,
    height: size,
    color: color === 'inherit' ? 'inherit' : theme.palette[color].main,
    ...(determinate ? { transform: 'rotate(-90deg)', transition: 'transform 300ms cubic-bezier(0.4, 0, 0.2, 1) 0ms' } : { animation: `${spin} 1.4s linear infinite` }),
    '& circle': determinate
      ? { stroke: 'currentColor', transition: 'stroke-dashoffset 300ms cubic-bezier(0.4, 0, 0.2, 1) 0ms' }
      : { stroke: 'currentColor', strokeDasharray: '80px, 200px', strokeDashoffset: 0, animation: `${dash} 1.4s ease-in-out infinite` },
  }
  return render(
    'span',
    {
      role: 'progressbar',
      'aria-valuenow': determinate ? Math.round(value) : undefined,
      className: cx('ui-CircularProgress', className),
      ...rest,
      children: (
        <svg viewBox={`${SIZE / 2} ${SIZE / 2} ${SIZE} ${SIZE}`} style={{ display: 'block' }}>
          <circle
            cx={SIZE}
            cy={SIZE}
            r={r}
            fill="none"
            strokeWidth={thickness}
            style={determinate ? { strokeDasharray: circumference.toFixed(3), strokeDashoffset: `${(((100 - value) / 100) * circumference).toFixed(3)}px` } : undefined}
          />
        </svg>
      ),
    },
    styles,
    sx,
    theme,
  )
}

const bar1 = keyframes({ '0%': { left: '-35%', right: '100%' }, '60%': { left: '100%', right: '-90%' }, '100%': { left: '100%', right: '-90%' } })
const bar2 = keyframes({ '0%': { left: '-200%', right: '100%' }, '60%': { left: '107%', right: '-8%' }, '100%': { left: '107%', right: '-8%' } })

export type LinearProgressProps = BoxProps & { value?: number; variant?: 'indeterminate' | 'determinate'; color?: ColorName }

/** A thin bar: indeterminate, or filled to `value` percent (Radix Progress). */
export function LinearProgress({ value = 0, variant = 'indeterminate', color = 'primary', sx, className, ...rest }: LinearProgressProps) {
  const theme = useTheme()
  const main = theme.palette[color].main
  const determinate = variant === 'determinate'
  const styles: SxObject = {
    position: 'relative',
    overflow: 'hidden',
    display: 'block',
    height: 4,
    zIndex: 0,
    borderRadius: theme.radius.full,
    backgroundColor: alpha(main, 0.15),
    '& .ui-LinearProgress-bar': {
      width: '100%',
      position: 'absolute',
      left: 0,
      bottom: 0,
      top: 0,
      transformOrigin: 'left',
      borderRadius: 'inherit',
      backgroundColor: main,
      transition: 'transform .4s linear',
    },
    '& .ui-LinearProgress-bar1': determinate ? {} : { width: 'auto', animation: `${bar1} 2.1s cubic-bezier(0.65, 0.815, 0.735, 0.395) infinite` },
    '& .ui-LinearProgress-bar2': { width: 'auto', animation: `${bar2} 2.1s cubic-bezier(0.165, 0.84, 0.44, 1) 1.15s infinite` },
  }
  return (
    <Progress.Root value={determinate ? value : null} max={100} asChild>
      {render(
        'span',
        {
          className: cx('ui-LinearProgress', className),
          ...rest,
          children: (
            <>
              <Progress.Indicator className="ui-LinearProgress-bar ui-LinearProgress-bar1" style={determinate ? { transform: `translateX(${value - 100}%)` } : undefined} />
              {!determinate && <span className="ui-LinearProgress-bar ui-LinearProgress-bar2" />}
            </>
          ),
        },
        styles,
        sx,
        theme,
      )}
    </Progress.Root>
  )
}

