import { useCallback } from 'react'
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
}: {
  edge: 'left' | 'top'
  size: number
  min: number
  max: number
  onResize: (next: number) => void
}) {
  const onPointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault()
      const horizontal = edge === 'left'
      const start = horizontal ? e.clientX : e.clientY
      const startSize = size
      const onMove = (ev: PointerEvent) => {
        const delta = (horizontal ? ev.clientX : ev.clientY) - start
        onResize(Math.round(Math.min(max, Math.max(min, startSize - delta))))
      }
      const onUp = () => {
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
        document.body.style.userSelect = ''
      }
      // Without this a fast drag selects text across the whole app.
      document.body.style.userSelect = 'none'
      window.addEventListener('pointermove', onMove)
      window.addEventListener('pointerup', onUp)
    },
    [edge, size, min, max, onResize],
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
