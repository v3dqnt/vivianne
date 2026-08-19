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

  if (!open) return null

  return (
    <Box sx={{ position: 'relative', height, flexShrink: 0, borderTop: 1, borderColor: 'divider', bgcolor: 'background.default' }}>
      <ResizeHandle edge="top" size={height} min={MIN_HEIGHT} max={MAX_HEIGHT} onResize={setHeight} />
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
