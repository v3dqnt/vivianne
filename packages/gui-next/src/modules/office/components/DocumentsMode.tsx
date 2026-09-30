import { lazy, Suspense } from 'react'
import { Box, Stack, Typography, Button, ButtonBase, Tooltip, CircularProgress } from '../../../ui'
import { CloseLinear, DocumentTextLinear, AddLinear } from '../../../icons'
import { usePanelStore } from '../../dock/store/panelStore'
import { fileName, kindOf } from '../lib/docs'
import { FileViewer } from '../../dock/components/FilePreview'

const DocumentViewer = lazy(() => import('./DocumentViewer').then((m) => ({ default: m.DocumentViewer })))

async function pickDocument(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog')
  const picked = await open({ multiple: false })
  return typeof picked === 'string' ? picked : null
}

/**
 * The Preview panel's Documents mode: the documents open for viewing and
 * editing, one tab each, and the viewer for the selected one.
 */
export function DocumentsMode() {
  const docs = usePanelStore((s) => s.openDocs)
  const active = usePanelStore((s) => s.activeDoc)
  const openDocument = usePanelStore((s) => s.openDocument)
  const closeDocument = usePanelStore((s) => s.closeDocument)

  const openFromDisk = async () => {
    const path = await pickDocument()
    if (path) openDocument(path)
  }

  if (docs.length === 0 || !active) {
    return (
      <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 1, p: 3, textAlign: 'center' }}>
        <DocumentTextLinear size={24} color="currentColor" />
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Nothing open
        </Typography>
        <Typography variant="caption" color="text.secondary" sx={{ maxWidth: 300 }}>
          Open any file to preview it: code, images, video, audio, 3D models, PDFs, and Word, Excel or PowerPoint files you can edit in place. Or click a file name in the chat.
        </Typography>
        <Button size="small" variant="outlined" onClick={() => void openFromDisk()} sx={{ mt: 0.5, borderRadius: '8px' }}>
          Open a file
        </Button>
      </Stack>
    )
  }

  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, px: 0.75, py: 0.5, borderBottom: 1, borderColor: 'divider', overflowX: 'auto', flexShrink: 0 }}>
        {docs.map((path) => {
          const selected = path === active
          return (
            <Stack
              key={path}
              direction="row"
              sx={{
                alignItems: 'center',
                gap: 0.25,
                pl: 1,
                pr: 0.25,
                borderRadius: '6px',
                flexShrink: 0,
                bgcolor: selected ? 'action.selected' : 'transparent',
                '&:hover': { bgcolor: selected ? 'action.selected' : 'action.hover' },
              }}
            >
              <Tooltip title={path} enterDelay={600}>
                <ButtonBase
                  onClick={() => openDocument(path)}
                  sx={{ fontSize: '0.75rem', fontWeight: selected ? 600 : 400, color: selected ? 'text.primary' : 'text.secondary', py: 0.375, maxWidth: 180 }}
                >
                  <Box component="span" sx={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {fileName(path)}
                  </Box>
                </ButtonBase>
              </Tooltip>
              <ButtonBase aria-label={`Close ${fileName(path)}`} onClick={() => closeDocument(path)} sx={{ p: 0.375, borderRadius: '4px', color: 'text.disabled', '&:hover': { color: 'text.primary' } }}>
                <CloseLinear size={10} color="currentColor" />
              </ButtonBase>
            </Stack>
          )
        })}
        <Tooltip title="Open a file">
          <ButtonBase onClick={() => void openFromDisk()} sx={{ p: 0.5, borderRadius: '6px', color: 'text.secondary', flexShrink: 0, '&:hover': { bgcolor: 'action.hover' } }}>
            <AddLinear size={13} color="currentColor" />
          </ButtonBase>
        </Tooltip>
      </Stack>
      <Suspense
        fallback={
          <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}>
            <CircularProgress size={18} />
          </Stack>
        }
      >
        {/* Keyed by path: each document gets a fresh viewer (and its own undo
            history). Office files and PDFs get the editor; anything else the
            general viewer. */}
        {kindOf(active) ? <DocumentViewer key={active} path={active} /> : <FileViewer key={active} path={active} />}
      </Suspense>
    </Stack>
  )
}
