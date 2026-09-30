import { useEffect, useState } from 'react'
import { Stack, Typography, Divider, Button, IconButton, Switch, InputBase } from '../../../ui'
import { ClockLinear, PlayLinear, TrashLinear, AddLinear, EditLinear } from '../../../icons'
import { call } from '../../../ipc'

export interface Automation {
  id: string
  name: string
  prompt: string
  cron: string
  enabled: boolean
  last_run?: number | null
  next_run?: number | null
  created_at?: number
}

/** Automations — scheduled prompts that fire on a cron expression. The
 * backend owns the scheduler; this is pure CRUD + run-now against it. */
export function AutomationsSection() {
  const [items, setItems] = useState<Automation[]>([])
  const [editing, setEditing] = useState<Automation | null>(null)
  const [creating, setCreating] = useState(false)
  const [name, setName] = useState('')
  const [prompt, setPrompt] = useState('')
  const [cron, setCron] = useState('0 * * * *')

  const refresh = async () => {
    const list = await call<Automation[]>('cmd_automation_list')
    setItems(list ?? [])
  }

  useEffect(() => {
    void refresh()
  }, [])

  const resetForm = () => {
    setEditing(null)
    setCreating(false)
    setName('')
    setPrompt('')
    setCron('0 * * * *')
  }

  const save = async () => {
    if (!name.trim() || !prompt.trim()) return
    if (editing) {
      await call('cmd_automation_update', { id: editing.id, name: name.trim(), prompt: prompt.trim(), cron: cron.trim() })
    } else {
      await call('cmd_automation_create', { name: name.trim(), prompt: prompt.trim(), cron: cron.trim() })
    }
    resetForm()
    await refresh()
  }

  const setEnabled = async (a: Automation, enabled: boolean) => {
    await call('cmd_automation_set_enabled', { id: a.id, enabled })
    await refresh()
  }

  const remove = async (a: Automation) => {
    await call('cmd_automation_delete', { id: a.id })
    await refresh()
  }

  const runNow = async (a: Automation) => {
    await call('cmd_automation_run_now', { id: a.id })
  }

  const startEdit = (a: Automation) => {
    setEditing(a)
    setCreating(true)
    setName(a.name)
    setPrompt(a.prompt)
    setCron(a.cron)
  }

  return (
    <Stack sx={{ gap: 1 }}>
      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between' }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
          Automations
        </Typography>
        <Button size="small" startIcon={<AddLinear size={14} color="currentColor" />} onClick={() => setCreating((v) => !v)} sx={{ textTransform: 'none' }}>
          New automation
        </Button>
      </Stack>
      <Typography variant="caption" color="text.secondary">
        Scheduled prompts that run on a cron schedule — e.g. a daily summary or weekly report.
      </Typography>
      <Divider />

      {creating && (
        <Stack sx={{ gap: 1, p: 1.5, borderRadius: 2, border: 1, borderColor: 'divider' }}>
          <InputBase
            fullWidth
            placeholder="Name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            sx={{ fontSize: '0.8125rem', bgcolor: 'action.hover', borderRadius: 1, px: 1, py: 0.5 }}
          />
          <InputBase
            fullWidth
            multiline
            minRows={2}
            placeholder="Prompt to run…"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            sx={{ fontSize: '0.8125rem', bgcolor: 'action.hover', borderRadius: 1, px: 1, py: 0.5 }}
          />
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
            <InputBase
              fullWidth
              placeholder="Cron expression (e.g. 0 9 * * 1)"
              value={cron}
              onChange={(e) => setCron(e.target.value)}
              sx={{ fontSize: '0.75rem', fontFamily: '"Geist Mono", ui-monospace, monospace', bgcolor: 'action.hover', borderRadius: 1, px: 1, py: 0.5 }}
            />
            <Button size="small" onClick={() => void save()} disabled={!name.trim() || !prompt.trim()} sx={{ textTransform: 'none', flexShrink: 0 }}>
              {editing ? 'Save' : 'Create'}
            </Button>
            <Button size="small" onClick={resetForm} sx={{ textTransform: 'none', flexShrink: 0 }}>
              Cancel
            </Button>
          </Stack>
        </Stack>
      )}

      {items.length === 0 && !creating ? (
        <Typography variant="caption" color="text.secondary" sx={{ py: 1 }}>
          No automations yet.
        </Typography>
      ) : (
        items.map((a) => (
          <Stack key={a.id} direction="row" sx={{ alignItems: 'center', gap: 1.5, py: 1, px: 1, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}>
            <ClockLinear size={15} color="currentColor" />
            <Stack sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="body2" sx={{ fontWeight: 500 }} noWrap>
                {a.name}
              </Typography>
              <Typography variant="caption" color="text.secondary" noWrap>
                {a.cron}
                {a.next_run ? ` · next ${new Date(a.next_run * 1000).toLocaleString()}` : ''}
              </Typography>
            </Stack>
            <IconButton size="small" title="Run now" onClick={() => void runNow(a)}>
              <PlayLinear size={14} color="currentColor" />
            </IconButton>
            <IconButton size="small" title="Edit" onClick={() => startEdit(a)}>
              <EditLinear size={14} color="currentColor" />
            </IconButton>
            <Switch size="small" checked={a.enabled} onChange={(e) => void setEnabled(a, e.target.checked)} />
            <IconButton size="small" title="Delete" onClick={() => void remove(a)}>
              <TrashLinear size={14} color="currentColor" />
            </IconButton>
          </Stack>
        ))
      )}
    </Stack>
  )
}
