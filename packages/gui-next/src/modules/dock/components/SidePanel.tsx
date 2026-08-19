import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import Tooltip from '@mui/material/Tooltip'
import { CloseCircleLinear } from '../../../icons'
import { ResizeHandle } from '../../../components/ResizeHandle'
import { usePanelStore, RIGHT_PANEL_LABELS } from '../store/panelStore'
import { WorkspaceTree } from './WorkspaceTree'
import { LivePreviewPanel } from './LivePreviewPanel'
import { BackgroundTasksPanel } from './BackgroundTasksPanel'
import { BrowserPanel } from './BrowserPanel'
import { ReviewPanel } from '../../git/components/ReviewPanel'

const MIN_WIDTH = 300
const MAX_WIDTH = 900

/** Right-hand panel host. Which panel it shows is chosen from the title
 * bar's Panels menu; there's no permanent icon rail any more. */
export function SidePanel() {
  const open = usePanelStore((s) => s.rightOpen)
  const kind = usePanelStore((s) => s.rightKind)
  const width = usePanelStore((s) => s.rightWidth)
  const setWidth = usePanelStore((s) => s.setRightWidth)
  const close = usePanelStore((s) => s.closeRight)

  if (!open) return null

  return (
    <Box sx={{ position: 'relative', width, flexShrink: 0, height: '100%', borderLeft: 1, borderColor: 'divider', bgcolor: 'background.default' }}>
      <ResizeHandle edge="left" size={width} min={MIN_WIDTH} max={MAX_WIDTH} onResize={setWidth} />
      <Stack sx={{ height: '100%', minHeight: 0 }}>
        {/* The Review panel draws its own richer header (tabs + view mode),
            so it opts out of this plain one. */}
        {kind !== 'review' && (
          <Stack direction="row" sx={{ alignItems: 'center', px: 1.5, py: 0.75, gap: 1, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
            <Typography variant="caption" sx={{ fontWeight: 600 }}>
              {RIGHT_PANEL_LABELS[kind]}
            </Typography>
            <Box sx={{ flex: 1 }} />
            <Tooltip title="Close panel">
              <IconButton size="small" onClick={close} sx={{ width: 22, height: 22 }}>
                <CloseCircleLinear size={13} color="currentColor" />
              </IconButton>
            </Tooltip>
          </Stack>
        )}
        <Box sx={{ flex: 1, minHeight: 0, minWidth: 0, display: 'flex' }}>
          {kind === 'review' && <ReviewPanel onClose={close} />}
          {kind === 'files' && <WorkspaceTree />}
          {kind === 'live' && <LivePreviewPanel />}
          {kind === 'browser' && <BrowserPanel />}
          {kind === 'tasks' && <BackgroundTasksPanel />}
        </Box>
      </Stack>
    </Box>
  )
}
