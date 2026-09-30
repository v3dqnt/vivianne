import { useEffect, useState, type ReactNode } from 'react'
import { Alert, Box, Button, ButtonBase, Dialog, Stack, TextField, ToggleButton, ToggleButtonGroup, Typography, alpha, useTheme } from '../../../ui'
import { CloseLinear, FolderLinear, MessageSquareLinear } from '../../../icons'
import { call } from '../../../ipc'
import { isWindows } from '../../../lib/platform'
import { AgentMark } from '../../../ui/brand/AgentMark'
import { SUBAGENT_COLOR_HEX } from '../../settings/lib/subagentProfiles'
import { useTeamStore, blankAgent } from '../../team/store/teamStore'
import { openPanel } from '../../workspace/layout'
import { useProjectsStore, type ProjectMode } from '../store/projectsStore'
import { Choices, Question } from '../onboarding/fields'
import { KINDS, domainOf } from '../onboarding/model'
import { starterTeamsFor } from '../onboarding/starterTeams'
import { OnboardingDialog } from './OnboardingDialog'

const MODES: Array<{ id: ProjectMode; title: string; body: string; best: string }> = [
  {
    id: 'threads',
    title: 'Threads',
    body: 'Separate chats, each its own conversation — the classic way.',
    best: 'Best for separate questions and quick jobs.',
  },
  {
    id: 'company',
    title: 'Company',
    body: 'A master agent runs the project from a charter, hires specialists and hands them work.',
    best: 'Best for one long piece of work you want managed.',
  },
  {
    id: 'team',
    title: 'Agent team',
    body: 'You build the agents — each with its own prompt, model and tools. Give the team a task and they work on it together, messaging each other.',
    best: 'Best for tasks that benefit from several expert views at once.',
  },
]

/** The picture on each mode's card: one mark, a master and its hires, or
 * a row of peers. */
function ModeArt({ mode }: { mode: ProjectMode }) {
  if (mode === 'threads') {
    return (
      <Stack sx={{ gap: 0.5, alignItems: 'flex-start' }}>
        {[70, 90, 55].map((w, i) => (
          <Box key={i} sx={{ display: 'flex', alignItems: 'center', gap: 0.75, color: 'text.secondary' }}>
            <MessageSquareLinear size={12} color="currentColor" />
            <Box sx={{ width: w, height: 5, borderRadius: 3, bgcolor: 'action.selected' }} />
          </Box>
        ))}
      </Stack>
    )
  }
  if (mode === 'company') {
    return (
      <Stack sx={{ alignItems: 'center', gap: 0.5 }}>
        <AgentMark color={SUBAGENT_COLOR_HEX.blue} size={26} />
        <Stack direction="row" sx={{ gap: 0.75 }}>
          {(['green', 'amber', 'purple'] as const).map((c) => (
            <AgentMark key={c} color={SUBAGENT_COLOR_HEX[c]} size={18} />
          ))}
        </Stack>
      </Stack>
    )
  }
  return (
    <Stack direction="row" sx={{ gap: 0.75, alignItems: 'center' }}>
      {(['purple', 'teal', 'red'] as const).map((c) => (
        <AgentMark key={c} color={SUBAGENT_COLOR_HEX[c]} size={24} />
      ))}
    </Stack>
  )
}

function lastSegment(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? ''
}

/**
 * Starting a project: first, how its work is organised.
 *
 * Company goes on to the onboarding interview (the master needs a charter).
 * Threads and Agent team need only a name and a folder — plus, for a team,
 * an optional starter team matched to the kind of project, which is a
 * faster start than building every agent from nothing.
 */
export function NewProjectDialog({ open, onClose, onMultiplayer }: { open: boolean; onClose: () => void; onMultiplayer: () => void }) {
  const theme = useTheme()
  const create = useProjectsStore((s) => s.create)
  const saveAgent = useTeamStore((s) => s.save)
  const [mode, setMode] = useState<ProjectMode | null>(null)
  const [interview, setInterview] = useState(false)
  const [name, setName] = useState('')
  const [folderMode, setFolderMode] = useState<'new' | 'existing'>('new')
  const [parent, setParent] = useState('')
  const [existing, setExisting] = useState('')
  const [kind, setKind] = useState<string | null>(null)
  const [starter, setStarter] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    setMode(null)
    setInterview(false)
    setName('')
    setFolderMode('new')
    setParent('')
    setExisting('')
    setKind(null)
    setStarter(null)
    setError(null)
  }, [open])

  const teams = starterTeamsFor(kind)
  // The best-fitting starter is picked by default for each kind.
  useEffect(() => setStarter(starterTeamsFor(kind)[0]?.id ?? null), [kind])

  const folderReady = folderMode === 'new' ? !!parent.trim() : !!existing.trim()
  const ready = !!name.trim() && folderReady && !busy

  const finish = async () => {
    if (!mode) return
    setBusy(true)
    setError(null)
    try {
      const root =
        folderMode === 'new'
          ? ((await call<string>('cmd_create_workspace_folder', { parent: parent.trim(), name: name.trim() })) ?? '')
          : existing.trim()
      if (!root) throw new Error('No project folder.')
      await create(name.trim(), root, null, mode)
      if (mode === 'team') {
        const team = teams.find((t) => t.id === starter)
        for (const a of team?.agents ?? []) {
          await saveAgent(root, { ...blankAgent(), name: a.name, role: a.role, color: a.color, instructions: a.instructions, proposedBy: 'user' }, null)
        }
        openPanel('agents')
      }
      onClose()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  if (interview) {
    return (
      <OnboardingDialog
        open={open}
        project={null}
        onClose={onClose}
        onMultiplayer={onMultiplayer}
      />
    )
  }

  const card = (children: ReactNode) => (
    <Stack sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 3, py: 2.5, gap: 3 }}>{children}</Stack>
  )

  return (
    <Dialog
      open={open}
      onClose={busy ? undefined : onClose}
      maxWidth={false}
      slotProps={{ paper: { sx: { width: 'min(820px, 94vw)', maxHeight: '90vh', maxWidth: 'none', display: 'flex', flexDirection: 'column' } } }}
    >
      <Stack direction="row" sx={{ alignItems: 'flex-start', px: 3, pt: 2.5, pb: 1.5, gap: 2 }}>
        <Stack sx={{ flex: 1 }}>
          <Typography sx={{ fontWeight: 700, fontSize: '1.125rem', lineHeight: 1.3 }}>New project</Typography>
          <Typography variant="body2" color="text.secondary">
            {mode ? MODES.find((m) => m.id === mode)?.title : 'How should the work in it be organised? You can’t change this later.'}
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

      {!mode
        ? card(
            <Stack direction={{ xs: 'column', sm: 'row' }} sx={{ gap: 1.5 }}>
              {MODES.map((m) => (
                <ButtonBase
                  key={m.id}
                  onClick={() => (m.id === 'company' ? setInterview(true) : setMode(m.id))}
                  sx={{
                    flex: 1,
                    flexDirection: 'column',
                    alignItems: 'flex-start',
                    textAlign: 'left',
                    gap: 1.25,
                    p: 2,
                    borderRadius: '14px',
                    border: 1,
                    borderColor: 'divider',
                    transition: 'border-color 0.12s, background-color 0.12s',
                    '&:hover': { borderColor: 'text.secondary', bgcolor: alpha(theme.palette.text.primary, 0.025) },
                  }}
                >
                  <Box sx={{ height: 52, display: 'flex', alignItems: 'center' }}>
                    <ModeArt mode={m.id} />
                  </Box>
                  <Typography sx={{ fontWeight: 600, fontSize: '0.9375rem' }}>{m.title}</Typography>
                  <Typography variant="body2" color="text.secondary" sx={{ lineHeight: 1.5 }}>
                    {m.body}
                  </Typography>
                  <Typography variant="caption" color="text.disabled" sx={{ mt: 'auto' }}>
                    {m.best}
                  </Typography>
                </ButtonBase>
              ))}
            </Stack>,
          )
        : card(
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
              {mode === 'team' && (
                <>
                  <Question label="What kind of work is it?" hint="Picks a starter team to match. Optional.">
                    <Choices options={KINDS} value={kind} onChange={setKind} allowOther={false} />
                  </Question>
                  <Question
                    label="Start with a team"
                    hint={
                      domainOf(kind) === 'research'
                        ? 'Experts who check each other’s work. Edit or add to them any time in the Agents panel.'
                        : 'Edit them, or add your own, any time in the Agents panel.'
                    }
                  >
                    <Stack sx={{ gap: 1 }}>
                      {[...teams, { id: 'none', label: 'Start empty', blurb: 'I’ll build the team myself', agents: [] }].map((t) => {
                        const on = starter === t.id
                        return (
                          <ButtonBase
                            key={t.id}
                            onClick={() => setStarter(t.id)}
                            aria-pressed={on}
                            sx={{
                              justifyContent: 'flex-start',
                              gap: 1.5,
                              px: 1.5,
                              py: 1,
                              borderRadius: '10px',
                              border: 1,
                              borderColor: on ? 'primary.main' : 'divider',
                              bgcolor: on ? 'action.selected' : 'transparent',
                              textAlign: 'left',
                            }}
                          >
                            <Stack direction="row" sx={{ gap: 0.5, minWidth: 84 }}>
                              {t.agents.map((a) => (
                                <AgentMark key={a.name} color={SUBAGENT_COLOR_HEX[a.color]} size={22} title={a.name} />
                              ))}
                            </Stack>
                            <Stack sx={{ minWidth: 0 }}>
                              <Typography variant="body2" sx={{ fontWeight: 600 }}>
                                {t.label}
                              </Typography>
                              <Typography variant="caption" color="text.secondary">
                                {t.blurb}
                              </Typography>
                            </Stack>
                          </ButtonBase>
                        )
                      })}
                    </Stack>
                  </Question>
                </>
              )}
              {error && <Alert severity="error">{error}</Alert>}
            </>,
          )}

      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 3, py: 1.5, borderTop: 1, borderColor: 'divider' }}>
        {mode ? (
          <Button color="inherit" onClick={() => setMode(null)} disabled={busy}>
            Back
          </Button>
        ) : (
          <Button color="inherit" size="small" onClick={onMultiplayer} sx={{ textTransform: 'none', color: 'text.secondary' }}>
            Multiplayer project instead
          </Button>
        )}
        <Box sx={{ flex: 1 }} />
        {mode && (
          <Button variant="contained" disableElevation disabled={!ready} onClick={() => void finish()}>
            {busy ? 'Creating…' : 'Create project'}
          </Button>
        )}
      </Stack>
    </Dialog>
  )
}
