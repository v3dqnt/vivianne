import { useCallback } from 'react'
import type { RefObject } from 'react'
import Box from '@mui/material/Box'

/**
 * Drag-to-resize edge. `edge` is the side of the panel the handle sits on,
 * which also decides the drag axis and its sign: a panel docked right grows
 * as the pointer moves left, one docked bottom grows as it moves up.
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
  edge: 'left' | 'top'
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
      const horizontal = edge === 'left'
      const start = horizontal ? e.clientX : e.clientY
      const startSize = size
      const dimProp = horizontal ? 'width' : 'height'
      let dragValue = startSize

      const onMove = (ev: PointerEvent) => {
        const delta = (horizontal ? ev.clientX : ev.clientY) - start
        dragValue = Math.round(Math.min(max, Math.max(min, startSize - delta)))
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

  const horizontal = edge === 'left'
  return (
    <Box
      onPointerDown={onPointerDown}
      sx={{
        position: 'absolute',
        zIndex: 10,
        ...(horizontal
          ? { left: -3, top: 0, bottom: 0, width: 6, cursor: 'col-resize' }
          : { top: -3, left: 0, right: 0, height: 6, cursor: 'row-resize' }),
        '&:hover': { bgcolor: 'primary.main', opacity: 0.35 },
      }}
    />
  )
}
