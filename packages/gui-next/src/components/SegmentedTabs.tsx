import { useLayoutEffect, useRef, type ReactNode } from 'react'
import { Box, ButtonBase, Stack } from '../ui'

export interface SegmentedTab<T extends string> {
  id: T
  label: string
  icon?: ReactNode
}

const DURATION = '250ms'
const EASE = 'cubic-bezier(0.22, 1, 0.36, 1)'

/**
 * The app's segmented tabs: a muted track with the chosen tab on a raised chip that slides to the tab you pick (the sliding-tabs
 * pattern from Transitions.dev, in the app's own colors and corner radius). One component, so Home's Projects / 24/7 agents switch and
 * the project rail's tabs look and move the same. `fill` stretches the tabs to share the track's width; `compact` shows icons only.
 *
 * The chip's position and width are measured from the active tab and written straight to the element, so the CSS transition tweens
 * from the old tab to the new one. A change of size (the rail widening, the window resizing) snaps it into place without animating.
 */
export function SegmentedTabs<T extends string>({ tabs, value, onChange, fill = false, compact = false }: { tabs: SegmentedTab<T>[]; value: T; onChange: (id: T) => void; fill?: boolean; compact?: boolean }) {
  const track = useRef<HTMLDivElement | null>(null)
  const chip = useRef<HTMLSpanElement | null>(null)
  const buttons = useRef(new Map<T, HTMLElement>())
  const placed = useRef(false)
  // The tab to sit on, read by the resize watcher, which lives as long as the component does.
  const current = useRef(value)
  current.current = value

  const place = (animate: boolean) => {
    const el = buttons.current.get(current.current)
    const c = chip.current
    if (!el || !c) return
    if (!animate) c.style.transition = 'none'
    c.style.transform = `translateX(${el.offsetLeft}px)`
    c.style.width = `${el.offsetWidth}px`
    c.style.opacity = '1'
    if (!animate) {
      void c.offsetWidth // commit the jump before the transition comes back
      c.style.transition = ''
    }
  }

  // Slide when the chosen tab changes; jump into place the first time.
  useLayoutEffect(() => {
    place(placed.current)
    placed.current = true
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value, tabs.length, compact])

  // Keep it on its tab while the track changes size. Watched once for the component's whole life: re-subscribing on every tab
  // change fires straight away and snaps the chip, which would cancel the slide.
  useLayoutEffect(() => {
    const t = track.current
    if (!t) return
    let first = true
    const ro = new ResizeObserver(() => {
      if (first) {
        first = false
        return
      }
      place(false)
    })
    ro.observe(t)
    return () => ro.disconnect()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return (
    <Stack
      ref={track}
      direction="row"
      role="tablist"
      sx={{ position: 'relative', gap: '4px', p: '2px', borderRadius: '8px', bgcolor: 'action.hover', alignSelf: fill ? 'stretch' : 'flex-start' }}
    >
      <Box
        component="span"
        ref={chip}
        aria-hidden
        sx={{
          position: 'absolute',
          top: '2px',
          left: 0,
          height: 28,
          width: 0,
          opacity: 0,
          borderRadius: '6px',
          bgcolor: 'background.paper',
          pointerEvents: 'none',
          zIndex: 0,
          willChange: 'transform, width',
          transition: `transform ${DURATION} ${EASE}, width ${DURATION} ${EASE}`,
          '@media (prefers-reduced-motion: reduce)': { transition: 'none' },
        }}
      />
      {tabs.map(({ id, label, icon }) => {
        const active = id === value
        return (
          <ButtonBase
            key={id}
            ref={(el: HTMLElement | null) => {
              if (el) buttons.current.set(id, el)
              else buttons.current.delete(id)
            }}
            role="tab"
            aria-selected={active}
            aria-label={label}
            title={compact ? label : undefined}
            onClick={() => onChange(id)}
            sx={{
              position: 'relative',
              zIndex: 1,
              flex: fill ? '1 1 0' : '0 0 auto',
              minWidth: 0,
              height: 28,
              px: compact ? '8px' : '12px',
              gap: '6px',
              borderRadius: '6px',
              fontSize: '0.8125rem',
              fontWeight: active ? 600 : 500,
              color: active ? 'text.primary' : 'text.secondary',
              transition: `color ${DURATION} ${EASE}`,
              '&:hover': { color: 'text.primary' },
              '@media (prefers-reduced-motion: reduce)': { transition: 'none' },
            }}
          >
            {icon && <Box sx={{ display: 'flex', flexShrink: 0 }}>{icon}</Box>}
            {!compact && (
              <Box component="span" className="seg-label" sx={{ whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                {label}
              </Box>
            )}
          </ButtonBase>
        )
      })}
    </Stack>
  )
}
