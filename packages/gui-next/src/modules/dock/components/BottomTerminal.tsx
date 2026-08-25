import { useEffect, useRef, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import Tooltip from '@mui/material/Tooltip'
import { CloseCircleLinear } from '../../../icons'
import { ResizeHandle } from '../../../components/ResizeHandle'
import { usePanelStore } from '../store/panelStore'
import { TerminalPanel } from './TerminalPanel'

const MIN_HEIGHT = 120
const MAX_HEIGHT = 720

/** Terminal docked to the bottom of the chat column, drag-resizable from its
 * top edge. Opaque `background.default` rather than the glass treatment the
 * side panels use — terminal text needs a flat, high-contrast backdrop to
 * stay readable, and xterm renders its own transparent background over it. */
export function BottomTerminal() {
  const open = usePanelStore((s) => s.bottomOpen)
  const height = usePanelStore((s) => s.bottomHeight)
  const setHeight = usePanelStore((s) => s.setBottomHeight)
  const close = usePanelStore((s) => s.closeBottom)
  const setPanelResizing = usePanelStore((s) => s.setPanelResizing)
  const panelRef = useRef<HTMLDivElement>(null)

  // Mount the terminal on first open, then keep it mounted (CSS-hidden) when
  // closed. Unmounting used to kill the PTY — losing shell state, cwd, and
  // scrollback — and reopening paid a fresh shell spawn every time. The
  // deferred mount also keeps xterm's sizing sane: it initializes while the
  // panel is actually visible, and afterwards just rides ResizeObserver.
  const [everOpened, setEverOpened] = useState(open)
  useEffect(() => {
    if (open) setEverOpened(true)
  }, [open])

  if (!everOpened) return null

  return (
    <Box
      ref={panelRef}
      sx={{
        position: 'relative',
        height: open ? height : 0,
        flexShrink: 0,
        borderTop: open ? 1 : 0,
        borderColor: 'divider',
        bgcolor: 'background.default',
        overflow: 'hidden',
        // Fully out of interaction/tab order while collapsed; visibility (not
        // display:none) so xterm's DOM survives intact for the reopen refit.
        visibility: open ? 'visible' : 'hidden',
        pointerEvents: open ? undefined : 'none',
      }}
    >
      <ResizeHandle
        edge="top"
        size={height}
        min={MIN_HEIGHT}
        max={MAX_HEIGHT}
        onResize={setHeight}
        targetRef={panelRef}
        onDragActiveChange={setPanelResizing}
      />
      <Stack sx={{ height: '100%', minHeight: 0 }}>
        <Stack direction="row" sx={{ alignItems: 'center', px: 1.5, py: 0.5, gap: 1, flexShrink: 0 }}>
          <Typography variant="caption" sx={{ fontWeight: 600, color: 'text.secondary' }}>
            Terminal
          </Typography>
          <Box sx={{ flex: 1 }} />
          <Tooltip title="Close terminal">
            <IconButton size="small" onClick={close} sx={{ width: 22, height: 22 }}>
              <CloseCircleLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        </Stack>
        <Box sx={{ flex: 1, minHeight: 0, display: 'flex' }}>
          <TerminalPanel />
        </Box>
      </Stack>
    </Box>
  )
}
