import { useEffect, useState } from 'react'
import { Alert, Stack, Typography, Divider, Button, IconButton, Switch, InputBase, ToggleButton, ToggleButtonGroup, Checkbox, FormControlLabel } from '../../../ui'
import { ClockLinear, PlayLinear, TrashLinear, AddLinear, EditLinear, FolderOpenLinear, CloseLinear } from '../../../icons'
import { call } from '../../../ipc'

interface Step {
  prompt: string
  askFirst: boolean
}

/** Mirrors `automation::Automation` (camelCase on the wire; times in ms). */
export interface Automation {
  id: string
  name: string
  prompt: string
  cron: string
  enabled: boolean
  lastRun?: number | null
  nextRun?: number | null
  steps?: Step[]
  folder?: string | null
  glob?: string | null
}

const field = { fontSize: '0.8125rem', bgcolor: 'action.hover', borderRadius: 1, px: 1, py: 0.5 } as const

/** Workflows: a prompt, or a chain of them, that runs on a schedule or when a
 * file shows up in a folder. The backend owns the triggers; each run opens a
 * thread of its own, and a step can stop and ask first. */
export function AutomationsSection() {
  const [items, setItems] = useState<Automation[]>([])
  const [editing, setEditing] = useState<Automation | null>(null)
  const [creating, setCreating] = useState(false)
  const [name, setName] = useState('')
  const [trigger, setTrigger] = useState<'schedule' | 'folder'>('schedule')
  const [cron, setCron] = useState('0 9 * * *')
  const [folder, setFolder] = useState('')
  const [glob, setGlob] = useState('')
  const [steps, setSteps] = useState<Step[]>([{ prompt: '', askFirst: false }])
  const [error, setError] = useState<string | null>(null)

  const refresh = async () => setItems((await call<Automation[]>('cmd_automation_list')) ?? [])
  useEffect(() => {
    void refresh()
  }, [])

  const resetForm = () => {
    setEditing(null)
    setCreating(false)
    setName('')
    setTrigger('schedule')
    setCron('0 9 * * *')
    setFolder('')
    setGlob('')
    setSteps([{ prompt: '', askFirst: false }])
    setError(null)
  }

  const startEdit = (a: Automation) => {
    setEditing(a)
    setCreating(true)
    setName(a.name)
    setTrigger(a.folder ? 'folder' : 'schedule')
    setCron(a.cron || '0 9 * * *')
    setFolder(a.folder ?? '')
    setGlob(a.glob ?? '')
    setSteps([{ prompt: a.prompt, askFirst: false }, ...(a.steps ?? [])])
    setError(null)
  }

  const valid = name.trim() && steps[0]?.prompt.trim() && (trigger === 'schedule' ? cron.trim() : folder.trim())

  const save = async () => {
    if (!valid) return
    setError(null)
    const [first, ...rest] = steps
    const args = {
      name: name.trim(),
      prompt: first.prompt.trim(),
      cron: trigger === 'schedule' ? cron.trim() : '',
      steps: rest.filter((s) => s.prompt.trim()),
      folder: trigger === 'folder' ? folder.trim() : '',
      glob: trigger === 'folder' ? glob.trim() : '',
    }
    try {
      if (editing) await call('cmd_automation_update', { id: editing.id, ...args })
      else await call('cmd_automation_create', args)
      resetForm()
      await refresh()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const pickFolder = async () => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const dir = await open({ directory: true, multiple: false })
    if (typeof dir === 'string') setFolder(dir)
  }

  const setStep = (i: number, patch: Partial<Step>) => setSteps((s) => s.map((x, j) => (j === i ? { ...x, ...patch } : x)))

  const when = (a: Automation) =>
    a.folder
      ? `When a file ${a.glob ? `matching ${a.glob} ` : ''}appears in ${a.folder}`
      : `${a.cron}${a.nextRun ? ` · next ${new Date(a.nextRun).toLocaleString()}` : ''}`

  return (
    <Stack sx={{ gap: 1 }}>
      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between' }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
          Workflows
        </Typography>
        <Button size="small" startIcon={<AddLinear size={14} color="currentColor" />} onClick={() => setCreating((v) => !v)} sx={{ textTransform: 'none' }}>
          New workflow
        </Button>
      </Stack>
      <Typography variant="caption" color="text.secondary">
        A prompt, or a chain of them, that runs on a schedule or when a file shows up in a folder. Each run gets its own thread, and any step
        can stop and ask you first. Use <code>{'{{file}}'}</code> in a step for the file that triggered it. Runs while Vivianne is open.
      </Typography>
      <Divider />

      {creating && (
        <Stack sx={{ gap: 1.25, p: 1.5, borderRadius: 2, border: 1, borderColor: 'divider' }}>
          <InputBase fullWidth placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} sx={field} />

          <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
            <ToggleButtonGroup size="small" exclusive value={trigger} onChange={(_: unknown, v: 'schedule' | 'folder' | null) => v && setTrigger(v)}>
              <ToggleButton value="schedule" sx={{ textTransform: 'none', px: 1.25 }}>
                On a schedule
              </ToggleButton>
              <ToggleButton value="folder" sx={{ textTransform: 'none', px: 1.25 }}>
                When a file appears
              </ToggleButton>
            </ToggleButtonGroup>
          </Stack>

          {trigger === 'schedule' ? (
            <InputBase
              fullWidth
              placeholder="Cron expression (e.g. 0 9 * * 1 is Mondays at 9)"
              value={cron}
              onChange={(e) => setCron(e.target.value)}
              sx={{ ...field, fontSize: '0.75rem', fontFamily: '"Geist Mono", ui-monospace, monospace' }}
            />
          ) : (
            <Stack direction="row" sx={{ gap: 1 }}>
              <InputBase fullWidth placeholder="Folder to watch" value={folder} onChange={(e) => setFolder(e.target.value)} sx={{ ...field, flex: 1 }} />
              <IconButton size="small" title="Choose folder" onClick={() => void pickFolder()}>
                <FolderOpenLinear size={15} color="currentColor" />
              </IconButton>
              <InputBase placeholder="Only *.pdf" value={glob} onChange={(e) => setGlob(e.target.value)} sx={{ ...field, width: 120 }} />
            </Stack>
          )}

          {steps.map((s, i) => (
            <Stack key={i} sx={{ gap: 0.5 }}>
              <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
                <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 600 }}>
                  Step {i + 1}
                </Typography>
                {i > 0 && (
                  <FormControlLabel
                    control={<Checkbox size="small" checked={s.askFirst} onChange={(e) => setStep(i, { askFirst: e.target.checked })} />}
                    label={<Typography variant="caption">Ask me first</Typography>}
                    sx={{ ml: 0.5 }}
                  />
                )}
                <span style={{ flex: 1 }} />
                {i > 0 && (
                  <IconButton size="small" title="Remove step" onClick={() => setSteps((x) => x.filter((_, j) => j !== i))}>
                    <CloseLinear size={13} color="currentColor" />
                  </IconButton>
                )}
              </Stack>
              <InputBase
                fullWidth
                multiline
                minRows={2}
                placeholder={i === 0 ? 'What should Vivianne do?' : 'Then…'}
                value={s.prompt}
                onChange={(e) => setStep(i, { prompt: e.target.value })}
                sx={field}
              />
            </Stack>
          ))}
          <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
            <Button size="small" onClick={() => setSteps((s) => [...s, { prompt: '', askFirst: true }])} sx={{ textTransform: 'none' }}>
              Add a step
            </Button>
            <span style={{ flex: 1 }} />
            <Button size="small" variant="contained" disabled={!valid} onClick={() => void save()} sx={{ textTransform: 'none' }}>
              {editing ? 'Save' : 'Create'}
            </Button>
            <Button size="small" onClick={resetForm} sx={{ textTransform: 'none' }}>
              Cancel
            </Button>
          </Stack>
          {error && <Alert severity="error">{error}</Alert>}
        </Stack>
      )}

      {items.length === 0 && !creating ? (
        <Typography variant="caption" color="text.secondary" sx={{ py: 1 }}>
          No workflows yet.
        </Typography>
      ) : (
        items.map((a) => (
          <Stack key={a.id} direction="row" sx={{ alignItems: 'center', gap: 1.5, py: 1, px: 1, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}>
            <ClockLinear size={15} color="currentColor" />
            <Stack sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="body2" sx={{ fontWeight: 500 }} noWrap>
                {a.name}
                {(a.steps?.length ?? 0) > 0 ? ` · ${(a.steps?.length ?? 0) + 1} steps` : ''}
              </Typography>
              <Typography variant="caption" color="text.secondary" noWrap title={when(a)}>
                {when(a)}
              </Typography>
            </Stack>
            <IconButton size="small" title="Run now" onClick={() => void call('cmd_automation_run_now', { id: a.id })}>
              <PlayLinear size={14} color="currentColor" />
            </IconButton>
            <IconButton size="small" title="Edit" onClick={() => startEdit(a)}>
              <EditLinear size={14} color="currentColor" />
            </IconButton>
            <Switch
              size="small"
              checked={a.enabled}
              onChange={(e) => void call('cmd_automation_set_enabled', { id: a.id, enabled: e.target.checked }).then(refresh)}
            />
            <IconButton size="small" title="Delete" onClick={() => void call('cmd_automation_delete', { id: a.id }).then(refresh)}>
              <TrashLinear size={14} color="currentColor" />
            </IconButton>
          </Stack>
        ))
      )}
    </Stack>
  )
}
