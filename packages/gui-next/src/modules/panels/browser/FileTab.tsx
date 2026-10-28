import { Box, Stack, Typography, IconButton, Tooltip } from '../../../ui'
import { CopyLinear, GlobalLinear } from '../../../icons'
import { call } from '../../../ipc'
import { FileContent } from '../../dock/components/FilePreview'
import { DocumentViewer } from '../../office/components/DocumentViewer'
import { isDocument } from '../../office/lib/docs'

/** A local file shown in the Browser panel: documents get the office viewer,
 * everything else the file previewer (images, media, text, PDFs). */
export function FileTab({ path }: { path: string }) {
  return (
    <Stack sx={{ flex: 1, minHeight: 0, minWidth: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, px: 1, py: 0.5, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <Typography variant="caption" color="text.secondary" noWrap sx={{ flex: 1, minWidth: 0, fontSize: '0.6875rem', fontFamily: '"Geist Mono", ui-monospace, monospace' }} title={path}>
          {path}
        </Typography>
        <Tooltip title="Copy path">
          <IconButton size="small" aria-label="Copy path" onClick={() => void navigator.clipboard.writeText(path)} sx={{ width: 20, height: 20 }}>
            <CopyLinear size={12} color="currentColor" />
          </IconButton>
        </Tooltip>
        <Tooltip title="Open with the system's default app">
          <IconButton size="small" aria-label="Open externally" onClick={() => void call('cmd_open_path', { path })} sx={{ width: 20, height: 20 }}>
            <GlobalLinear size={12} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Stack>
      <Box sx={{ flex: 1, minHeight: 0, overflow: 'auto' }}>
        {isDocument(path) ? <DocumentViewer path={path} /> : <FileContent path={path} full fresh />}
      </Box>
    </Stack>
  )
}
