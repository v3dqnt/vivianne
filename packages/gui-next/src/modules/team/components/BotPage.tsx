import { useEffect, useMemo, useState } from 'react'
import { Box, Button, ButtonBase, IconButton, InputBase, Orb, Stack, Switch, TextField, Tooltip, Typography, alpha, useTheme } from '../../../ui'
import { ArrowLeftLinear, EditLinear, TrashLinear } from '../../../icons'
import { call } from '../../../ipc'
import { useTeamStore, type TeamAgent } from '../store/teamStore'
import { useBotsStore, botKey, type BotSettings, type Level, type Routine } from '../store/botsStore'
import { LiveAvatar, SectionLabel, colorOf } from './bits'
import { IDLE_LINES, STATE_LABEL, useBotFace } from './AgentFace'
import { ModelPicker } from '../../ai/components/ModelPicker'
import { ContactTab } from './ContactTab'

type Tab = 'routines' | 'contact' | 'rules' | 'memory'

const RULES: { key: keyof BotSettings['rules']; label: string; hint: string }[] = [
  { key: 'files', label: 'Change files', hint: 'write, edit, patch' },
  { key: 'commands', label: 'Run commands', hint: 'terminal, scripts, its own tools' },
  { key: 'computer', label: 'Use your computer', hint: 'clicking and typing in apps' },
  { key: 'web', label: 'Use the web', hint: 'search, read pages' },
  { key: 'outside', label: 'Act outside this machine', hint: 'email, texts and calls, GitHub, connected apps' },
]

const LEVELS: { value: Level; label: string }[] = [
  { value: 'auto', label: 'Just do it' },
  { value: 'if_requested', label: 'If I asked' },
  { value: 'ask', label: 'Ask me' },
  { value: 'hand_off', label: 'Hand to me' },
]

/** Schedules people actually want, as cron. */
const PRESETS: { label: string; cron: string }[] = [
  { label: 'Every weekday at 9:00', cron: '0 9 * * 1-5' },
  { label: 'Every day at 9:00', cron: '0 9 * * *' },
  { label: 'Every Monday at 9:00', cron: '0 9 * * 1' },
  { label: 'Every hour', cron: '0 * * * *' },
  { label: 'Every 15 minutes', cron: '*/15 * * * *' },
]

function when(ms: number | null): string {
  if (!ms) return '—'
  return new Date(ms).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
}

function Segmented({ value, onChange }: { value: Level; onChange: (v: Level) => void }) {
  return (
    <Stack direction="row" sx={{ p: 0.25, borderRadius: '8px', bgcolor: 'action.hover', gap: 0.25, flexShrink: 0 }}>
      {LEVELS.map((l) => (
        <ButtonBase
          key={l.value}
          onClick={() => onChange(l.value)}
          sx={{
            px: 1,
            py: 0.375,
            borderRadius: '6px',
            fontSize: '0.6875rem',
            fontWeight: 600,
            color: value === l.value ? 'text.primary' : 'text.secondary',
            bgcolor: value === l.value ? 'background.paper' : 'transparent',
            boxShadow: value === l.value ? 1 : 0,
          }}
        >
          {l.label}
        </ButtonBase>
      ))}
    </Stack>
  )
}

function RoutineEditor({ initial, onSave, onCancel }: { initial: Partial<Routine>; onSave: (r: Partial<Routine>) => Promise<void>; onCancel: () => void }) {
  const [r, setR] = useState<Partial<Routine>>({ title: '', prompt: '', cron: PRESETS[0].cron, enabled: true, ...initial })
  const [err, setErr] = useState<string | null>(null)
  const preset = PRESETS.find((p) => p.cron === r.cron)
  return (
    <Stack sx={{ gap: 1, p: 1.25, borderRadius: '10px', border: 1, borderColor: 'divider' }}>
      <TextField size="small" label="Title" value={r.title} onChange={(e) => setR({ ...r, title: e.target.value })} placeholder="Morning standup" />
      <TextField
        size="small"
        label="What to do"
        multiline
        minRows={3}
        value={r.prompt}
        onChange={(e) => setR({ ...r, prompt: e.target.value })}
        placeholder="Check yesterday's commits and open issues, and post a short summary of what changed and what needs attention."
      />
      <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
        {PRESETS.map((p) => (
          <ButtonBase
            key={p.cron}
            onClick={() => setR({ ...r, cron: p.cron })}
            sx={{ px: 1, py: 0.375, borderRadius: '999px', fontSize: '0.6875rem', border: 1, borderColor: r.cron === p.cron ? 'text.primary' : 'divider', color: r.cron === p.cron ? 'text.primary' : 'text.secondary' }}
          >
            {p.label}
          </ButtonBase>
        ))}
      </Stack>
      <TextField
        size="small"
        label="Schedule (cron)"
        value={r.cron}
        onChange={(e) => setR({ ...r, cron: e.target.value })}
        helperText={preset ? preset.label : 'minute hour day month weekday — e.g. 30 8 * * 1-5'}
      />
      {err && (
        <Typography variant="caption" color="error.main">
          {err}
        </Typography>
      )}
      <Stack direction="row" sx={{ gap: 1, justifyContent: 'flex-end' }}>
        <Button size="small" onClick={onCancel}>
          Cancel
        </Button>
        <Button
          size="small"
          variant="contained"
          onClick={() =>
            void onSave(r).catch((e) => setErr(e instanceof Error ? e.message : String(e)))
          }
        >
          Save routine
        </Button>
      </Stack>
    </Stack>
  )
}

/**
 * A teammate's own page: talk to it alone, its routines, its rules for
 * acting, and its memory — plus pause, stop and reset.
 */
export function BotPage({ root, agent, onBack, onEdit }: { root: string; agent: TeamAgent; onBack: () => void; onEdit: () => void }) {
  const theme = useTheme()
  const [tab, setTab] = useState<Tab>('routines')
  const setChannel = useBotsStore((s) => s.setChannel)
  const st = useBotsStore((s) => s.status[botKey(root, agent.id)])
  const { loadDm, loadStatus } = useBotsStore.getState()
  const saveAgent = useTeamStore((s) => s.save)
  const [settings, setSettings] = useState<BotSettings | null>(null)
  const [routines, setRoutines] = useState<Routine[]>([])
  const [editing, setEditing] = useState<Partial<Routine> | null>(null)
  const [memory, setMemory] = useState<string | null>(null)
  const [memoryDirty, setMemoryDirty] = useState(false)

  const args = { root, botId: agent.id }
  useEffect(() => {
    void loadDm(root, agent.id)
    void loadStatus(root)
    void call<BotSettings>('cmd_bots_settings', args).then((s) => s && setSettings(s))
    void call<Routine[]>('cmd_bots_routines', args).then((r) => setRoutines(r ?? []))
    void call<string>('cmd_bots_memory', args).then((m) => setMemory(m ?? ''))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [root, agent.id])

  const saveSettings = async (next: BotSettings) => {
    setSettings(next)
    const saved = await call<BotSettings>('cmd_bots_save_settings', { ...args, settings: next })
    if (saved) setSettings(saved)
  }

  const state = st?.state ?? (settings?.paused ? 'paused' : 'idle')
  const color = colorOf(agent)
  const face = useBotFace(root, agent.id)
  const idleLine = useMemo(() => IDLE_LINES[Math.floor(Math.random() * IDLE_LINES.length)], [])
  const caption =
    state === 'working' && face !== 'listening' ? (st?.activity ?? STATE_LABEL[face]) : face === 'idle' ? idleLine : STATE_LABEL[face]

  return (
    <Stack sx={{ height: '100%', minHeight: 0 }}>
      <Stack sx={{ p: 1.5, pb: 1, gap: 1.25, flexShrink: 0 }}>
        <ButtonBase onClick={onBack} sx={{ alignSelf: 'flex-start', gap: 0.5, color: 'text.secondary', fontSize: '0.75rem', borderRadius: '6px', px: 0.5 }}>
          <ArrowLeftLinear size={13} color="currentColor" />
          Team
        </ButtonBase>
        <Stack
          direction="row"
          sx={{
            alignItems: 'center',
            gap: 1.75,
            p: 1.5,
            borderRadius: '18px',
            border: 1,
            borderColor: alpha(color, 0.28),
            background: `linear-gradient(135deg, ${alpha(color, 0.16)} 0%, ${alpha(color, 0.04)} 100%)`,
          }}
        >
          <LiveAvatar root={root} agent={agent} size={64} />
          <Stack sx={{ flex: 1, minWidth: 0, gap: 0.25 }}>
            <Stack direction="row" sx={{ alignItems: 'baseline', gap: 0.75 }}>
              <Typography variant="body2" sx={{ fontWeight: 700 }}>
                {agent.name}
              </Typography>
              <Typography variant="caption" color="text.secondary">
                @{agent.id}
              </Typography>
            </Stack>
            <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.45 }}>
              {agent.role || 'No job described yet'}
            </Typography>
            <Stack
              direction="row"
              sx={{ gap: 0.75, alignItems: 'center', mt: 0.5, alignSelf: 'flex-start', maxWidth: '100%', px: 1, height: 22, borderRadius: '999px', bgcolor: alpha(color, 0.16) }}
            >
              {state === 'working' ? (
                <Orb state="working" px={12} label="Working" />
              ) : (
                <Box sx={{ width: 7, height: 7, borderRadius: '50%', flexShrink: 0, bgcolor: state === 'paused' ? 'text.disabled' : 'success.main' }} />
              )}
              <Typography variant="caption" sx={{ fontWeight: 600 }} noWrap>
                {caption}
              </Typography>
            </Stack>
          </Stack>
          <Tooltip title="Edit name, job, standing rules, model and tools">
            <IconButton size="small" onClick={onEdit}>
              <EditLinear size={15} color="currentColor" />
            </IconButton>
          </Tooltip>
        </Stack>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
          <Typography variant="caption" color="text.secondary" sx={{ flexShrink: 0 }}>
            Model
          </Typography>
          {/* Takes effect from its next turn; one in progress keeps its model. */}
          <ModelPicker mode="agent" variant="field" value={agent.model} onChange={(model) => void saveAgent(root, { ...agent, model }, agent.id)} />
        </Stack>
        <Stack direction="row" sx={{ gap: 0.75, flexWrap: 'wrap' }}>
          <Button size="small" variant="contained" onClick={() => setChannel(root, agent.id)} sx={{ borderRadius: '8px' }}>
            Message
          </Button>
          {state === 'working' && (
            <Button size="small" variant="outlined" color="error" onClick={() => void call('cmd_bots_stop', args)} sx={{ borderRadius: '8px' }}>
              Stop
            </Button>
          )}
          {settings && (
            <Button size="small" variant="outlined" onClick={() => void saveSettings({ ...settings, paused: !settings.paused })} sx={{ borderRadius: '8px' }}>
              {settings.paused ? 'Resume' : 'Pause'}
            </Button>
          )}
          <Tooltip title="Forget its conversation, messages and memory. Its definition stays.">
            <Button
              size="small"
              color="inherit"
              onClick={() => {
                if (window.confirm(`Reset ${agent.name}? Its conversation, direct messages and memory are deleted.`)) {
                  void call('cmd_bots_reset', args).then(() => {
                    void loadDm(root, agent.id)
                    setMemory('')
                  })
                }
              }}
              sx={{ borderRadius: '8px', color: 'text.secondary' }}
            >
              Reset
            </Button>
          </Tooltip>
        </Stack>
        <Stack direction="row" sx={{ gap: 0.25, borderBottom: 1, borderColor: 'divider' }}>
          {(['routines', 'contact', 'rules', 'memory'] as Tab[]).map((t) => (
            <ButtonBase
              key={t}
              onClick={() => setTab(t)}
              sx={{
                px: 1.25,
                py: 0.75,
                fontSize: '0.75rem',
                fontWeight: 600,
                textTransform: 'capitalize',
                color: tab === t ? 'text.primary' : 'text.secondary',
                borderBottom: 2,
                borderColor: tab === t ? 'text.primary' : 'transparent',
                mb: '-1px',
              }}
            >
              {t === 'routines' ? `Routines${routines.length ? ` · ${routines.length}` : ''}` : t}
            </ButtonBase>
          ))}
        </Stack>
      </Stack>

      {(
        <Box sx={{ flex: 1, minHeight: 0, overflow: 'auto', px: 1.5, pb: 2 }}>
          {tab === 'routines' && (
            <Stack sx={{ gap: 1.25, pt: 0.5 }}>
              <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.5 }}>
                Work {agent.name} does on a schedule, while you're elsewhere (the app keeps running in the tray). Results are
                posted in the room. Up to 50.
              </Typography>
              {routines.map((r) => (
                <Stack key={r.id} sx={{ gap: 0.5, p: 1.25, borderRadius: '10px', border: 1, borderColor: 'divider' }}>
                  <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
                    <Typography variant="body2" sx={{ fontWeight: 600, flex: 1, minWidth: 0 }} noWrap>
                      {r.title}
                    </Typography>
                    <Switch
                      size="small"
                      checked={r.enabled}
                      onChange={(e) => void call<Routine[]>('cmd_bots_save_routine', { ...args, routine: { ...r, enabled: e.target.checked } }).then((all) => all && setRoutines(all))}
                    />
                  </Stack>
                  <Typography variant="caption" color="text.secondary" sx={{ whiteSpace: 'pre-wrap' }}>
                    {r.prompt}
                  </Typography>
                  <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem' }}>
                    {PRESETS.find((p) => p.cron === r.cron)?.label ?? r.cron} · next {r.enabled ? when(r.nextRun) : 'off'} · {r.runs.length} recent run
                    {r.runs.length === 1 ? '' : 's'}
                    {r.runs.length ? ` · last ${when(r.runs[r.runs.length - 1].at)}` : ''}
                  </Typography>
                  <Stack direction="row" sx={{ gap: 0.5 }}>
                    <Button size="small" onClick={() => void call('cmd_bots_run_routine', { ...args, routineId: r.id })}>
                      Run now
                    </Button>
                    <Button size="small" onClick={() => setEditing(r)}>
                      Edit
                    </Button>
                    <IconButton size="small" onClick={() => void call<Routine[]>('cmd_bots_delete_routine', { ...args, routineId: r.id }).then((all) => setRoutines(all ?? []))}>
                      <TrashLinear size={14} color="currentColor" />
                    </IconButton>
                  </Stack>
                </Stack>
              ))}
              {editing ? (
                <RoutineEditor
                  initial={editing}
                  onCancel={() => setEditing(null)}
                  onSave={async (r) => {
                    const all = await call<Routine[]>('cmd_bots_save_routine', { ...args, routine: { id: '', runs: [], nextRun: null, enabled: true, ...r } })
                    if (all) setRoutines(all)
                    setEditing(null)
                  }}
                />
              ) : (
                routines.length < 50 && (
                  <Button size="small" variant="outlined" onClick={() => setEditing({})} sx={{ alignSelf: 'flex-start', borderRadius: '8px' }}>
                    New routine
                  </Button>
                )
              )}
            </Stack>
          )}

          {tab === 'contact' && <ContactTab root={root} agent={agent} />}

          {tab === 'rules' && settings && (
            <Stack sx={{ gap: 1.5, pt: 0.5 }}>
              <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.5 }}>
                What {agent.name} may do on its own. “If I asked” means only when your message asked for it; “Hand to me” means it
                tells you what to do instead. Reading files and talking are always allowed.
              </Typography>
              {RULES.map((r) => (
                <Stack key={r.key} direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
                  <Stack sx={{ flex: 1, minWidth: 0 }}>
                    <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem' }}>
                      {r.label}
                    </Typography>
                    <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }}>
                      {r.hint}
                    </Typography>
                  </Stack>
                  <Segmented value={settings.rules[r.key]} onChange={(v) => void saveSettings({ ...settings, rules: { ...settings.rules, [r.key]: v } })} />
                </Stack>
              ))}
              <Box sx={{ mt: 1, p: 1.25, borderRadius: '10px', bgcolor: alpha(theme.palette.text.primary, 0.04) }}>
                <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
                  <Stack sx={{ flex: 1 }}>
                    <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem' }}>
                      Always on
                    </Typography>
                    <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.45 }}>
                      Checks in on the project every few hours, read-only, and posts only when it finds something worth your
                      attention.
                    </Typography>
                  </Stack>
                  <Switch checked={settings.alwaysOn} onChange={(e) => void saveSettings({ ...settings, alwaysOn: e.target.checked })} />
                </Stack>
                {settings.alwaysOn && (
                  <Stack direction="row" sx={{ alignItems: 'center', gap: 1, mt: 1 }}>
                    <Typography variant="caption" color="text.secondary">
                      Every
                    </Typography>
                    <InputBase
                      type="number"
                      value={settings.checkinHours}
                      onChange={(e) => void saveSettings({ ...settings, checkinHours: Math.max(1, Math.min(168, Number(e.target.value) || 1)) })}
                      sx={{ width: 56, px: 1, borderRadius: '6px', border: 1, borderColor: 'divider', fontSize: '0.8125rem' }}
                    />
                    <Typography variant="caption" color="text.secondary">
                      hours · last {when(settings.lastCheckin)}
                    </Typography>
                  </Stack>
                )}
              </Box>
            </Stack>
          )}

          {tab === 'memory' && memory !== null && (
            <Stack sx={{ gap: 1, pt: 0.5 }}>
              <SectionLabel>What {agent.name} remembers</SectionLabel>
              <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.5 }}>
                Its own notes, which it keeps as it works. Edit or delete anything; it reads this at the start of every turn.
                It treats memory as a starting point and re-checks before acting on it.
              </Typography>
              <TextField
                multiline
                minRows={10}
                value={memory}
                onChange={(e) => {
                  setMemory(e.target.value)
                  setMemoryDirty(true)
                }}
                placeholder="Nothing yet."
                sx={{ '& textarea': { fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.75rem' } }}
              />
              {memoryDirty && (
                <Button
                  size="small"
                  variant="contained"
                  onClick={() => void call('cmd_bots_save_memory', { ...args, text: memory }).then(() => setMemoryDirty(false))}
                  sx={{ alignSelf: 'flex-end' }}
                >
                  Save
                </Button>
              )}
            </Stack>
          )}
        </Box>
      )}
    </Stack>
  )
}
