import { Box, Stack, ToggleButton, ToggleButtonGroup } from '../../../ui'
import { usePanelStore } from '../store/panelStore'
import { LivePreviewPanel } from './LivePreviewPanel'
import { BrowserPanel } from './BrowserPanel'
import { LiveBrowserPanel } from './LiveBrowserPanel'
import { DocumentsMode } from '../../office/components/DocumentsMode'

/** One panel for everything that's previewed: the dev server, a live
 * webview, the agent's browser, and documents (PDF, Word, Excel,
 * PowerPoint — viewed and edited in place). Documents used to be their own
 * panel and a panel per file; they're a mode here now, with a tab per file. */
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
          sx={{ '& .ui-ToggleButton': { px: 1.25, py: 0.25, fontSize: '0.75rem', textTransform: 'none' } }}
        >
          <ToggleButton value="dev">Dev server</ToggleButton>
          <ToggleButton value="live">Live</ToggleButton>
          <ToggleButton value="browser">Agent view</ToggleButton>
          <ToggleButton value="documents">Documents</ToggleButton>
        </ToggleButtonGroup>
      </Stack>
      {/* All three stay mounted-by-mode rather than unmounted: `Live` owns an
          OS-level webview layered over the window, and remounting it would
          reload the page on every tab switch. It is told whether it is active
          instead, and parks itself off-screen when it is not. */}
      <Stack sx={{ flex: 1, minHeight: 0 }}>
        {mode === 'dev' && <LivePreviewPanel />}
        {mode === 'browser' && <BrowserPanel />}
        {mode === 'documents' && <DocumentsMode />}
        <Box sx={{ flex: mode === 'live' ? 1 : 0, minHeight: 0, display: mode === 'live' ? 'flex' : 'none' }}>
          <LiveBrowserPanel active={mode === 'live'} />
        </Box>
      </Stack>
    </Stack>
  )
}
