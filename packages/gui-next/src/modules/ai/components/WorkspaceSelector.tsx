import { useEffect, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Button from '@mui/material/Button'
import Popover from '@mui/material/Popover'
import Typography from '@mui/material/Typography'
import InputBase from '@mui/material/InputBase'
import IconButton from '@mui/material/IconButton'
import ButtonBase from '@mui/material/ButtonBase'
import Divider from '@mui/material/Divider'
import CircularProgress from '@mui/material/CircularProgress'
import { FolderLinear, LockLinear, UnlockLinear, TrashLinear, AddLinear, FolderAddLinear } from '../../../icons'
import { useWorkspaceStore } from '../store/workspaceStore'

function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() || path
}

export function WorkspaceSelector() {
  const folders = useWorkspaceStore((s) => s.folders)
  const accessMode = useWorkspaceStore((s) => s.accessMode)
  const loading = useWorkspaceStore((s) => s.loading)
  const refresh = useWorkspaceStore((s) => s.refresh)
  const addExistingFolder = useWorkspaceStore((s) => s.addExistingFolder)
  const createFolder = useWorkspaceStore((s) => s.createFolder)
  const removeFolder = useWorkspaceStore((s) => s.removeFolder)
  const toggleAccessMode = useWorkspaceStore((s) => s.toggleAccessMode)

  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const [creating, setCreating] = useState(false)
  const [newFolderName, setNewFolderName] = useState('')

  useEffect(() => {
    void refresh()
  }, [refresh])

  const confirmCreate = async () => {
    await createFolder(newFolderName)
    setNewFolderName('')
    setCreating(false)
  }

  const label = folders.length === 0 ? 'No workspace' : folders.length === 1 ? folderName(folders[0]) : `${folders.length} folders`

  return (
    <>
      <Button
        size="small"
        onClick={(e) => setAnchor(e.currentTarget)}
        startIcon={<FolderLinear size={14} color="currentColor" />}
        endIcon={accessMode === 'readOnly' ? <LockLinear size={11} color="currentColor" /> : undefined}
        title={folders.join(', ') || 'Select a workspace folder'}
        sx={{
          textTransform: 'none',
          fontSize: '0.75rem',
          fontWeight: 500,
          color: 'text.secondary',
          maxWidth: 160,
          borderRadius: 999,
          px: 1,
          py: 0.5,
          minWidth: 0,
          '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
        }}
      >
        <Typography variant="caption" noWrap>
          {label}
        </Typography>
      </Button>

      <Popover
        open={!!anchor}
        anchorEl={anchor}
        onClose={() => {
          setAnchor(null)
          setCreating(false)
        }}
        anchorOrigin={{ vertical: 'top', horizontal: 'left' }}
        transformOrigin={{ vertical: 'bottom', horizontal: 'left' }}
        slotProps={{ paper: { sx: { width: 320, p: 1.5 } } }}
      >
        <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 600, display: 'block', px: 0.5, mb: 0.5 }}>
          Workspace folders
        </Typography>

        {folders.length === 0 ? (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', px: 0.5, py: 1 }}>
            No folders yet — add one below.
          </Typography>
        ) : (
          <Stack sx={{ mb: 1 }}>
            {folders.map((f, i) => (
              <Stack
                key={f}
                direction="row"
                sx={{ alignItems: 'center', gap: 1, px: 0.75, py: 0.75, borderRadius: 1.5, '&:hover .ws-remove': { opacity: 1 } }}
              >
                <Typography
                  variant="caption"
                  noWrap
                  title={f}
                  sx={{ flex: 1, minWidth: 0, fontFamily: '"JetBrains Mono", ui-monospace, monospace' }}
                >
                  {folderName(f)} {i === 0 && <Box component="span" sx={{ color: 'text.secondary' }}>(primary)</Box>}
                </Typography>
                <IconButton
                  size="small"
                  className="ws-remove"
                  onClick={() => void removeFolder(f)}
                  disabled={loading}
                  sx={{ opacity: 0, transition: 'opacity 0.1s', flexShrink: 0 }}
                >
                  <TrashLinear size={13} color="currentColor" />
                </IconButton>
              </Stack>
            ))}
          </Stack>
        )}

        <Divider sx={{ my: 0.5 }} />

        {creating ? (
          <Stack sx={{ gap: 1, p: 0.5 }}>
            <InputBase
              autoFocus
              fullWidth
              placeholder="New folder name"
              value={newFolderName}
              onChange={(e) => setNewFolderName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void confirmCreate()
                if (e.key === 'Escape') setCreating(false)
              }}
              sx={{ fontSize: '0.8125rem', bgcolor: 'rgba(255,255,255,0.05)', borderRadius: 1.5, px: 1, py: 0.5 }}
            />
            <Stack direction="row" sx={{ gap: 1 }}>
              <ButtonBase
                onClick={() => setCreating(false)}
                sx={{ flex: 1, py: 0.75, borderRadius: 1.5, fontSize: '0.75rem', border: 1, borderColor: 'divider' }}
              >
                Cancel
              </ButtonBase>
              <ButtonBase
                onClick={() => void confirmCreate()}
                disabled={!newFolderName.trim() || loading}
                sx={{
                  flex: 1,
                  py: 0.75,
                  borderRadius: 1.5,
                  fontSize: '0.75rem',
                  bgcolor: 'primary.main',
                  color: 'primary.contrastText',
                  '&.Mui-disabled': { bgcolor: 'action.disabledBackground', color: 'text.disabled' },
                }}
              >
                {loading ? <CircularProgress size={13} sx={{ color: 'inherit' }} /> : 'Choose location…'}
              </ButtonBase>
            </Stack>
          </Stack>
        ) : (
          <Stack sx={{ gap: 0.25 }}>
            <ButtonBase
              onClick={() => void addExistingFolder()}
              disabled={loading}
              sx={{ justifyContent: 'flex-start', gap: 1, px: 1, py: 0.75, borderRadius: 1.5, fontSize: '0.8125rem', '&:hover': { bgcolor: 'action.hover' } }}
            >
              <AddLinear size={14} color="currentColor" />
              Add existing folder…
            </ButtonBase>
            <ButtonBase
              onClick={() => setCreating(true)}
              disabled={loading}
              sx={{ justifyContent: 'flex-start', gap: 1, px: 1, py: 0.75, borderRadius: 1.5, fontSize: '0.8125rem', '&:hover': { bgcolor: 'action.hover' } }}
            >
              <FolderAddLinear size={14} color="currentColor" />
              New folder…
            </ButtonBase>
          </Stack>
        )}

        <Divider sx={{ my: 0.5 }} />

        <ButtonBase
          onClick={() => void toggleAccessMode()}
          disabled={loading}
          sx={{
            width: '100%',
            justifyContent: 'flex-start',
            gap: 1,
            px: 1,
            py: 0.75,
            borderRadius: 1.5,
            fontSize: '0.8125rem',
            color: accessMode === 'readOnly' ? 'text.secondary' : 'text.primary',
            '&:hover': { bgcolor: 'action.hover' },
          }}
        >
          {accessMode === 'full' ? <LockLinear size={14} color="currentColor" /> : <UnlockLinear size={14} color="currentColor" />}
          {accessMode === 'full' ? 'Switch to read-only' : 'Switch to full access'}
        </ButtonBase>
      </Popover>
    </>
  )
}
