import Stack from '@mui/material/Stack'
import ToggleButton from '@mui/material/ToggleButton'
import ToggleButtonGroup from '@mui/material/ToggleButtonGroup'
import { usePanelStore } from '../store/panelStore'
import { LivePreviewPanel } from './LivePreviewPanel'
import { BrowserPanel } from './BrowserPanel'

/** Merged Live Preview + Browser panel: one dock slot, two modes. Dev server
 * mode is the old LivePreviewPanel body unchanged; Browser mode is
 * BrowserPanel, which itself shows the agent's live camoufox session when
 * one is active, or a manual address-bar iframe otherwise. */
export function PreviewPanel() {
  const mode = usePanelStore((s) => s.previewMode)
  const setMode = usePanelStore((s) => s.setPreviewMode)

  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', px: 1, py: 0.75, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <ToggleButtonGroup
          size="small"
          exclusive
          value={mode}
          onChange={(_, next) => next && setMode(next)}
          sx={{ '& .MuiToggleButton-root': { px: 1.25, py: 0.25, fontSize: '0.75rem', textTransform: 'none' } }}
        >
          <ToggleButton value="dev">Dev server</ToggleButton>
          <ToggleButton value="browser">Browser</ToggleButton>
        </ToggleButtonGroup>
      </Stack>
      <Stack sx={{ flex: 1, minHeight: 0 }}>{mode === 'dev' ? <LivePreviewPanel /> : <BrowserPanel />}</Stack>
    </Stack>
  )
}
