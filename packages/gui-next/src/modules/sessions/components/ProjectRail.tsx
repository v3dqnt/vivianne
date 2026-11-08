import { useEffect, useState } from 'react'
import { Stack, ButtonBase, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Button, Typography } from '../../../ui'
import { FolderLinear } from '../../../icons'
import { useProjectsStore, PROJECT_COLORS, type Project } from '../store/projectsStore'
import { isWindows } from '../../../lib/platform'

export const PROJECT_RAIL_WIDTH = 48

/** A picked image, cropped square from its centre and scaled to 64px, as a
 * PNG data URL — small enough to keep in projects.json. */
export async function iconFrom(file: File): Promise<string> {
  const bitmap = await createImageBitmap(file)
  const side = Math.min(bitmap.width, bitmap.height)
  const canvas = document.createElement('canvas')
  canvas.width = canvas.height = 64
  const ctx = canvas.getContext('2d')
  if (!ctx) throw new Error('No canvas')
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(bitmap, (bitmap.width - side) / 2, (bitmap.height - side) / 2, side, side, 0, 0, 64, 64)
  bitmap.close()
  return canvas.toDataURL('image/png')
}

/** Create/edit a project: name, folder, accent color. The folder is what
 * powers "open in Explorer/PowerShell" and the workspace the threads in this
 * project run against. */
export function ProjectDialog({ project, open, onClose }: { project: Project | null; open: boolean; onClose: () => void }) {
  const create = useProjectsStore((s) => s.create)
  const update = useProjectsStore((s) => s.update)
  const [name, setName] = useState('')
  const [path, setPath] = useState('')
  const [color, setColor] = useState<string>(Object.keys(PROJECT_COLORS)[0])
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (!open) return
    setName(project?.name ?? '')
    setPath(project?.path ?? '')
    setColor(project?.color ?? Object.keys(PROJECT_COLORS)[0])
  }, [open, project])

  const pickFolder = async () => {
    // Same picker the workspace selector uses — the dialog plugin, not a
    // Rust command.
    const { open: openDialog } = await import('@tauri-apps/plugin-dialog')
    const picked = await openDialog({ directory: true, multiple: false })
    if (typeof picked === 'string') {
      setPath(picked)
      if (!name.trim()) setName(picked.split(/[\\/]/).filter(Boolean).pop() ?? '')
    }
  }


  const submit = async () => {
    if (!name.trim()) return
    setBusy(true)
    try {
      if (project) {
        await update(project.id, name, path || null, color)
      } else {
        await create(name, path || null, color)
      }
      onClose()
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle sx={{ fontSize: '1rem', fontWeight: 600 }}>{project ? 'Edit project' : 'New project'}</DialogTitle>
      <DialogContent>
        <Stack sx={{ gap: 2, pt: 0.5 }}>
          <TextField size="small" label="Name" value={name} onChange={(e) => setName(e.target.value)} autoFocus fullWidth />
          <Stack direction="row" sx={{ gap: 1, alignItems: 'flex-end' }}>
            <TextField
              size="small"
              label="Folder"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              fullWidth
              placeholder={isWindows ? String.raw`C:\Users\you\Work\project` : '/Users/you/Work/project'}
            />
            <Button size="small" onClick={() => void pickFolder()} startIcon={<FolderLinear size={13} color="currentColor" />} sx={{ flexShrink: 0 }}>
              Browse
            </Button>
          </Stack>
          <Stack sx={{ gap: 0.75 }}>
            <Typography variant="caption" color="text.secondary">
              Color
            </Typography>
            <Stack direction="row" sx={{ gap: 0.75, flexWrap: 'wrap' }}>
              {Object.entries(PROJECT_COLORS).map(([id, hex]) => (
                <ButtonBase
                  key={id}
                  onClick={() => setColor(id)}
                  sx={{
                    width: 24,
                    height: 24,
                    borderRadius: 1.6,
                    bgcolor: hex,
                    border: '2px solid',
                    borderColor: color === id ? 'text.primary' : 'transparent',
                  }}
                />
              ))}
            </Stack>
          </Stack>

        </Stack>
      </DialogContent>
      <DialogActions>
        <Button color="inherit" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="contained"
          disableElevation
          onClick={() => void submit()}
          disabled={busy || !name.trim()}
        >
          {project ? 'Save' : 'Create'}
        </Button>
      </DialogActions>
    </Dialog>
  )
}


