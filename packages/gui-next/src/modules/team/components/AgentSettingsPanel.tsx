import { useEffect, useState } from 'react'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { Box, ButtonBase, InputBase, Stack, Switch, Typography, alpha } from '../../../ui'
import {
  BrainLinear,
  ChevronRightLinear,
  CloseLinear,
  CloudLinear,
  CubeLinear,
  FolderLinear,
  GlobalLinear,
  MonitorLinear,
  PowerLinear,
  RepeatLinear,
  SparklesLinear,
  TrashLinear,
} from '../../../icons'
import { call } from '../../../ipc'
import { ModelPicker } from '../../ai/components/ModelPicker'
import { useTeamStore, type TeamAgent } from '../store/teamStore'
import type { BotSettings, Level, Routine, Workplace } from '../store/botsStore'
import { Avatar, colorOf, faceOf } from './bits'
import { RoutineDialog } from './RoutineDialog'

type Tab = 'agent' | 'environment'
type Sheet = null | 'instructions' | 'rules' | 'routines' | 'memory' | 'folder'

const RULES: { key: keyof BotSettings['rules']; label: string; hint: string }[] = [
  { key: 'files', label: 'Change files', hint: 'write, edit' },
  { key: 'commands', label: 'Run commands', hint: 'terminal, scripts' },
  { key: 'computer', label: 'Use the screen or browser', hint: 'clicking and typing' },
  { key: 'web', label: 'Use the web', hint: 'search, read pages' },
  { key: 'outside', label: 'Act outside this computer', hint: 'email, texts, connected apps' },
]
const LEVELS: { value: Level; label: string }[] = [
  { value: 'auto', label: 'Just do it' },
  { value: 'if_requested', label: 'If I asked' },
  { value: 'ask', label: 'Ask me' },
  { value: 'hand_off', label: 'Hand to me' },
]

const PLACES: { id: Workplace; label: string; note?: string; Icon: typeof SparklesLinear; ready: boolean }[] = [
  { id: 'auto', label: 'Auto', Icon: SparklesLinear, ready: true },
  { id: 'cloud_desktop', label: 'Cloud desktop', note: 'Not set up', Icon: CloudLinear, ready: false },
  { id: 'private_desktop', label: 'Private desktop', note: 'Not set up', Icon: CubeLinear, ready: false },
  { id: 'this_pc', label: 'This PC', Icon: MonitorLinear, ready: true },
  { id: 'browser', label: 'Browser', Icon: GlobalLinear, ready: true },
  { id: 'off', label: 'Off', Icon: PowerLinear, ready: true },
]
const PLACE_NOTE: Record<Workplace, string> = {
  auto: 'Uses your browser or this computer’s screen, whichever the task needs.',
  this_pc: 'Works on this computer’s screen, and in your browser.',
  browser: 'Works in your browser only. It never touches the screen.',
  private_desktop: 'A desktop of its own on this computer. Coming soon.',
  cloud_desktop: 'A desktop in the cloud that runs while this computer sleeps. Coming soon.',
  off: 'No screen and no browser: it searches and reads the web, and asks you for the rest.',
}

const card = { borderRadius: '16px', bgcolor: 'action.hover', overflow: 'hidden' } as const
const rowSx = {
  display: 'flex',
  alignItems: 'center',
  gap: '12px',
  width: '100%',
  minHeight: 56,
  px: '16px',
  py: '10px',
  textAlign: 'left',
  justifyContent: 'flex-start',
  '& + &': { borderTop: 1, borderColor: 'divider' },
} as const

function IconTile({ Icon }: { Icon: typeof SparklesLinear }) {
  return (
    <Box sx={{ width: 32, height: 32, borderRadius: '10px', display: 'grid', placeItems: 'center', bgcolor: 'action.selected', color: 'text.secondary', flexShrink: 0 }}>
      <Icon size={16} color="currentColor" />
    </Box>
  )
}

/** One line of a card that opens something: icon, title, its value, a chevron. */
function Row({ Icon, title, sub, value, onClick }: { Icon?: typeof SparklesLinear; title: string; sub?: string; value?: string; onClick?: () => void }) {
  return (
    <ButtonBase onClick={onClick} disabled={!onClick} sx={{ ...rowSx, '&:hover': { bgcolor: 'action.hover' } }}>
      {Icon && <IconTile Icon={Icon} />}
      <Stack sx={{ flex: 1, minWidth: 0 }}>
        <Typography sx={{ fontSize: '14px', fontWeight: 500 }} noWrap>
          {title}
        </Typography>
        {sub && (
          <Typography sx={{ fontSize: '12px', color: 'text.secondary' }} noWrap>
            {sub}
          </Typography>
        )}
      </Stack>
      {value && (
        <Typography sx={{ fontSize: '13px', color: 'text.secondary', flexShrink: 0 }} noWrap>
          {value}
        </Typography>
      )}
      {onClick && (
        <Box sx={{ display: 'flex', color: 'text.disabled', flexShrink: 0 }}>
          <ChevronRightLinear size={14} color="currentColor" />
        </Box>
      )}
    </ButtonBase>
  )
}

/** Two to four choices on one track. */
function Segments<T extends string>({ value, options, onChange }: { value: T | null; options: { value: T; label: string }[]; onChange: (v: T) => void }) {
  return (
    <Stack direction="row" sx={{ p: '3px', borderRadius: '999px', bgcolor: 'action.selected', gap: '2px' }}>
      {options.map((o) => (
        <ButtonBase
          key={o.value}
          onClick={() => onChange(o.value)}
          aria-pressed={value === o.value}
          sx={{ flex: 1, height: 32, px: '10px', borderRadius: '999px', fontSize: '13px', fontWeight: value === o.value ? 600 : 400, color: value === o.value ? 'text.primary' : 'text.secondary', bgcolor: value === o.value ? 'background.paper' : 'transparent', whiteSpace: 'nowrap' }}
        >
          {o.label}
        </ButtonBase>
      ))}
    </Stack>
  )
}

function SheetHeader({ title, sub, onBack }: { title: string; sub?: string; onBack: () => void }) {
  return (
    <Stack sx={{ gap: '4px', mb: '16px' }}>
      <ButtonBase onClick={onBack} sx={{ alignSelf: 'flex-start', gap: '4px', fontSize: '12px', color: 'text.secondary', borderRadius: '6px', '&:hover': { color: 'text.primary' } }}>
        <Box sx={{ display: 'flex', transform: 'rotate(180deg)' }}>
          <ChevronRightLinear size={12} color="currentColor" />
        </Box>
        Settings
      </ButtonBase>
      <Typography sx={{ fontSize: '20px', fontWeight: 600 }}>{title}</Typography>
      {sub && <Typography sx={{ fontSize: '13px', color: 'text.secondary', lineHeight: 1.5 }}>{sub}</Typography>}
    </Stack>
  )
}

const when = (ms: number | null) => (ms ? new Date(ms).toLocaleString(undefined, { weekday: 'short', hour: 'numeric', minute: '2-digit' }) : 'off')

/**
 * A 24/7 agent's settings beside its chat, in two tabs: the agent itself
 * (model, instructions, what it may do alone) and its environment (where it
 * works, its routines, its memory, its folder). A row opens its own page in
 * the panel.
 */
export function AgentSettingsPanel({ root, agent, onClose, onEditProfile }: { root: string; agent: TeamAgent; onClose: () => void; onEditProfile: () => void }) {
  const color = colorOf(agent)
  const save = useTeamStore((s) => s.save)
  const [tab, setTab] = useState<Tab>('agent')
  const [sheet, setSheet] = useState<Sheet>(null)
  const [settings, setSettings] = useState<BotSettings | null>(null)
  const [routines, setRoutines] = useState<Routine[]>([])
  const [memory, setMemory] = useState<string | null>(null)
  const [memoryDirty, setMemoryDirty] = useState(false)
  const [instructions, setInstructions] = useState(agent.instructions)
  const [editing, setEditing] = useState<Partial<Routine> | null>(null)
  const [shot, setShot] = useState<string | null>(null)
  const args = { root, botId: agent.id }

  useEffect(() => {
    void call<BotSettings>('cmd_bots_settings', args).then((s) => s && setSettings(s))
    void call<Routine[]>('cmd_bots_routines', args).then((r) => setRoutines(r ?? []))
    void call<string>('cmd_bots_memory', args).then((m) => setMemory(m ?? ''))
    setInstructions(agent.instructions)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [root, agent.id])

  const place = settings?.workplace ?? 'auto'
  // The screen it would work on, refreshed while the environment tab shows it.
  useEffect(() => {
    if (tab !== 'environment' || sheet || !(place === 'this_pc' || place === 'auto')) return
    let live = true
    const grab = () =>
      void call<{ image_base64: string; media_type: string }>('cmd_capture_screen')
        .then((s) => live && s && setShot(`data:${s.media_type};base64,${s.image_base64}`))
        .catch(() => {})
    grab()
    const t = window.setInterval(grab, 4000)
    return () => {
      live = false
      window.clearInterval(t)
    }
  }, [tab, sheet, place])

  const saveSettings = async (next: BotSettings) => {
    setSettings(next)
    const saved = await call<BotSettings>('cmd_bots_save_settings', { ...args, settings: next })
    if (saved) setSettings(saved)
  }
  const saveRoutine = async (r: Partial<Routine>) => {
    const all = await call<Routine[]>('cmd_bots_save_routine', { ...args, routine: { id: '', runs: [], nextRun: null, enabled: true, ...r } })
    if (all) setRoutines(all)
  }

  const levels = settings ? RULES.map((r) => settings.rules[r.key]) : []
  const uniform = levels.every((l) => l === levels[0]) ? levels[0] : null
  const acts: 'ask' | 'auto' | null = uniform === 'ask' || uniform === 'auto' ? uniform : null
  const setAll = (level: Level) => settings && void saveSettings({ ...settings, rules: Object.fromEntries(RULES.map((r) => [r.key, level])) as unknown as BotSettings['rules'] })

  const chooseFolder = async () => {
    const picked = await openDialog({ directory: true, multiple: false })
    if (typeof picked === 'string' && settings) void saveSettings({ ...settings, folder: picked })
  }

  const body = (() => {
    if (sheet === 'instructions')
      return (
        <>
          <SheetHeader title="Instructions" sub={`How ${agent.name} should work, every time.`} onBack={() => setSheet(null)} />
          <InputBase
            multiline
            minRows={10}
            value={instructions}
            onChange={(e) => setInstructions(e.target.value)}
            onBlur={() => instructions !== agent.instructions && void save(root, { ...agent, instructions }, agent.id)}
            placeholder={'Check the inbox at 8am.\nNever send anything without asking me first.'}
            sx={{ ...card, px: '16px', py: '12px', fontSize: '14px', lineHeight: 1.55, alignItems: 'flex-start' }}
          />
          <Typography sx={{ fontSize: '12px', color: 'text.disabled', mt: '8px' }}>Saved when you click away.</Typography>
        </>
      )
    if (sheet === 'rules' && settings)
      return (
        <>
          <SheetHeader title={`What ${agent.name} may do alone`} sub="Reading files and talking are always allowed." onBack={() => setSheet(null)} />
          <Stack sx={{ gap: '16px' }}>
            {RULES.map((r) => (
              <Stack key={r.key} sx={{ gap: '8px' }}>
                <Stack direction="row" sx={{ alignItems: 'baseline', gap: '8px' }}>
                  <Typography sx={{ fontSize: '14px', fontWeight: 500 }}>{r.label}</Typography>
                  <Typography sx={{ fontSize: '12px', color: 'text.disabled' }}>{r.hint}</Typography>
                </Stack>
                <Segments value={settings.rules[r.key]} options={LEVELS} onChange={(v) => void saveSettings({ ...settings, rules: { ...settings.rules, [r.key]: v } })} />
              </Stack>
            ))}
          </Stack>
        </>
      )
    if (sheet === 'routines')
      return (
        <>
          <SheetHeader title="Routines" sub={`Work ${agent.name} does on a schedule. Results arrive in its chat.`} onBack={() => setSheet(null)} />
          <Stack sx={{ gap: '8px' }}>
            {routines.map((r) => (
              <Stack key={r.id} sx={{ ...card, p: '14px', gap: '6px' }}>
                <Stack direction="row" sx={{ alignItems: 'center', gap: '8px' }}>
                  <ButtonBase onClick={() => setEditing(r)} sx={{ flex: 1, minWidth: 0, justifyContent: 'flex-start', textAlign: 'left' }}>
                    <Typography sx={{ fontSize: '14px', fontWeight: 500 }} noWrap>
                      {r.title}
                    </Typography>
                  </ButtonBase>
                  <Switch size="small" checked={r.enabled} onChange={(e) => void saveRoutine({ ...r, enabled: e.target.checked })} />
                </Stack>
                <Typography sx={{ fontSize: '12px', color: 'text.secondary' }}>
                  {r.once ? 'Once' : 'Repeats'} · next {r.enabled ? when(r.nextRun) : 'off'}
                </Typography>
                <Stack direction="row" sx={{ gap: '4px', mt: '2px' }}>
                  <ButtonBase onClick={() => void call('cmd_bots_run_routine', { ...args, routineId: r.id })} sx={{ height: 26, px: '10px', borderRadius: '8px', fontSize: '12px', bgcolor: 'action.selected' }}>
                    Run now
                  </ButtonBase>
                  <ButtonBase onClick={() => setEditing(r)} sx={{ height: 26, px: '10px', borderRadius: '8px', fontSize: '12px', color: 'text.secondary' }}>
                    Edit
                  </ButtonBase>
                  <ButtonBase
                    aria-label="Delete"
                    onClick={() => void call<Routine[]>('cmd_bots_delete_routine', { ...args, routineId: r.id }).then((all) => setRoutines(all ?? []))}
                    sx={{ width: 26, height: 26, borderRadius: '8px', color: 'text.secondary', ml: 'auto', '&:hover': { color: 'error.main' } }}
                  >
                    <TrashLinear size={14} color="currentColor" />
                  </ButtonBase>
                </Stack>
              </Stack>
            ))}
            <ButtonBase onClick={() => setEditing({})} sx={{ height: 44, borderRadius: '14px', fontSize: '14px', fontWeight: 600, bgcolor: alpha(color, 0.16), '&:hover': { bgcolor: alpha(color, 0.24) } }}>
              New routine
            </ButtonBase>
          </Stack>
        </>
      )
    if (sheet === 'memory')
      return (
        <>
          <SheetHeader title="What it remembers" sub={`${agent.name}'s own notes, read at the start of every turn. Change or delete anything.`} onBack={() => setSheet(null)} />
          <InputBase
            multiline
            minRows={12}
            value={memory ?? ''}
            onChange={(e) => {
              setMemory(e.target.value)
              setMemoryDirty(true)
            }}
            placeholder="Nothing yet."
            sx={{ ...card, px: '16px', py: '12px', fontSize: '13px', lineHeight: 1.55, alignItems: 'flex-start' }}
          />
          {memoryDirty && (
            <ButtonBase
              onClick={() => void call('cmd_bots_save_memory', { ...args, text: memory }).then(() => setMemoryDirty(false))}
              sx={{ alignSelf: 'flex-end', mt: '10px', height: 34, px: '16px', borderRadius: '10px', fontSize: '13px', fontWeight: 600, bgcolor: color, color: (t) => t.palette.getContrastText(color) }}
            >
              Save
            </ButtonBase>
          )}
        </>
      )
    if (sheet === 'folder' && settings)
      return (
        <>
          <SheetHeader title="Working folder" sub={`Where ${agent.name} keeps and edits files.`} onBack={() => setSheet(null)} />
          <Stack sx={card}>
            <ButtonBase onClick={() => void saveSettings({ ...settings, folder: null })} sx={{ ...rowSx, '&:hover': { bgcolor: 'action.hover' } }}>
              <Stack sx={{ flex: 1, minWidth: 0 }}>
                <Typography sx={{ fontSize: '14px', fontWeight: 500 }}>Its own private folder</Typography>
                <Typography sx={{ fontSize: '12px', color: 'text.secondary' }}>Nothing else on this computer is touched</Typography>
              </Stack>
              {!settings.folder && <Typography sx={{ color }}>✓</Typography>}
            </ButtonBase>
            <ButtonBase onClick={() => void chooseFolder()} sx={{ ...rowSx, '&:hover': { bgcolor: 'action.hover' } }}>
              <Stack sx={{ flex: 1, minWidth: 0 }}>
                <Typography sx={{ fontSize: '14px', fontWeight: 500 }}>A folder you choose</Typography>
                <Typography sx={{ fontSize: '12px', color: 'text.secondary' }} noWrap title={settings.folder ?? undefined}>
                  {settings.folder ?? 'Desktop, a project, Downloads'}
                </Typography>
              </Stack>
              {settings.folder && <Typography sx={{ color }}>✓</Typography>}
            </ButtonBase>
          </Stack>
        </>
      )
    if (tab === 'agent')
      return (
        <Stack sx={{ gap: '16px' }}>
          <Stack sx={card}>
            <ButtonBase onClick={onEditProfile} sx={{ ...rowSx, minHeight: 72, '&:hover': { bgcolor: 'action.hover' } }}>
              <Avatar {...faceOf(agent)} size={40} />
              <Stack sx={{ flex: 1, minWidth: 0 }}>
                <Typography sx={{ fontSize: '15px', fontWeight: 600 }} noWrap>
                  {agent.name}
                </Typography>
                <Typography sx={{ fontSize: '12px', color: 'text.secondary' }} noWrap>
                  {agent.role || 'Name, job and look'}
                </Typography>
              </Stack>
              <Box sx={{ display: 'flex', color: 'text.disabled' }}>
                <ChevronRightLinear size={14} color="currentColor" />
              </Box>
            </ButtonBase>
          </Stack>
          <Stack sx={card}>
            <Stack direction="row" sx={{ ...rowSx, justifyContent: 'space-between' }}>
              <IconTile Icon={SparklesLinear} />
              <Typography sx={{ flex: 1, fontSize: '14px', fontWeight: 500 }}>Model</Typography>
              <Box sx={{ minWidth: 0, maxWidth: 200 }}>
                <ModelPicker mode="agent" value={agent.model} onChange={(model) => void save(root, { ...agent, model }, agent.id)} />
              </Box>
            </Stack>
            <Row Icon={RepeatLinear} title="Instructions" sub={agent.instructions ? agent.instructions.split('\n')[0] : `Tell ${agent.name} how to work for you`} onClick={() => setSheet('instructions')} />
            {settings && (
              <Stack sx={{ ...rowSx, flexDirection: 'column', alignItems: 'stretch', gap: '10px', py: '14px' }}>
                <Typography sx={{ fontSize: '14px', fontWeight: 500 }}>Before {agent.name} acts</Typography>
                <Segments value={acts} options={[{ value: 'ask', label: 'Ask me first' }, { value: 'auto', label: 'Just do it' }]} onChange={setAll} />
                <ButtonBase onClick={() => setSheet('rules')} sx={{ alignSelf: 'flex-start', fontSize: '12px', color: 'text.secondary', '&:hover': { color: 'text.primary' } }}>
                  {acts ? 'Fine-tune by kind of action' : 'Custom: set by kind of action'}
                </ButtonBase>
              </Stack>
            )}
          </Stack>
          {settings && (
            <Stack sx={card}>
              <Stack direction="row" sx={rowSx}>
                <IconTile Icon={PowerLinear} />
                <Stack sx={{ flex: 1, minWidth: 0 }}>
                  <Typography sx={{ fontSize: '14px', fontWeight: 500 }}>Always on</Typography>
                  <Typography sx={{ fontSize: '12px', color: 'text.secondary' }}>Checks in every {settings.checkinHours} h, posts only what matters</Typography>
                </Stack>
                <Switch checked={settings.alwaysOn} onChange={(e) => void saveSettings({ ...settings, alwaysOn: e.target.checked })} />
              </Stack>
              <Stack direction="row" sx={rowSx}>
                <IconTile Icon={CloseLinear} />
                <Typography sx={{ flex: 1, fontSize: '14px', fontWeight: 500 }}>Paused</Typography>
                <Switch checked={settings.paused} onChange={(e) => void saveSettings({ ...settings, paused: e.target.checked })} />
              </Stack>
            </Stack>
          )}
        </Stack>
      )
    return (
      <Stack sx={{ gap: '16px' }}>
        {(place === 'this_pc' || place === 'auto') && (
          <Stack sx={{ gap: '8px' }}>
            <Stack direction="row" sx={{ justifyContent: 'space-between' }}>
              <Typography sx={{ fontSize: '13px', color: 'text.secondary' }}>{agent.name}'s screen</Typography>
              <Typography sx={{ fontSize: '12px', color: 'text.disabled' }}>this computer</Typography>
            </Stack>
            <Box sx={{ ...card, aspectRatio: '16 / 9', display: 'grid', placeItems: 'center' }}>
              {shot ? (
                <Box component="img" src={shot} alt="This computer's screen" sx={{ width: '100%', height: '100%', objectFit: 'cover' }} />
              ) : (
                <Typography sx={{ fontSize: '12px', color: 'text.disabled' }}>Capturing the screen…</Typography>
              )}
            </Box>
          </Stack>
        )}
        <Stack sx={{ ...card, p: '16px', gap: '12px' }}>
          <Typography sx={{ fontSize: '15px', fontWeight: 600 }}>Where {agent.name} works</Typography>
          <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: '8px' }}>
            {PLACES.map((p) => {
              const on = place === p.id
              return (
                <ButtonBase
                  key={p.id}
                  disabled={!p.ready || !settings}
                  onClick={() => settings && void saveSettings({ ...settings, workplace: p.id })}
                  aria-pressed={on}
                  sx={{
                    flexDirection: 'column',
                    gap: '6px',
                    minHeight: 84,
                    px: '8px',
                    py: '12px',
                    borderRadius: '12px',
                    bgcolor: on ? alpha(color, 0.16) : 'background.paper',
                    boxShadow: on ? `inset 0 0 0 1.5px ${alpha(color, 0.75)}` : (t) => `inset 0 0 0 1px ${t.palette.divider}`,
                    color: on ? 'text.primary' : 'text.secondary',
                    '&:disabled': { opacity: 0.5 },
                  }}
                >
                  <p.Icon size={18} color={on ? color : 'currentColor'} />
                  <Typography sx={{ fontSize: '13px', fontWeight: on ? 600 : 500, color: 'inherit', textAlign: 'center' }}>{p.label}</Typography>
                  {p.note && <Typography sx={{ fontSize: '11px', color: 'text.disabled' }}>{p.note}</Typography>}
                </ButtonBase>
              )
            })}
          </Box>
          <Typography sx={{ fontSize: '12px', color: 'text.secondary', lineHeight: 1.5 }}>{PLACE_NOTE[place]}</Typography>
        </Stack>
        <Stack sx={card}>
          <Row Icon={RepeatLinear} title="Routines" value={routines.length ? `${routines.filter((r) => r.enabled).length} on` : 'None'} onClick={() => setSheet('routines')} />
          <Row Icon={BrainLinear} title="What it remembers" value={memory ? `${memory.split('\n').filter((l) => l.trim()).length} notes` : 'Nothing yet'} onClick={() => setSheet('memory')} />
          <Row Icon={FolderLinear} title="Working folder" value={settings?.folder ? settings.folder.split(/[\\/]/).filter(Boolean).pop() : 'Private'} onClick={() => setSheet('folder')} />
        </Stack>
      </Stack>
    )
  })()

  return (
    <Stack sx={{ width: 400, flexShrink: 0, height: '100%', borderLeft: 1, borderColor: 'divider', bgcolor: 'background.paper', minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', px: '16px', pt: '14px', pb: '10px' }}>
        <Box sx={{ flex: 1 }}>
          {!sheet && (
            <Segments
              value={tab}
              options={[
                { value: 'agent', label: 'Agent settings' },
                { value: 'environment', label: 'Environment' },
              ]}
              onChange={setTab}
            />
          )}
        </Box>
        <ButtonBase onClick={onClose} aria-label="Close settings" sx={{ width: 32, height: 32, borderRadius: '50%', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}>
          <CloseLinear size={16} color="currentColor" />
        </ButtonBase>
      </Stack>
      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: '16px', pb: '20px' }}>{body}</Box>
      {editing && <RoutineDialog agent={agent} initial={editing} onClose={() => setEditing(null)} onSave={saveRoutine} />}
    </Stack>
  )
}
