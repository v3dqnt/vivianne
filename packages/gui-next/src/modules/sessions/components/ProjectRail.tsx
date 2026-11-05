import { useEffect, useState } from 'react'
import { Box, Stack, ButtonBase, Tooltip, IconButton, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Button, Menu, MenuItem, Typography, Divider } from '../../../ui'
import { AddLinear, SettingsLinear, FolderLinear, MessageSquareLinear } from '../../../icons'
import { useSettingsUi } from '../../settings/store/uiStore'
import { useProjectsStore, projectColor, projectInitial, PROJECT_COLORS, GENERAL_PROJECT_ID, type Project } from '../store/projectsStore'
import { isWindows } from '../../../lib/platform'
import { NewProjectDialog } from './NewProjectDialog'
import { PairProjectDialog } from '../../multiplayer/components/PairProjectDialog'

export const PROJECT_RAIL_WIDTH = 48
/** The rail's buttons. */
const TILE = 30

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

/** Far-left rail of project squares. Exactly one project is always active —
 * the sidebar next to it shows only that project's threads. */
export function ProjectRail() {
  const projects = useProjectsStore((s) => s.projects)
  const activeProjectId = useProjectsStore((s) => s.activeProjectId)
  const setActiveProject = useProjectsStore((s) => s.setActiveProject)
  const remove = useProjectsStore((s) => s.remove)
  const setIcon = useProjectsStore((s) => s.setIcon)
  const [iconFor, setIconFor] = useState<Project | null>(null)
  const refresh = useProjectsStore((s) => s.refresh)
  const openSettings = useSettingsUi((s) => s.openSettings)
  const [dialog, setDialog] = useState<{ open: boolean; project: Project | null }>({ open: false, project: null })
  const [pairing, setPairing] = useState(false)
  const [menu, setMenu] = useState<{ anchor: HTMLElement; project: Project } | null>(null)
  // New projects start with onboarding. The plain dialog remains for editing
  // a project and for multiplayer setup, which onboarding doesn't cover.
  const [onboarding, setOnboarding] = useState(false)

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
        // The sidebar card sits 8px to the right of the rail on the same
        // chrome background, so the strip you see is 56px wide. The same 8px
        // on the left centres the icons in it rather than in the rail alone.
        pl: '8px',
        boxSizing: 'border-box',
        bgcolor: 'background.chrome',
      }}
    >
      <Tooltip title="General — questions not tied to a project" placement="right">
        <ButtonBase
          onClick={() => setActiveProject(GENERAL_PROJECT_ID)}
          sx={{
            width: TILE,
            height: TILE,
            flexShrink: 0,
            borderRadius: '8px',
            color: activeProjectId === GENERAL_PROJECT_ID ? 'text.primary' : 'text.secondary',
            bgcolor: 'action.hover',
            border: '1.5px solid',
            borderColor: activeProjectId === GENERAL_PROJECT_ID ? 'text.primary' : 'transparent',
            transition: 'border-color 0.15s, transform 0.08s',
            '&:hover': { borderColor: 'text.secondary' },
            '&:active': { transform: 'scale(0.94)' },
          }}
        >
          <MessageSquareLinear size={15} color="currentColor" />
        </ButtonBase>
      </Tooltip>

      {projects.length > 0 && <Divider sx={{ width: 20 }} />}

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
                width: TILE,
                height: TILE,
                flexShrink: 0,
                borderRadius: '8px',
                overflow: 'hidden',
                fontSize: '0.8125rem',
                fontWeight: 500,
                color: projectColor(p),
                bgcolor: `${projectColor(p)}22`,
                border: '1.5px solid',
                borderColor: active ? projectColor(p) : 'transparent',
                transition: 'border-color 0.15s, transform 0.08s',
                '&:hover': { borderColor: projectColor(p) },
                '&:active': { transform: 'scale(0.94)' },
              }}
            >
              {p.icon ? <Box component="img" src={p.icon} alt="" draggable={false} sx={{ width: '100%', height: '100%', objectFit: 'cover', display: 'block' }} /> : projectInitial(p.name)}
            </ButtonBase>
          </Tooltip>
        )
      })}

      <Tooltip title="New project" placement="right">
        <IconButton
          size="small"
          onClick={() => setOnboarding(true)}
          sx={{ width: TILE, height: TILE, flexShrink: 0, borderRadius: '8px', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover' } }}
        >
          <AddLinear size={15} color="currentColor" />
        </IconButton>
      </Tooltip>

      <Box sx={{ flex: 1 }} />

      <Tooltip title="Settings" placement="right">
        <IconButton size="small" onClick={() => openSettings()} sx={{ width: 28, height: 28, color: 'text.secondary' }}>
          <SettingsLinear size={15} color="currentColor" />
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
            if (menu) setIconFor(menu.project)
            setMenu(null)
            document.getElementById('project-icon-input')?.click()
          }}
        >
          Set icon…
        </MenuItem>
        {menu?.project.icon && (
          <MenuItem
            onClick={() => {
              void setIcon(menu.project.id, null)
              setMenu(null)
            }}
          >
            Remove icon
          </MenuItem>
        )}
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

      <input
        id="project-icon-input"
        type="file"
        accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml"
        hidden
        onChange={async (e) => {
          const file = e.target.files?.[0]
          e.target.value = ''
          if (!file || !iconFor) return
          try {
            await setIcon(iconFor.id, await iconFrom(file))
          } catch (err) {
            console.warn('Could not set the project icon', err)
          }
          setIconFor(null)
        }}
      />

      <ProjectDialog open={dialog.open} project={dialog.project} onClose={() => setDialog({ open: false, project: null })} />
      <NewProjectDialog
        open={onboarding}
        onClose={() => setOnboarding(false)}
        onMultiplayer={() => {
          setOnboarding(false)
          setPairing(true)
        }}
      />
      <PairProjectDialog open={pairing} onClose={() => setPairing(false)} />
    </Stack>
  )
}
