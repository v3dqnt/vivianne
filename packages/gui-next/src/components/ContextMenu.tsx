import { useCallback, useEffect, useState, type MouseEvent, type ReactNode } from 'react'
import { Box, Typography } from '../ui'
import { groupItems, resolveMenu, type MenuItem } from '../lib/contextMenu'

interface Open {
  x: number
  y: number
  items: MenuItem[]
}

const WIDTH = 220

/**
 * Right-click support for one target: `onContextMenu` goes on the element, `menu` is rendered anywhere (it is positioned to the viewport).
 * The items come from whatever is registered for `kind` (see `lib/contextMenu`).
 */
export function useContextMenu<T>(kind: string, target: T): { onContextMenu: (e: MouseEvent) => void; menu: ReactNode } {
  const [open, setOpen] = useState<Open | null>(null)
  const close = useCallback(() => setOpen(null), [])

  const onContextMenu = useCallback(
    (e: MouseEvent) => {
      e.preventDefault()
      e.stopPropagation()
      const { clientX, clientY } = e
      void resolveMenu(kind, target).then((items) => {
        if (items.length > 0) setOpen({ x: clientX, y: clientY, items })
      })
    },
    [kind, target],
  )

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && close()
    window.addEventListener('keydown', onKey)
    window.addEventListener('blur', close)
    window.addEventListener('resize', close)
    return () => {
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('blur', close)
      window.removeEventListener('resize', close)
    }
  }, [open, close])

  const menu = open && (
    <Box
      onMouseDown={close}
      onContextMenu={(e: MouseEvent) => {
        e.preventDefault()
        close()
      }}
      sx={{ position: 'fixed', inset: 0, zIndex: 1400 }}
    >
      <Box
        role="menu"
        onMouseDown={(e: MouseEvent) => e.stopPropagation()}
        sx={{
          position: 'fixed',
          // Kept inside the window: opens leftwards or upwards near an edge.
          left: Math.max(8, Math.min(open.x, window.innerWidth - WIDTH - 8)),
          top: Math.max(8, Math.min(open.y, window.innerHeight - 32 * open.items.length - 24)),
          width: WIDTH,
          py: 0.5,
          borderRadius: '8px',
          border: 1,
          borderColor: 'divider',
          bgcolor: 'background.paper',
          boxShadow: '0 12px 32px rgba(0,0,0,0.25)',
        }}
      >
        {groupItems(open.items).map((group, gi) => (
          <Box key={gi} sx={{ ...(gi > 0 && { borderTop: 1, borderColor: 'divider', mt: 0.5, pt: 0.5 }) }}>
            {group.map((item) => (
              <Box
                key={item.id}
                role="menuitem"
                tabIndex={0}
                onClick={() => {
                  close()
                  void Promise.resolve(item.run()).catch(() => undefined)
                }}
                onKeyDown={(e: React.KeyboardEvent) => {
                  if (e.key === 'Enter') {
                    close()
                    void Promise.resolve(item.run()).catch(() => undefined)
                  }
                }}
                sx={{ mx: 0.5, px: 1.25, height: 32, display: 'flex', alignItems: 'center', borderRadius: '6px', cursor: 'pointer', color: item.danger ? 'error.main' : 'text.primary', '&:hover, &:focus-visible': { bgcolor: 'action.hover', outline: 'none' } }}
              >
                <Typography noWrap sx={{ fontSize: '0.8125rem' }}>
                  {item.label}
                </Typography>
              </Box>
            ))}
          </Box>
        ))}
      </Box>
    </Box>
  )

  return { onContextMenu, menu }
}
