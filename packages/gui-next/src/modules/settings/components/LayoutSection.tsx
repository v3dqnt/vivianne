import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { Stack, Typography, Divider, Box, Button, TextField, Select, MenuItem, Switch, ToggleButtonGroup, ToggleButton, IconButton, Tooltip, Alert, ButtonBase } from '../../../ui'
import { TrashLinear, UndoLinear } from '../../../icons'
import { call, hasTauri } from '../../../ipc'
import { isMac } from '../../../lib/platform'
import { INSTANT_ENABLED } from '../../instant/enabled'
import {
  PRESET_LABELS,
  applyPreset,
  applySavedLayout,
  deleteSavedLayout,
  renameSavedLayout,
  resetLayout,
  saveCurrentLayout,
  setStartupLayout,
  useSavedLayouts,
  type Preset,
} from '../../workspace/layout'
import {
  clashes,
  comboFromEvent,
  formatCombo,
  isBindable,
  keysFor,
  setRecording,
  shortcutActions,
  useShortcutStore,
  type ShortcutAction,
} from '../../../lib/shortcuts'
import { useLayoutPrefs } from '../store/layoutPrefsStore'
import { useSettingsUi } from '../store/uiStore'
import { DEFAULT_PREFS, type NotchPrefs } from '../../instant/components/RestingNotch'

function Row({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 2, py: 0.5 }}>
      <Stack sx={{ flex: 1, minWidth: 0 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }}>
          {label}
        </Typography>
        {hint && (
          <Typography variant="caption" color="text.secondary">
            {hint}
          </Typography>
        )}
      </Stack>
      <Box sx={{ flexShrink: 0, display: 'flex', alignItems: 'center', gap: 1 }}>{children}</Box>
    </Stack>
  )
}

function Segmented<T extends string>({ value, options, onChange }: { value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return (
    <ToggleButtonGroup
      size="small"
      exclusive
      value={value}
      onChange={(_: unknown, next: T | null) => next && onChange(next)}
      sx={{ '& .ui-ToggleButton': { px: 1.25, py: 0.25, fontSize: '0.75rem', textTransform: 'none' } }}
    >
      {options.map(([v, label]) => (
        <ToggleButton key={v} value={v}>
          {label}
        </ToggleButton>
      ))}
    </ToggleButtonGroup>
  )
}

/** A key field: click, press the keys, done. Escape cancels; Backspace clears. */
function KeyRecorder({ value, onChange, placeholder = 'Not set' }: { value: string | null; onChange: (keys: string) => void; placeholder?: string }) {
  const [rec, setRec] = useState(false)
  const [hint, setHint] = useState<string | null>(null)

  useEffect(() => {
    if (!rec) return
    setRecording(true)
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()
      if (e.key === 'Escape') return setRec(false)
      if (e.key === 'Backspace' || e.key === 'Delete') {
        onChange('')
        return setRec(false)
      }
      const combo = comboFromEvent(e)
      if (!combo) return
      if (!isBindable(combo)) {
        setHint('Add Ctrl, Alt or Shift (a plain key would fire while you type)')
        return
      }
      onChange(combo)
      setRec(false)
    }
    window.addEventListener('keydown', onKey, true)
    return () => {
      setRecording(false)
      window.removeEventListener('keydown', onKey, true)
    }
  }, [rec, onChange])

  return (
    <Tooltip title={hint ?? (rec ? 'Press the keys · Esc to cancel · Backspace to clear' : 'Click to change')}>
      <ButtonBase
        onClick={() => {
          setHint(null)
          setRec(true)
        }}
        onBlur={() => setRec(false)}
        sx={{
          minWidth: 120,
          justifyContent: 'center',
          px: 1,
          py: 0.4,
          borderRadius: 1.5,
          border: 1,
          borderColor: rec ? 'primary.main' : 'divider',
          fontFamily: 'monospace',
          fontSize: '0.75rem',
          color: value ? 'text.primary' : 'text.secondary',
          bgcolor: rec ? 'action.selected' : 'transparent',
        }}
      >
        {rec ? 'Press keys…' : value ? formatCombo(value) : placeholder}
      </ButtonBase>
    </Tooltip>
  )
}

function ShortcutRow({ action }: { action: ShortcutAction }) {
  const overrides = useShortcutStore((s) => s.overrides)
  const set = useShortcutStore((s) => s.set)
  const reset = useShortcutStore((s) => s.reset)
  const keys = keysFor(action.id, overrides)
  const clash = keys ? clashes(action.id, keys) : []
  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1, py: 0.25 }}>
      <Stack sx={{ flex: 1, minWidth: 0 }}>
        <Typography variant="body2">{action.label}</Typography>
        {clash.length > 0 && (
          <Typography variant="caption" color="warning.main">
            Also used by: {clash.map((c) => c.label).join(', ')}
          </Typography>
        )}
      </Stack>
      <KeyRecorder value={keys} onChange={(k) => set(action.id, k)} />
      <Tooltip title="Back to the default">
        <span>
          <IconButton size="small" disabled={!(action.id in overrides)} onClick={() => reset(action.id)}>
            <UndoLinear size={13} color="currentColor" />
          </IconButton>
        </span>
      </Tooltip>
    </Stack>
  )
}

/** The app's `Mod+Shift+Space` → the backend's `Ctrl+Shift+Space`. */
function toChord(combo: string): string {
  return combo
    .split('+')
    .map((p) => (p === 'Mod' ? (isMac ? 'Cmd' : 'Ctrl') : p === 'Meta' ? 'Super' : p))
    .join('+')
}

function InstantChordRow() {
  const [chord, setChord] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    if (!hasTauri()) return
    void call<string>('cmd_instant_chord').then((c) => setChord(c ?? null))
    void call<string | null>('cmd_instant_shortcut_error').then((e) => setError(e ?? null))
  }, [])
  // Shown in the app's own notation.
  const shown = chord?.replace(/\b(Ctrl|Cmd)\b/, isMac ? 'Mod' : 'Ctrl').replace(/^Ctrl/, isMac ? 'Ctrl' : 'Mod') ?? null
  const save = async (combo: string) => {
    setError(null)
    try {
      await call('cmd_instant_set_chord', { chord: combo ? toChord(combo) : '' })
      setChord((await call<string>('cmd_instant_chord')) ?? null)
    } catch (e) {
      setError(String(e))
    }
  }
  return (
    <>
      <Row label="Viv Instant" hint="Hold anywhere on your computer to talk to Vivianne. Works even when the app is in the background.">
        <KeyRecorder value={shown} onChange={(k) => void save(k)} placeholder="Default" />
      </Row>
      {error && <Alert severity="warning">{error}</Alert>}
      <AssistChordRow />
    </>
  )
}

/** The keys that start Viv Instant already knowing which window you're in. */
function AssistChordRow() {
  const [chord, setChord] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    if (hasTauri()) void call<string>('cmd_instant_assist_chord').then((c) => setChord(c ?? null))
  }, [])
  const shown = chord?.replace(/\b(Ctrl|Cmd)\b/, isMac ? 'Mod' : 'Ctrl').replace(/^Ctrl/, isMac ? 'Ctrl' : 'Mod') ?? null
  const save = async (combo: string) => {
    setError(null)
    try {
      await call('cmd_instant_set_assist_chord', { chord: combo ? toChord(combo) : '' })
      setChord((await call<string>('cmd_instant_assist_chord')) ?? null)
    } catch (e) {
      setError(String(e))
    }
  }
  return (
    <>
      <Row label="Ask about this window" hint="Hold while you're in any app and ask about what's on screen, e.g. “why is this failing?”. Say nothing and it tells you what it sees.">
        <KeyRecorder value={shown} onChange={(k) => void save(k)} placeholder="Default" />
      </Row>
      {error && <Alert severity="warning">{error}</Alert>}
    </>
  )
}

function NotchRows() {
  const [prefs, setPrefs] = useState<NotchPrefs>(DEFAULT_PREFS)
  useEffect(() => {
    if (hasTauri()) void call<NotchPrefs>('cmd_notch_prefs').then((p) => p && setPrefs(p))
  }, [])
  const update = (patch: Partial<NotchPrefs>) => {
    const next = { ...prefs, ...patch }
    setPrefs(next)
    void call<NotchPrefs>('cmd_notch_set_prefs', { prefs: next }).then((p) => p && setPrefs(p))
  }
  return (
    <>
      <Row label="Position">
        <Segmented<NotchPrefs['position']>
          value={prefs.position}
          options={[
            ['left', 'Left'],
            ['center', 'Centre'],
            ['right', 'Right'],
          ]}
          onChange={(position) => update({ position })}
        />
      </Row>
      <Row label="Size">
        <Segmented<string>
          value={String(prefs.scale)}
          options={[
            ['0.85', 'Small'],
            ['1', 'Medium'],
            ['1.2', 'Large'],
          ]}
          onChange={(v) => update({ scale: Number(v) })}
        />
      </Row>
      <Row label="Agents at work" hint="The list when you hover it">
        <Switch size="small" checked={prefs.showAgents} onChange={() => update({ showAgents: !prefs.showAgents })} />
      </Row>
      <Row label="What they're doing" hint="The current step, next to the agent's name">
        <Switch size="small" checked={prefs.showActivity} onChange={() => update({ showActivity: !prefs.showActivity })} />
      </Row>
      <Row label="Talk button">
        <Switch size="small" checked={prefs.showMic} onChange={() => update({ showMic: !prefs.showMic })} />
      </Row>
    </>
  )
}

/** Settings → Layout & shortcuts. */
export function LayoutSection() {
  const closeSettings = useSettingsUi((s) => s.closeSettings)
  const saved = useSavedLayouts((s) => s.saved)
  const startup = useSavedLayouts((s) => s.startup)
  const sidebarSide = useLayoutPrefs((s) => s.sidebarSide)
  const showRail = useLayoutPrefs((s) => s.showRail)
  const updatePrefs = useLayoutPrefs((s) => s.update)
  const resetAll = useShortcutStore((s) => s.resetAll)
  const overrides = useShortcutStore((s) => s.overrides)
  const [name, setName] = useState('')
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null)

  // Closing Settings on the way out: a layout rearranges exactly what the
  // dialog covers, so a working button would look dead behind it.
  const apply = (fn: () => void) => {
    fn()
    closeSettings()
  }

  const groups = useMemo(() => {
    const out = new Map<string, ShortcutAction[]>()
    for (const a of shortcutActions()) out.set(a.group, [...(out.get(a.group) ?? []), a])
    return [...out.entries()]
    // Rebuilt with the saved layouts (they're actions too).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [saved])

  return (
    <Stack sx={{ gap: 1.5 }}>
      <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
        Layouts
      </Typography>
      <Typography variant="caption" color="text.secondary">
        Drag any panel, the chat included, onto another to share its tabs or onto an edge to split beside it. Start from one of
        these, or save your own arrangement.
      </Typography>
      <Stack direction="row" sx={{ gap: 1, flexWrap: 'wrap' }}>
        {(Object.keys(PRESET_LABELS) as Preset[]).map((p) => (
          <Button key={p} size="small" variant="outlined" onClick={() => apply(() => applyPreset(p))} sx={{ textTransform: 'none' }}>
            {PRESET_LABELS[p]}
          </Button>
        ))}
      </Stack>
      {saved.length > 0 && (
        <Stack sx={{ gap: 0.5 }}>
          {saved.map((l) => (
            <Stack key={l.id} direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.25, py: 0.5, borderRadius: 2, border: 1, borderColor: 'divider' }}>
              {renaming?.id === l.id ? (
                <TextField
                  size="small"
                  value={renaming.name}
                  autoFocus
                  onChange={(e) => setRenaming({ id: l.id, name: e.target.value })}
                  onBlur={() => {
                    renameSavedLayout(l.id, renaming.name)
                    setRenaming(null)
                  }}
                  onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
                  sx={{ flex: 1 }}
                />
              ) : (
                <Typography variant="body2" sx={{ flex: 1, cursor: 'text' }} onDoubleClick={() => setRenaming({ id: l.id, name: l.name })}>
                  {l.name}
                </Typography>
              )}
              <Button size="small" onClick={() => apply(() => applySavedLayout(l.id))} sx={{ textTransform: 'none' }}>
                Apply
              </Button>
              <Tooltip title="Delete">
                <IconButton size="small" onClick={() => deleteSavedLayout(l.id)}>
                  <TrashLinear size={13} color="currentColor" />
                </IconButton>
              </Tooltip>
            </Stack>
          ))}
        </Stack>
      )}
      <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
        <TextField size="small" placeholder="Name this arrangement" value={name} onChange={(e) => setName(e.target.value)} sx={{ flex: 1 }} />
        <Button
          size="small"
          variant="contained"
          disabled={!name.trim()}
          onClick={() => {
            saveCurrentLayout(name)
            setName('')
          }}
          sx={{ textTransform: 'none' }}
        >
          Save current layout
        </Button>
      </Stack>
      <Row label="At launch">
        <Select size="small" value={startup} onChange={(e) => setStartupLayout(String(e.target.value))} sx={{ minWidth: 220 }}>
          <MenuItem value="last">Where I left off</MenuItem>
          {(Object.keys(PRESET_LABELS) as Preset[]).map((p) => (
            <MenuItem key={p} value={p}>
              {PRESET_LABELS[p]}
            </MenuItem>
          ))}
          {saved.map((l) => (
            <MenuItem key={l.id} value={`saved:${l.id}`}>
              {l.name}
            </MenuItem>
          ))}
        </Select>
      </Row>
      <Box>
        <Button size="small" color="inherit" onClick={() => apply(resetLayout)} sx={{ textTransform: 'none', color: 'text.secondary' }}>
          Reset the arrangement
        </Button>
      </Box>

      <Divider />
      <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
        Window
      </Typography>
      <Row label="Sidebar side">
        <Segmented<'left' | 'right'>
          value={sidebarSide}
          options={[
            ['left', 'Left'],
            ['right', 'Right'],
          ]}
          onChange={(side) => updatePrefs({ sidebarSide: side })}
        />
      </Row>
      <Row label="Project rail" hint="The column of project icons">
        <Switch size="small" checked={showRail} onChange={() => updatePrefs({ showRail: !showRail })} />
      </Row>

      <Divider />
      <Stack direction="row" sx={{ alignItems: 'center' }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 700, flex: 1 }}>
          Shortcuts
        </Typography>
        <Button size="small" color="inherit" disabled={Object.keys(overrides).length === 0} onClick={resetAll} sx={{ textTransform: 'none', color: 'text.secondary' }}>
          Reset all
        </Button>
      </Stack>
      <Typography variant="caption" color="text.secondary">
        Click a shortcut and press the new keys. In a terminal or the code editor, only shortcuts with Shift or Alt reach the app,
        so their own keys keep working.
      </Typography>
      {INSTANT_ENABLED && hasTauri() && <InstantChordRow />}
      {groups.map(([group, actions]) => (
        <Stack key={group} sx={{ gap: 0.25 }}>
          <Typography variant="caption" sx={{ fontWeight: 600, color: 'text.secondary', textTransform: 'uppercase', letterSpacing: 0.4, mt: 0.5 }}>
            {group}
          </Typography>
          {actions.map((a) => (
            <ShortcutRow key={a.id} action={a} />
          ))}
        </Stack>
      ))}

      {INSTANT_ENABLED && hasTauri() && (
        <>
          <Divider />
          <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
            Notch
          </Typography>
          <NotchRows />
        </>
      )}
    </Stack>
  )
}
