import { useEffect, useState } from 'react'
import { Box, Stack, ButtonBase, Tooltip, IconButton, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Button, Menu, MenuItem, Typography, FormControlLabel, Switch, Divider } from '../../../ui'
import { AddLinear, SettingsLinear, FolderLinear, Link2Linear, MessageSquareLinear } from '../../../icons'
import { useSettingsUi } from '../../settings/store/uiStore'
import { useProjectsStore, projectColor, projectInitial, PROJECT_COLORS, GENERAL_PROJECT_ID, type Project } from '../store/projectsStore'
import { useMultiplayerStore } from '../../multiplayer/store/multiplayerStore'
import { call } from '../../../ipc'
import { isWindows } from '../../../lib/platform'
import { NewProjectDialog } from './NewProjectDialog'

export const PROJECT_RAIL_WIDTH = 56

/** Create/edit a project: name, folder, accent color. The folder is what
 * powers "open in Explorer/PowerShell" and the workspace the threads in this
 * project run against. */
function ProjectDialog({ project, open, onClose }: { project: Project | null; open: boolean; onClose: () => void }) {
  const create = useProjectsStore((s) => s.create)
  const update = useProjectsStore((s) => s.update)
  const createMultiplayer = useProjectsStore((s) => s.createMultiplayer)
  const myId = useMultiplayerStore((s) => s.myId)
  const initMultiplayer = useMultiplayerStore((s) => s.init)
  const [name, setName] = useState('')
  const [path, setPath] = useState('')
  const [color, setColor] = useState<string>(Object.keys(PROJECT_COLORS)[0])
  const [busy, setBusy] = useState(false)
  // Multiplayer projects are set up at creation only — editing an existing
  // project's pairing/git-remote isn't wired yet, so the toggle is hidden
  // once a project already exists (renaming/re-coloring still works via the
  // fields above).
  const [multiplayer, setMultiplayer] = useState(false)
  const [peerId, setPeerId] = useState('')
  // Pair with a second Vivianne on this computer instead of another device.
  const [testPartner, setTestPartner] = useState(false)
  const [partnerError, setPartnerError] = useState<string | null>(null)
  const [gitRemote, setGitRemote] = useState('')

  useEffect(() => {
    if (!open) return
    setName(project?.name ?? '')
    setPath(project?.path ?? '')
    setColor(project?.color ?? Object.keys(PROJECT_COLORS)[0])
    setMultiplayer(false)
    setTestPartner(false)
    setPartnerError(null)
    setPeerId('')
    setGitRemote('')
    if (!project) void initMultiplayer()
  }, [open, project, initMultiplayer])

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

  const isMultiplayerReady = multiplayer && !!path.trim() && !!peerId.trim()

  const submit = async () => {
    if (!name.trim()) return
    setBusy(true)
    try {
      if (project) {
        await update(project.id, name, path || null, color)
      } else if (multiplayer) {
        if (!isMultiplayerReady) return
        await createMultiplayer(name, path.trim(), peerId.trim(), testPartner ? 'Test partner' : null, gitRemote.trim() || null, color)
        if (testPartner) {
          // Gives the partner the mirror of this project and opens it.
          await call('cmd_multiplayer_launch_test_partner', { name: name.trim(), folder: path.trim() })
        }
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
                    borderRadius: 1.5,
                    bgcolor: hex,
                    border: '2px solid',
                    borderColor: color === id ? 'text.primary' : 'transparent',
                  }}
                />
              ))}
            </Stack>
          </Stack>

          {!project && (
            <>
              <FormControlLabel
                control={<Switch size="small" checked={multiplayer} onChange={(e) => setMultiplayer(e.target.checked)} />}
                label={
                  <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75 }}>
                    <Link2Linear size={14} color="currentColor" />
                    <Typography variant="body2">Multiplayer project</Typography>
                  </Stack>
                }
              />
              {multiplayer && (
                <Stack sx={{ gap: 1.5, p: 1.5, borderRadius: 1.5, bgcolor: 'action.hover' }}>
                  <Typography variant="caption" color="text.secondary">
                    A folder is required — it becomes a git repo both of you work against. You each paste the other’s
                    pairing code (yours is below; they see theirs in this same dialog).
                  </Typography>
                  <FormControlLabel
                    control={
                      <Switch
                        size="small"
                        checked={testPartner}
                        onChange={async (e) => {
                          const on = e.target.checked
                          setTestPartner(on)
                          setPartnerError(null)
                          if (!on) {
                            setPeerId('')
                            return
                          }
                          try {
                            const id = await call<string>('cmd_multiplayer_test_partner_id')
                            if (id) setPeerId(id)
                          } catch (err) {
                            setTestPartner(false)
                            setPartnerError(err instanceof Error ? err.message : String(err))
                          }
                        }}
                      />
                    }
                    label={
                      <Stack>
                        <Typography variant="body2">Test with a partner on this computer</Typography>
                        <Typography variant="caption" color="text.secondary">
                          No second device? A second Vivianne opens here with its own data, already paired to this project.
                        </Typography>
                      </Stack>
                    }
                    sx={{ alignItems: 'flex-start', '& .ui-Switch': { mt: 0.25 } }}
                  />
                  {partnerError && (
                    <Typography variant="caption" color="error.main">
                      {partnerError}
                    </Typography>
                  )}
                  <TextField
                    size="small"
                    label={testPartner ? "Test partner's pairing code" : "Your pair's pairing code"}
                    value={peerId}
                    onChange={(e) => setPeerId(e.target.value)}
                    disabled={testPartner}
                    fullWidth
                    slotProps={{ input: { sx: { fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.75rem' } } }}
                  />
                  <TextField
                    size="small"
                    label="Git remote (optional)"
                    placeholder="https://github.com/you/project.git"
                    value={gitRemote}
                    onChange={(e) => setGitRemote(e.target.value)}
                    fullWidth
                  />
                  {myId && (
                    <Typography variant="caption" color="text.secondary">
                      Your own pairing code (share it with them): <code>{myId}</code>
                    </Typography>
                  )}
                </Stack>
              )}
            </>
          )}
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
          disabled={busy || !name.trim() || (!project && multiplayer && !isMultiplayerReady)}
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
  const refresh = useProjectsStore((s) => s.refresh)
  const openSettings = useSettingsUi((s) => s.openSettings)
  const [dialog, setDialog] = useState<{ open: boolean; project: Project | null }>({ open: false, project: null })
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
        bgcolor: 'background.chrome',
      }}
    >
      <Tooltip title="General — questions not tied to a project" placement="right">
        <ButtonBase
          onClick={() => setActiveProject(GENERAL_PROJECT_ID)}
          sx={{
            width: 38,
            height: 38,
            flexShrink: 0,
            borderRadius: 2,
            color: activeProjectId === GENERAL_PROJECT_ID ? 'text.primary' : 'text.secondary',
            bgcolor: 'action.hover',
            border: '1.5px solid',
            borderColor: activeProjectId === GENERAL_PROJECT_ID ? 'text.primary' : 'transparent',
            transition: 'border-color 0.15s, transform 0.08s',
            '&:hover': { borderColor: 'text.secondary' },
            '&:active': { transform: 'scale(0.94)' },
          }}
        >
          <MessageSquareLinear size={17} color="currentColor" />
        </ButtonBase>
      </Tooltip>

      {projects.length > 0 && <Divider sx={{ width: 24 }} />}

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
          onClick={() => setOnboarding(true)}
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
      <NewProjectDialog
        open={onboarding}
        onClose={() => setOnboarding(false)}
        onMultiplayer={() => {
          setOnboarding(false)
          setDialog({ open: true, project: null })
        }}
      />
    </Stack>
  )
}
