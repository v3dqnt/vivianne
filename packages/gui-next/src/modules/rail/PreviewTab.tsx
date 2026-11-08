import { Box, ButtonBase, Stack, Typography } from '../../ui'
import { ArrowLeftLinear } from '../../icons'
import { FileViewer } from '../dock/components/FilePreview'
import { baseName } from '../ai/components/FileLink'
import { useRail } from './railStore'

/** One file, read in the rail, with the way back to the tree. */
export function PreviewTab() {
  const path = useRail((s) => s.previewPath)
  const setTab = useRail((s) => s.setTab)
  return (
    <Stack sx={{ minHeight: 0, flex: 1, gap: '8px' }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: '6px', flexShrink: 0 }}>
        <ButtonBase onClick={() => setTab('files')} aria-label="Back to files" sx={{ width: 26, height: 28, borderRadius: '6px', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}>
          <ArrowLeftLinear size={14} color="currentColor" />
        </ButtonBase>
        <Typography noWrap title={path ?? ''} sx={{ fontSize: '0.8125rem', fontWeight: 500, minWidth: 0 }}>
          {path ? baseName(path) : 'Nothing open'}
        </Typography>
      </Stack>
      <Box sx={{ flex: 1, minHeight: 0, overflow: 'auto', borderRadius: '8px' }}>
        {path ? <FileViewer path={path} /> : <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', px: '8px' }}>Pick a file in Files to read it here.</Typography>}
      </Box>
    </Stack>
  )
}
