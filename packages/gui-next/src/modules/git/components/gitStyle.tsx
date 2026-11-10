import type { ReactNode } from 'react'
import { ButtonBase } from '../../../ui'

/**
 * The Git view's measures: everything on a 4px grid, and five type sizes.
 * The app's spacing unit is 7px, so the view spells its spacing out in pixels
 * through `u` instead of theme units.
 */
export const u = (n: number) => `${n * 4}px`

export const MONO_FONT = '"Geist Mono", ui-monospace, monospace'

export const T = {
  /** A detail's heading. */
  title: { fontSize: '0.875rem', lineHeight: '20px', fontWeight: 600 },
  /** Row titles, prose. */
  body: { fontSize: '0.8125rem', lineHeight: '20px' },
  /** Buttons, tabs, secondary lines. */
  small: { fontSize: '0.75rem', lineHeight: '16px' },
  /** Meta lines under a title, counts, times. */
  meta: { fontSize: '0.75rem', lineHeight: '16px' },
  /** Section labels. */
  label: { fontSize: '0.75rem', lineHeight: '16px', fontWeight: 600, letterSpacing: '0.04em', textTransform: 'uppercase' },
  mono: { fontFamily: MONO_FONT },
} as const

/** Buttons in the view: 24px tall, 8px in, 6px corners. */
export const BTN_SX = { height: 24, minHeight: 24, minWidth: 0, px: u(2), py: 0, fontSize: '0.75rem', lineHeight: '16px', borderRadius: '6px', whiteSpace: 'nowrap' } as const

/** A filter choice: 20px, flat when picked, outlined when not. */
export function Pill({ active, onClick, children, title }: { active: boolean; onClick: () => void; children: ReactNode; title?: string }) {
  return (
    <ButtonBase
      onClick={onClick}
      title={title}
      aria-pressed={active}
      sx={{
        height: 20,
        px: u(2),
        borderRadius: '4px',
        ...T.meta,
        fontWeight: active ? 600 : 500,
        whiteSpace: 'nowrap',
        maxWidth: 200,
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        display: 'inline-block',
        color: active ? 'text.primary' : 'text.secondary',
        bgcolor: active ? 'action.selected' : 'transparent',
        border: 1,
        borderColor: active ? 'transparent' : 'divider',
        '&:hover': { color: 'text.primary', bgcolor: active ? 'action.selected' : 'action.hover' },
      }}
    >
      {children}
    </ButtonBase>
  )
}
