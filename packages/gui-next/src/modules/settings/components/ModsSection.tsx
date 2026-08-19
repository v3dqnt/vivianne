import { useEffect, useState } from 'react'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import Divider from '@mui/material/Divider'
import Switch from '@mui/material/Switch'
import Button from '@mui/material/Button'
import IconButton from '@mui/material/IconButton'
import CircularProgress from '@mui/material/CircularProgress'
import { FlashLinear, ImportLinear, TrashLinear, BookLinear } from '../../../icons'
import { call } from '../../../ipc'
import { usePlaybookStore } from '../store/playbookStore'
import type { PlaybookDef } from '../lib/playbooks'

interface ToolInfo {
  name: string
  description?: string
}

/** Built-in tools — the real list read from the backend's own schema
 * (`cmd_list_tool_names`), not a hand-maintained array that drifts every
 * time a tool is added or renamed on the Rust side. */
function ToolsSubsection() {
  const [tools, setTools] = useState<ToolInfo[]>([])
  const [disabled, setDisabled] = useState<string[]>([])
  const [settings, setSettings] = useState<Record<string, unknown> | null>(null)
  const [loaded, setLoaded] = useState(false)

  useEffect(() => {
    void Promise.all([
      call<ToolInfo[]>('cmd_list_tool_names'),
      call<Record<string, unknown> & { disabledTools?: string[] }>('cmd_get_settings'),
    ]).then(([toolList, s]) => {
      setTools(toolList ?? [])
      if (s) {
        setSettings(s)
        setDisabled(s.disabledTools ?? [])
      }
      setLoaded(true)
    })
  }, [])

  const toggle = async (id: string, on: boolean) => {
    const next = on ? disabled.filter((d) => d !== id) : [...disabled, id]
    setDisabled(next)
    await call('cmd_save_settings', { ...(settings ?? {}), disabledTools: next })
  }

  return (
    <Stack sx={{ gap: 0.5 }}>
      <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
        Built-in tools
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ mb: 1 }}>
        Disabled tools are hidden from the agent entirely — it won't see them in its toolset, and it can't call them.
      </Typography>
      {!loaded ? (
        <Typography variant="caption" color="text.secondary" sx={{ py: 1 }}>
          Loading…
        </Typography>
      ) : (
        tools.map((tool) => (
          <Stack
            key={tool.name}
            direction="row"
            sx={{ alignItems: 'center', gap: 1.5, py: 0.75, px: 1, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}
          >
            <FlashLinear size={15} color="currentColor" style={{ flexShrink: 0 }} />
            <Stack sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="body2" sx={{ fontWeight: 500 }} noWrap>
                {tool.name}
              </Typography>
              {tool.description && (
                <Typography variant="caption" color="text.secondary" noWrap title={tool.description}>
                  {tool.description}
                </Typography>
              )}
            </Stack>
            <Switch size="small" checked={!disabled.includes(tool.name)} onChange={(e) => void toggle(tool.name, e.target.checked)} />
          </Stack>
        ))
      )}
    </Stack>
  )
}

function PlaybookRow({ playbook }: { playbook: PlaybookDef }) {
  const remove = usePlaybookStore((s) => s.remove)
  const [busy, setBusy] = useState(false)

  const doRemove = async () => {
    setBusy(true)
    try {
      await remove(playbook.name)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, py: 1, px: 1, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}>
      <BookLinear size={15} color="currentColor" style={{ flexShrink: 0 }} />
      <Stack sx={{ flex: 1, minWidth: 0 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }} noWrap>
          {playbook.name}
        </Typography>
        <Typography variant="caption" color="text.secondary" noWrap title={playbook.command}>
          {playbook.description || playbook.command}
        </Typography>
      </Stack>
      <IconButton size="small" onClick={() => void doRemove()} disabled={busy}>
        {busy ? <CircularProgress size={14} /> : <TrashLinear size={15} color="currentColor" />}
      </IconButton>
    </Stack>
  )
}

/** Playbooks — custom, user-added tools (a JSON file describing a shell
 * command template), same "extends what the agent can do" concern as the
 * built-in toggles above, so they share one tab instead of two. */
function PlaybooksSubsection() {
  const playbooks = usePlaybookStore((s) => s.playbooks)
  const refresh = usePlaybookStore((s) => s.refresh)
  const pickPlaybookFile = usePlaybookStore((s) => s.pickPlaybookFile)
  const addFromFile = usePlaybookStore((s) => s.addFromFile)
  const [adding, setAdding] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    void refresh()
  }, [refresh])

  const addPlaybook = async () => {
    const path = await pickPlaybookFile()
    if (!path) return
    setAdding(true)
    setError(null)
    try {
      await addFromFile(path)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setAdding(false)
    }
  }

  return (
    <Stack sx={{ gap: 0.5 }}>
      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between' }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
          Playbooks
        </Typography>
        <Button
          size="small"
          onClick={() => void addPlaybook()}
          disabled={adding}
          startIcon={adding ? <CircularProgress size={14} /> : <ImportLinear size={14} color="currentColor" />}
          sx={{ textTransform: 'none' }}
        >
          Add playbook
        </Button>
      </Stack>
      <Typography variant="caption" color="text.secondary" sx={{ mb: 1 }}>
        A playbook is a JSON file describing one custom tool — name, description, parameters, and a shell command
        template (<code>{'{{param}}'}</code> substituted from the arguments). Adding one makes it available to the
        agent immediately, no restart.
      </Typography>
      {error && (
        <Typography variant="caption" color="error.main" sx={{ mb: 1 }}>
          {error}
        </Typography>
      )}
      {playbooks.length === 0 ? (
        <Typography variant="caption" color="text.secondary" sx={{ py: 1 }}>
          No playbooks added yet.
        </Typography>
      ) : (
        playbooks.map((p) => <PlaybookRow key={p.name} playbook={p} />)
      )}
    </Stack>
  )
}

export function ModsSection() {
  return (
    <Stack sx={{ gap: 2.5 }}>
      <ToolsSubsection />
      <Divider />
      <PlaybooksSubsection />
    </Stack>
  )
}
