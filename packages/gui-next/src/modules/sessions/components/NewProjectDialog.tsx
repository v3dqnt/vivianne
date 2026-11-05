import { useEffect, useState, type ReactNode } from 'react'
import { Alert, Box, Button, Dialog, Stack, TextField, ToggleButton, ToggleButtonGroup, Typography } from '../../../ui'
import { CloseLinear, FolderLinear } from '../../../icons'
import { call } from '../../../ipc'
import { isWindows } from '../../../lib/platform'
import { useProjectsStore, type Project } from '../store/projectsStore'
import { Question } from '../onboarding/fields'

function lastSegment(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? ''
}

/**
 * Starting a project: a name and the folder its threads live in.
 *
 * Every project is a threads project — separate chats, each its own
 * conversation. (Older tree projects still open and chat normally; new ones
 * are no longer created here.)
 */
export function NewProjectDialog({ open, onClose, onMultiplayer, onCreated }: { open: boolean; onClose: () => void; onMultiplayer: () => void; onCreated?: (project: Project) => void }) {
  const create = useProjectsStore((s) => s.create)
  const [name, setName] = useState('')
  const [folderMode, setFolderMode] = useState<'new' | 'existing'>('new')
  const [parent, setParent] = useState('')
  const [existing, setExisting] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    setName('')
    setFolderMode('new')
    setParent('')
    setExisting('')
    setError(null)
  }, [open])

  const folderReady = folderMode === 'new' ? !!parent.trim() : !!existing.trim()
  const ready = !!name.trim() && folderReady && !busy

  const finish = async () => {
    setBusy(true)
    setError(null)
    try {
      const root =
        folderMode === 'new'
          ? ((await call<string>('cmd_create_workspace_folder', { parent: parent.trim(), name: name.trim() })) ?? '')
          : existing.trim()
      if (!root) throw new Error('No project folder.')
      // Adding a project doesn't open anything: the caller decides what it's for.
      const project = await create(name.trim(), root, null, 'threads', false)
      if (project) onCreated?.(project)
      onClose()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const card = (children: ReactNode) => (
    <Stack sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 3, py: 2.5, gap: 3 }}>{children}</Stack>
  )

  return (
    <Dialog
      open={open}
      onClose={busy ? undefined : onClose}
      maxWidth={false}
      slotProps={{ paper: { sx: { width: 'min(560px, 94vw)', maxHeight: '90vh', maxWidth: 'none', display: 'flex', flexDirection: 'column' } } }}
    >
      <Stack direction="row" sx={{ alignItems: 'flex-start', px: 3, pt: 2.5, pb: 1.5, gap: 2 }}>
        <Stack sx={{ flex: 1 }}>
          <Typography sx={{ fontWeight: 600, fontSize: '1.125rem', lineHeight: 1.3 }}>New project</Typography>
          <Typography variant="body2" color="text.secondary">
            Give it a name and a folder — its threads live there.
          </Typography>
        </Stack>
        <Box
          component="button"
          type="button"
          aria-label="Close"
          onClick={onClose}
          disabled={busy}
          sx={{ border: 0, bgcolor: 'transparent', color: 'text.secondary', cursor: 'pointer', p: 0.5, borderRadius: '6px', '&:hover': { bgcolor: 'action.hover' } }}
        >
          <CloseLinear size={16} color="currentColor" />
        </Box>
      </Stack>

      {card(
        <>
          <Question label="Project name">
            <TextField size="small" autoFocus fullWidth value={name} onChange={(e) => setName(e.target.value)} />
          </Question>
          <Question label="Where should it live?">
            <ToggleButtonGroup
              size="small"
              exclusive
              value={folderMode}
              onChange={(_, v: 'new' | 'existing' | null) => v && setFolderMode(v)}
              sx={{ '& .ui-ToggleButton': { textTransform: 'none', px: 1.5, py: 0.375 } }}
            >
              <ToggleButton value="new">Create a new folder</ToggleButton>
              <ToggleButton value="existing">Use an existing folder</ToggleButton>
            </ToggleButtonGroup>
            <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
              <TextField
                size="small"
                fullWidth
                placeholder={isWindows ? String.raw`C:\Users\you\Work` : '/Users/you/Work'}
                label={folderMode === 'new' ? 'Create it inside' : 'Folder'}
                value={folderMode === 'new' ? parent : existing}
                onChange={(e) => (folderMode === 'new' ? setParent(e.target.value) : setExisting(e.target.value))}
              />
              <Button
                size="small"
                startIcon={<FolderLinear size={13} color="currentColor" />}
                sx={{ flexShrink: 0 }}
                onClick={async () => {
                  const { open: openDialog } = await import('@tauri-apps/plugin-dialog')
                  const picked = await openDialog({ directory: true, multiple: false })
                  if (typeof picked !== 'string') return
                  if (folderMode === 'new') setParent(picked)
                  else {
                    setExisting(picked)
                    if (!name.trim()) setName(lastSegment(picked))
                  }
                }}
              >
                Browse
              </Button>
            </Stack>
          </Question>
          {error && <Alert severity="error">{error}</Alert>}
        </>,
      )}

      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 3, py: 1.5, borderTop: 1, borderColor: 'divider' }}>
        <Button color="inherit" size="small" onClick={onMultiplayer} disabled={busy} sx={{ textTransform: 'none', color: 'text.secondary' }}>
          Multiplayer project instead
        </Button>
        <Box sx={{ flex: 1 }} />
        <Button variant="contained" disableElevation disabled={!ready} onClick={() => void finish()}>
          {busy ? 'Creating…' : 'Create project'}
        </Button>
      </Stack>
    </Dialog>
  )
}
