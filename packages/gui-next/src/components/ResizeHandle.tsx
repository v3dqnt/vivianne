import { useCallback } from 'react'
import type { RefObject } from 'react'
import { Box } from '../ui'

/**
 * Drag-to-resize edge. `edge` is the side of the panel the handle sits on,
 * which also decides the drag axis and its sign: a panel docked right grows
 * as the pointer moves left, one docked bottom grows as it moves up, and one
 * docked LEFT (handle on its right edge — IDE layout moves the dock there)
 * grows as it moves right.
 */
export function ResizeHandle({
  edge,
  size,
  min,
  max,
  onResize,
  targetRef,
  onDragActiveChange,
}: {
  edge: 'left' | 'right' | 'top'
  size: number
  min: number
  max: number
  onResize: (next: number) => void
  /** The panel element being resized. Sized directly (bypassing React/the
   * store) for the duration of the drag — see `onPointerDown` below. */
  targetRef: RefObject<HTMLElement | null>
  /** Fires `true` at drag start and `false` at drag end. Lets a panel's
   * contents (e.g. the terminal's xterm `fit()`, which runs off a
   * ResizeObserver on the panel and can't be silenced by skipping React
   * state) suppress expensive per-frame work while the size is in flux. */
  onDragActiveChange?: (active: boolean) => void
}) {
  const onPointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault()
      const horizontal = edge === 'left' || edge === 'right'
      const start = horizontal ? e.clientX : e.clientY
      const startSize = size
      const dimProp = horizontal ? 'width' : 'height'
      let dragValue = startSize

      const onMove = (ev: PointerEvent) => {
        const delta = (horizontal ? ev.clientX : ev.clientY) - start
        // Sign depends on which side the handle is on. A handle on the
        // panel's LEFT or TOP grows the panel as the pointer moves away from
        // it (negative delta); one on its RIGHT grows with a positive delta.
        // Getting this backwards makes the panel shrink when you drag it
        // bigger, which reads as the handle being broken rather than inverted.
        const signed = edge === 'right' ? startSize + delta : startSize - delta
        dragValue = Math.round(Math.min(max, Math.max(min, signed)))
        // Mutate the panel's own size directly during the drag. Going
        // through the store here — `set` + a synchronous localStorage
        // `persist()` — on every `pointermove` re-rendered up to 1200
        // unvirtualized tree rows per tick (the file tree) or forced an xterm
        // `fit()` + resize IPC per tick (a terminal tab). The store is only
        // touched once, in `onUp`, below.
        const el = targetRef.current
        if (el) el.style[dimProp] = `${dragValue}px`
      }
      const onUp = () => {
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
        document.body.style.userSelect = ''
        if (dragValue !== startSize) onResize(dragValue)
        // Hand sizing back to React's own `sx` — the inline style set above
        // would otherwise outrank it (inline always beats a stylesheet
        // class) and silently freeze the panel at this size forever.
        const el = targetRef.current
        if (el) el.style[dimProp] = ''
        onDragActiveChange?.(false)
      }
      // Without this a fast drag selects text across the whole app.
      document.body.style.userSelect = 'none'
      onDragActiveChange?.(true)
      window.addEventListener('pointermove', onMove)
      window.addEventListener('pointerup', onUp)
    },
    [edge, size, min, max, onResize, targetRef, onDragActiveChange],
  )

  return (
    <Box
      onPointerDown={onPointerDown}
      sx={{
        position: 'absolute',
        zIndex: 10,
        ...(edge === 'top'
          ? { top: -3, left: 0, right: 0, height: 6, cursor: 'row-resize' }
          : edge === 'left'
            ? { left: -3, top: 0, bottom: 0, width: 6, cursor: 'col-resize' }
            : { right: -3, top: 0, bottom: 0, width: 6, cursor: 'col-resize' }),
        '&:hover': { bgcolor: 'primary.main', opacity: 0.35 },
      }}
    />
  )
}
