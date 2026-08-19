import { useEffect, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import ButtonBase from '@mui/material/ButtonBase'
import Tooltip from '@mui/material/Tooltip'
import IconButton from '@mui/material/IconButton'
import Dialog from '@mui/material/Dialog'
import DialogTitle from '@mui/material/DialogTitle'
import DialogContent from '@mui/material/DialogContent'
import DialogActions from '@mui/material/DialogActions'
import TextField from '@mui/material/TextField'
import Button from '@mui/material/Button'
import Menu from '@mui/material/Menu'
import MenuItem from '@mui/material/MenuItem'
import Typography from '@mui/material/Typography'
import { AddLinear, SettingsLinear, FolderLinear } from '../../../icons'
import { useSettingsUi } from '../../settings/store/uiStore'
import { useProjectsStore, projectColor, projectInitial, PROJECT_COLORS, type Project } from '../store/projectsStore'

export const PROJECT_RAIL_WIDTH = 56

/** Create/edit a project: name, folder, accent color. The folder is what
 * powers "open in Explorer/PowerShell" and the workspace the threads in this
 * project run against. */
function ProjectDialog({ project, open, onClose }: { project: Project | null; open: boolean; onClose: () => void }) {
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
      if (project) await update(project.id, name, path || null, color)
      else await create(name, path || null, color)
      onClose()
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle sx={{ fontSize: '1rem', fontWeight: 700 }}>{project ? 'Edit project' : 'New project'}</DialogTitle>
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
              placeholder="C:\\Users\\you\\Work\\project"
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
                    borderRadius: 1.5,
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
        <Button variant="contained" disableElevation onClick={() => void submit()} disabled={busy || !name.trim()}>
          {project ? 'Save' : 'Create'}
        </Button>
      </DialogActions>
    </Dialog>
  )
}

/** Far-left rail of project squares. Exactly one project is always active —
 * the sidebar next to it shows only that project's threads. */
export function ProjectRail() {
  const projects = useProjectsStore((s) => s.projects)
  const activeProjectId = useProjectsStore((s) => s.activeProjectId)
  const setActiveProject = useProjectsStore((s) => s.setActiveProject)
  const remove = useProjectsStore((s) => s.remove)
  const refresh = useProjectsStore((s) => s.refresh)
  const openSettings = useSettingsUi((s) => s.openSettings)
  const [dialog, setDialog] = useState<{ open: boolean; project: Project | null }>({ open: false, project: null })
  const [menu, setMenu] = useState<{ anchor: HTMLElement; project: Project } | null>(null)

  useEffect(() => {
    void refresh()
  }, [refresh])

  return (
    <Stack
      sx={{
        width: PROJECT_RAIL_WIDTH,
        flexShrink: 0,
        height: '100%',
        alignItems: 'center',
        gap: 1,
        py: 1.5,
        borderRight: 1,
        borderColor: 'divider',
        bgcolor: 'background.default',
      }}
    >
      {projects.map((p) => {
        const active = p.id === activeProjectId
        return (
          <Tooltip key={p.id} title={p.path ? `${p.name} — ${p.path}` : p.name} placement="right">
            <ButtonBase
              onClick={() => setActiveProject(p.id)}
              onContextMenu={(e) => {
                e.preventDefault()
                setMenu({ anchor: e.currentTarget as HTMLElement, project: p })
              }}
              sx={{
                width: 38,
                height: 38,
                flexShrink: 0,
                borderRadius: 2,
                fontSize: '0.9rem',
                fontWeight: 700,
                color: projectColor(p),
                bgcolor: `${projectColor(p)}22`,
                border: '1.5px solid',
                borderColor: active ? projectColor(p) : 'transparent',
                transition: 'border-color 0.15s, transform 0.08s',
                '&:hover': { borderColor: projectColor(p) },
                '&:active': { transform: 'scale(0.94)' },
              }}
            >
              {projectInitial(p.name)}
            </ButtonBase>
          </Tooltip>
        )
      })}

      <Tooltip title="New project" placement="right">
        <IconButton
          size="small"
          onClick={() => setDialog({ open: true, project: null })}
          sx={{ width: 38, height: 38, flexShrink: 0, borderRadius: 2, color: 'text.secondary', '&:hover': { bgcolor: 'action.hover' } }}
        >
          <AddLinear size={17} color="currentColor" />
        </IconButton>
      </Tooltip>

      <Box sx={{ flex: 1 }} />

      <Tooltip title="Settings" placement="right">
        <IconButton size="small" onClick={() => openSettings()} sx={{ width: 32, height: 32, color: 'text.secondary' }}>
          <SettingsLinear size={16} color="currentColor" />
        </IconButton>
      </Tooltip>

      <Menu anchorEl={menu?.anchor ?? null} open={!!menu} onClose={() => setMenu(null)}>
        <MenuItem
          onClick={() => {
            if (menu) setDialog({ open: true, project: menu.project })
            setMenu(null)
          }}
        >
          Edit project…
        </MenuItem>
        <MenuItem
          onClick={() => {
            if (menu) void remove(menu.project.id)
            setMenu(null)
          }}
          sx={{ color: 'error.main' }}
        >
          Delete project
        </MenuItem>
      </Menu>

      <ProjectDialog open={dialog.open} project={dialog.project} onClose={() => setDialog({ open: false, project: null })} />
    </Stack>
  )
}
