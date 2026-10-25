import { Box, Stack } from '../../../ui'
import { usePanelStore } from '../store/panelStore'
import { LivePreviewPanel } from './LivePreviewPanel'
import { BrowserPanel } from './BrowserPanel'
import { LiveBrowserPanel } from './LiveBrowserPanel'
import { DocumentsMode } from '../../office/components/DocumentsMode'

/** One panel for everything that's previewed: the dev server, a live
 * webview, the agent's browser, and documents (PDF, Word, Excel,
 * PowerPoint — viewed and edited in place). Documents used to be their own
 * panel and a panel per file; they're a mode here now, with a tab per file.
 * There is no mode switcher: the mode is set by whatever opens the panel
 * (a file, a dev server, the agent's browser). */
export function PreviewPanel() {
  const mode = usePanelStore((s) => s.previewMode)

  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
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
