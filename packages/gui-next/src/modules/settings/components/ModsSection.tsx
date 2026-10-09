import { useEffect, useMemo, useState } from 'react'
import { Stack, Typography, Divider, Switch, Button, IconButton, CircularProgress, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Checkbox, MenuItem, ButtonGroup } from '../../../ui'
import { FlashLinear, ImportLinear, TrashLinear, BookLinear, LayerLinear, AddLinear, EditLinear, CodeLinear } from '../../../icons'
import { call } from '../../../ipc'
import { usePlaybookStore } from '../store/playbookStore'
import { useSettingsUi } from '../store/uiStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { CodeBlock } from '../../ai/components/CodeBlock'
import { isPanel, type PlaybookDef } from '../lib/playbooks'
import { SuperchargeSubsection } from './SuperchargeSubsection'
import { GuidesSubsection } from './GuidesSubsection'

// ── Playbook parameter editor: a small list-builder over the JSON-schema
// `{ type: 'object', properties: {...}, required: [...] }` shape every
// playbook's `parameters` field already is — see `coding_agent::playbooks`.
// A raw-JSON textarea would be more powerful but is exactly the "hand-write
// JSON externally" friction the in-app editor exists to remove. ────────────

interface ParamRow {
  key: string
  name: string
  type: 'string' | 'number' | 'boolean'
  description: string
  required: boolean
}

let paramKeySeq = 0
function newParamRow(): ParamRow {
  return { key: `p${paramKeySeq++}`, name: '', type: 'string', description: '', required: false }
}

function parametersToRows(parameters: Record<string, unknown>): ParamRow[] {
  const properties = (parameters.properties as Record<string, { type?: string; description?: string }>) ?? {}
  const required = new Set((parameters.required as string[] | undefined) ?? [])
  return Object.entries(properties).map(([name, schema]) => ({
    key: `p${paramKeySeq++}`,
    name,
    type: schema.type === 'number' || schema.type === 'boolean' ? schema.type : 'string',
    description: schema.description ?? '',
    required: required.has(name),
  }))
}

function rowsToParameters(rows: ParamRow[]): Record<string, unknown> {
  const properties: Record<string, unknown> = {}
  const required: string[] = []
  for (const row of rows) {
    const name = row.name.trim()
    if (!name) continue
    properties[name] = { type: row.type, description: row.description.trim() || undefined }
    if (row.required) required.push(name)
  }
  return { type: 'object', properties, ...(required.length ? { required } : {}) }
}

function PlaybookDialog({ playbook, open, onClose }: { playbook: PlaybookDef | null; open: boolean; onClose: () => void }) {
  const save = usePlaybookStore((s) => s.save)
  const [mode, setMode] = useState<'tool' | 'panel'>('tool')
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [command, setCommand] = useState('')
  const [panelUrl, setPanelUrl] = useState('')
  const [rows, setRows] = useState<ParamRow[]>([])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    setMode(playbook && isPanel(playbook) ? 'panel' : 'tool')
    setName(playbook?.name ?? '')
    setDescription(playbook?.description ?? '')
    setCommand(playbook?.command ?? '')
    setPanelUrl(playbook?.panel?.url ?? '')
    setRows(playbook ? parametersToRows(playbook.parameters) : [])
    setError(null)
  }, [open, playbook])

  const canSubmit = name.trim() && (mode === 'tool' ? command.trim() : panelUrl.trim())

  const submit = async () => {
    if (!canSubmit) return
    setBusy(true)
    setError(null)
    try {
      await save(
        {
          name: name.trim(),
          description: description.trim(),
          parameters: mode === 'tool' ? rowsToParameters(rows) : {},
          command: mode === 'tool' ? command : null,
          panel: mode === 'panel' ? { url: panelUrl.trim() } : null,
        },
        playbook?.name,
      )
      onClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle sx={{ fontSize: '1rem', fontWeight: 700 }}>{playbook ? 'Edit playbook' : 'New playbook'}</DialogTitle>
      <DialogContent>
        <Stack sx={{ gap: 2, pt: 0.5 }}>
          <ButtonGroup size="small" sx={{ alignSelf: 'flex-start' }}>
            <Button
              variant={mode === 'tool' ? 'contained' : 'outlined'}
              disableElevation
              onClick={() => setMode('tool')}
              startIcon={<BookLinear size={14} color="currentColor" />}
              sx={{ textTransform: 'none' }}
            >
              Tool
            </Button>
            <Button
              variant={mode === 'panel' ? 'contained' : 'outlined'}
              disableElevation
              onClick={() => setMode('panel')}
              startIcon={<LayerLinear size={14} color="currentColor" />}
              sx={{ textTransform: 'none' }}
            >
              Panel
            </Button>
          </ButtonGroup>

          <TextField size="small" label="Name" value={name} onChange={(e) => setName(e.target.value)} autoFocus fullWidth />
          <TextField size="small" label="Description" value={description} onChange={(e) => setDescription(e.target.value)} fullWidth />

          {mode === 'tool' ? (
            <>
              <Stack sx={{ gap: 0.75 }}>
                <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between' }}>
                  <Typography variant="caption" color="text.secondary">
                    Parameters
                  </Typography>
                  <Button size="small" onClick={() => setRows((r) => [...r, newParamRow()])} sx={{ textTransform: 'none', minWidth: 0, p: 0.5 }}>
                    <AddLinear size={14} color="currentColor" />
                  </Button>
                </Stack>
                {rows.map((row, i) => (
                  <Stack key={row.key} direction="row" sx={{ gap: 0.75, alignItems: 'center' }}>
                    <TextField
                      size="small"
                      placeholder="name"
                      value={row.name}
                      onChange={(e) => setRows((r) => r.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))}
                      sx={{ flex: 1, minWidth: 0 }}
                    />
                    <TextField
                      size="small"
                      select
                      value={row.type}
                      onChange={(e) => setRows((r) => r.map((x, j) => (j === i ? { ...x, type: e.target.value as ParamRow['type'] } : x)))}
                      sx={{ width: 100, flexShrink: 0 }}
                    >
                      <MenuItem value="string">string</MenuItem>
                      <MenuItem value="number">number</MenuItem>
                      <MenuItem value="boolean">boolean</MenuItem>
                    </TextField>
                    <TextField
                      size="small"
                      placeholder="description"
                      value={row.description}
                      onChange={(e) => setRows((r) => r.map((x, j) => (j === i ? { ...x, description: e.target.value } : x)))}
                      sx={{ flex: 2, minWidth: 0 }}
                    />
                    <Checkbox
                      size="small"
                      title="Required"
                      checked={row.required}
                      onChange={(e) => setRows((r) => r.map((x, j) => (j === i ? { ...x, required: e.target.checked } : x)))}
                      sx={{ p: 0.5, flexShrink: 0 }}
                    />
                    <IconButton size="small" onClick={() => setRows((r) => r.filter((_, j) => j !== i))} sx={{ flexShrink: 0 }}>
                      <TrashLinear size={13} color="currentColor" />
                    </IconButton>
                  </Stack>
                ))}
              </Stack>

              <TextField
                size="small"
                label="Command"
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                multiline
                minRows={2}
                maxRows={8}
                fullWidth
                placeholder={'echo "Hello {{name}}"'}
                helperText={<>Shell command run via bash. Use <code>{'{{param}}'}</code> to substitute each parameter above.</>}
                slotProps={{ input: { sx: { fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.8125rem' } } }}
              />
            </>
          ) : (
            <TextField
              size="small"
              label="Panel URL"
              value={panelUrl}
              onChange={(e) => setPanelUrl(e.target.value)}
              fullWidth
              placeholder="http://localhost:3000"
              helperText="Must start with http://localhost: or http://127.0.0.1: — the app's CSP won't frame anything else. Point it at a local dev server and it opens as a dock panel."
              slotProps={{ input: { sx: { fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.8125rem' } } }}
            />
          )}

          {error && (
            <Typography variant="caption" color="error.main">
              {error}
            </Typography>
          )}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button color="inherit" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="contained" disableElevation onClick={() => void submit()} disabled={busy || !canSubmit}>
          {playbook ? 'Save' : 'Create'}
        </Button>
      </DialogActions>
    </Dialog>
  )
}

interface ToolInfo {
  name: string
  description?: string
  /** Capability group from the backend's own `capabilities::GROUPS` — the
   * same map the model's system prompt is built from, so this list can't
   * drift from what the agent is actually told it can do. */
  group?: string
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
    await call('cmd_save_settings', { settings: { ...(settings ?? {}), disabledTools: next } })
  }

  // Preserve the backend's own group ordering (capability map order, which is
  // deliberately flow-ordered rather than alphabetical) instead of re-sorting.
  const grouped = useMemo(() => {
    const order: string[] = []
    const byGroup = new Map<string, ToolInfo[]>()
    for (const tool of tools) {
      const key = tool.group || 'Other'
      if (!byGroup.has(key)) {
        byGroup.set(key, [])
        order.push(key)
      }
      byGroup.get(key)?.push(tool)
    }
    return order.map((label) => ({ label, items: byGroup.get(label) ?? [] }))
  }, [tools])

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
        grouped.map((group) => (
          <Stack key={group.label} sx={{ gap: 0.25, mb: 1.5 }}>
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{
                fontWeight: 600,
                textTransform: 'uppercase',
                letterSpacing: '0.08em',
                fontSize: '0.6875rem',
                px: 1,
                pt: 1,
                pb: 0.5,
              }}
            >
              {group.label}
            </Typography>
            {group.items.map((tool) => (
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
            ))}
          </Stack>
        ))
      )}
    </Stack>
  )
}

/** Dedicated toggle for `run_script` (Programmatic Tool Calling). It lives
 * in its own setting rather than the per-tool switches above; on unless the
 * user has switched it off, matching the backend's default. */
function ExperimentalSubsection() {
  const [enabled, setEnabled] = useState(true)
  const [settings, setSettings] = useState<Record<string, unknown> | null>(null)
  const [loaded, setLoaded] = useState(false)

  useEffect(() => {
    void call<Record<string, unknown> & { enableProgrammaticToolCalling?: boolean }>('cmd_get_settings').then((s) => {
      if (s) {
        setSettings(s)
        setEnabled(s.enableProgrammaticToolCalling !== false)
      }
      setLoaded(true)
    })
  }, [])

  const toggle = async (on: boolean) => {
    setEnabled(on)
    await call('cmd_save_settings', { settings: { ...(settings ?? {}), enableProgrammaticToolCalling: on } })
  }

  return (
    <Stack sx={{ gap: 0.5 }}>
      <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
        Scripting
      </Typography>
      {loaded && (
        <Stack
          direction="row"
          sx={{ alignItems: 'center', gap: 1.5, py: 0.75, px: 1, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}
        >
          <FlashLinear size={15} color="currentColor" style={{ flexShrink: 0 }} />
          <Stack sx={{ flex: 1, minWidth: 0 }}>
            <Typography variant="body2" sx={{ fontWeight: 500 }} noWrap>
              Programmatic tool calling
            </Typography>
            <Typography variant="caption" color="text.secondary">
              Lets the agent write a short script (run_script) that calls any of its tools — built-in, MCP or
              playbook — in one turn instead of one round-trip each. Every call still goes through the usual approvals.
            </Typography>
          </Stack>
          <Switch size="small" checked={enabled} onChange={(e) => void toggle(e.target.checked)} />
        </Stack>
      )}
    </Stack>
  )
}

function PlaybookRow({ playbook, onEdit }: { playbook: PlaybookDef; onEdit: () => void }) {
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

  const detail = playbook.command ?? playbook.panel?.url ?? ''

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, py: 1, px: 1, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}>
      {isPanel(playbook) ? (
        <LayerLinear size={15} color="currentColor" style={{ flexShrink: 0 }} />
      ) : (
        <BookLinear size={15} color="currentColor" style={{ flexShrink: 0 }} />
      )}
      <Stack sx={{ flex: 1, minWidth: 0, cursor: 'pointer' }} onClick={onEdit}>
        <Typography variant="body2" sx={{ fontWeight: 500 }} noWrap>
          {playbook.name}
        </Typography>
        <Typography variant="caption" color="text.secondary" noWrap title={detail}>
          {playbook.description || detail}
        </Typography>
      </Stack>
      <IconButton size="small" onClick={onEdit}>
        <EditLinear size={14} color="currentColor" />
      </IconButton>
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
  const [dialog, setDialog] = useState<{ open: boolean; playbook: PlaybookDef | null }>({ open: false, playbook: null })

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
        <Stack direction="row" sx={{ gap: 0.5 }}>
          <Button
            size="small"
            onClick={() => setDialog({ open: true, playbook: null })}
            startIcon={<AddLinear size={14} color="currentColor" />}
            sx={{ textTransform: 'none' }}
          >
            New
          </Button>
          <Button
            size="small"
            onClick={() => void addPlaybook()}
            disabled={adding}
            startIcon={adding ? <CircularProgress size={14} /> : <ImportLinear size={14} color="currentColor" />}
            sx={{ textTransform: 'none' }}
          >
            Import file
          </Button>
        </Stack>
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
        playbooks.map((p) => <PlaybookRow key={p.name} playbook={p} onEdit={() => setDialog({ open: true, playbook: p })} />)
      )}
      <PlaybookDialog open={dialog.open} playbook={dialog.playbook} onClose={() => setDialog({ open: false, playbook: null })} />
    </Stack>
  )
}

interface CustomTool {
  name: string
  description: string
  createdAt: string
  updatedAt: string
  uses: number
  lastUsed: string | null
}

/** What the shredder asks: a review of the whole set, not a purge. */
const SHREDDER_PROMPT = `Review the custom tools you and other models have written. Call list_custom_tools, and for each one decide whether it still earns its place. Retire with delete_tool the ones that are:
- unused for over a month, or used once and never again,
- duplicates or near-duplicates of another tool (keep the better one, and improve it with create_tool replace: true if the other had something it lacks),
- superseded by a built-in tool.
Keep anything used regularly. Then tell me in a short list what you retired and why, and what you kept.`

function shortDate(iso: string | null): string {
  if (!iso) return 'never'
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? 'never' : d.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
}

function CustomToolRow({ tool, onRetire }: { tool: CustomTool; onRetire: () => Promise<void> }) {
  const [source, setSource] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const toggleSource = async () => {
    if (source !== null) return setSource(null)
    setSource((await call<string>('cmd_custom_tool_source', { name: tool.name }).catch(() => null)) ?? '')
  }
  return (
    <Stack sx={{ py: 1, px: 1, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
        <CodeLinear size={15} color="currentColor" style={{ flexShrink: 0 }} />
        <Stack sx={{ flex: 1, minWidth: 0, cursor: 'pointer' }} onClick={() => void toggleSource()}>
          <Typography variant="body2" sx={{ fontWeight: 500, fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.8125rem' }} noWrap>
            {tool.name}
          </Typography>
          <Typography variant="caption" color="text.secondary" noWrap title={tool.description}>
            {tool.description}
          </Typography>
        </Stack>
        <Typography variant="caption" color="text.disabled" sx={{ flexShrink: 0, textAlign: 'right' }}>
          {tool.uses} use{tool.uses === 1 ? '' : 's'} · last {shortDate(tool.lastUsed)}
        </Typography>
        <IconButton
          size="small"
          aria-label={`Retire ${tool.name}`}
          title="Retire (kept in ~/.viv/tools/.retired)"
          disabled={busy}
          onClick={() => {
            setBusy(true)
            void onRetire().finally(() => setBusy(false))
          }}
        >
          {busy ? <CircularProgress size={14} /> : <TrashLinear size={15} color="currentColor" />}
        </IconButton>
      </Stack>
      {source !== null && (
        <Stack sx={{ mt: 1, maxHeight: 280, overflow: 'auto' }}>
          <CodeBlock code={source || '(empty)'} lang="python" dense />
        </Stack>
      )}
    </Stack>
  )
}

/** Tools the agent wrote for itself with `create_tool` — available to every
 * model — and the shredder that reviews them when they pile up. */
function CustomToolsSubsection() {
  const [tools, setTools] = useState<CustomTool[] | null>(null)
  const sendMessage = useAgentStore((s) => s.sendMessage)
  const closeSettings = useSettingsUi((s) => s.closeSettings)

  const refresh = async () => setTools((await call<CustomTool[]>('cmd_list_custom_tools').catch(() => null)) ?? [])
  useEffect(() => {
    void refresh()
  }, [])

  const shred = () => {
    closeSettings()
    void sendMessage(SHREDDER_PROMPT)
  }

  return (
    <Stack sx={{ gap: 1 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
        <Stack sx={{ flex: 1 }}>
          <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
            Tools Vivianne made
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Python tools written with create_tool when a task needed one. Every model you use gets them, in every project. Calls ask for approval like any code that runs.
          </Typography>
        </Stack>
        <Button size="small" variant="outlined" disabled={!tools?.length} onClick={shred} sx={{ flexShrink: 0 }}>
          Run shredder
        </Button>
      </Stack>
      {tools === null ? (
        <CircularProgress size={16} />
      ) : tools.length === 0 ? (
        <Typography variant="caption" color="text.disabled" sx={{ px: 1 }}>
          None yet. Ask for something no tool does and Vivianne can write one, or say "make a tool that…".
        </Typography>
      ) : (
        <Stack>
          {tools.length > 30 && (
            <Typography variant="caption" color="warning.main" sx={{ px: 1, pb: 0.5 }}>
              {tools.length} tools: past 30 they cost context on every request. Run the shredder to retire stale ones.
            </Typography>
          )}
          {tools.map((t) => (
            <CustomToolRow
              key={t.name}
              tool={t}
              onRetire={async () => {
                await call('cmd_delete_custom_tool', { name: t.name }).catch(() => null)
                await refresh()
              }}
            />
          ))}
        </Stack>
      )}
    </Stack>
  )
}

/** Playbooks are switched off in Settings for now: the agent's own tool
 * creator covers the same ground. Flip to bring the section back. */
const PLAYBOOKS_ENABLED = false

export function ModsSection() {
  return (
    <Stack sx={{ gap: 2.5 }}>
      <SuperchargeSubsection />
      <Divider />
      <GuidesSubsection />
      <Divider />
      <ToolsSubsection />
      <Divider />
      <CustomToolsSubsection />
      <Divider />
      <ExperimentalSubsection />
      {PLAYBOOKS_ENABLED && (
        <>
          <Divider />
          <PlaybooksSubsection />
        </>
      )}
    </Stack>
  )
}
