import { useEffect, useMemo, useState } from 'react'
import { Stack, Typography, Divider, Switch, Button, IconButton, CircularProgress } from '../../../ui'
import { FlashLinear, TrashLinear, CodeLinear, ArrowDownLinear } from '../../../icons'
import { call } from '../../../ipc'
import { useSettingsUi } from '../store/uiStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { CodeBlock } from '../../ai/components/CodeBlock'
import { GuidesSubsection } from './GuidesSubsection'
import { ModsManager } from '../../mods/components/ModsManager'

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
  const [open, setOpen] = useState(false)

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
      <Stack
        direction="row"
        role="button"
        aria-expanded={open}
        tabIndex={0}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), setOpen((o) => !o))}
        sx={{ alignItems: 'center', gap: 0.75, cursor: 'pointer', borderRadius: 1.5, mx: -0.5, px: 0.5, py: 0.25, '&:hover': { bgcolor: 'action.hover' } }}
      >
        <ArrowDownLinear
          size={14}
          color="currentColor"
          style={{ flexShrink: 0, transition: 'transform 120ms', transform: open ? 'none' : 'rotate(-90deg)' }}
        />
        <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
          Built-in tools
        </Typography>
        {loaded && (
          <Typography variant="caption" color="text.secondary">
            {tools.length - disabled.filter((d) => tools.some((t) => t.name === d)).length} of {tools.length} on
          </Typography>
        )}
      </Stack>
      {open && (
        <Typography variant="caption" color="text.secondary" sx={{ mb: 1 }}>
          Disabled tools are hidden from the agent entirely — it won't see them in its toolset, and it can't call them.
        </Typography>
      )}
      {!open ? null : !loaded ? (
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

export function ModsSection() {
  return (
    <Stack sx={{ gap: 2.5 }}>
      <ModsManager />
      <Divider />
      <GuidesSubsection />
      <Divider />
      <ToolsSubsection />
      <Divider />
      <CustomToolsSubsection />
    </Stack>
  )
}
