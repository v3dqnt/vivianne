import { useEffect, useState, type ReactNode } from 'react'
import { Stack, Typography, Divider, Box, ButtonBase, Button, Slider, ToggleButtonGroup, ToggleButton, IconButton, Tooltip, Alert, Switch } from '../../../ui'
import { AddLinear, EditLinear, TrashLinear, ExportLinear, ImportLinear, CloseLinear } from '../../../icons'
import { call, hasTauri } from '../../../ipc'
import { allPalettes, FONTS, onColor, type FontChoice, type PaletteDef } from '../../../theme'
import { useThemeStore, type ChatWidth, type Density, type ModePref } from '../store/themeStore'
import { ThemeBuilder } from './ThemeBuilder'
import { FaceEditor } from '../../team/components/FaceEditor'
import { useFacePrefs } from '../../team/store/faceStore'
import { useReadAloudPref } from '../../ai/lib/readAloud'
import { SUBAGENT_COLOR_HEX } from '../lib/subagentProfiles'

const ACCENTS = ['#4a9eff', '#7c5cff', '#d97757', '#e5484d', '#f5a524', '#30a46c', '#12a594', '#e93d82']

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
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

function ThemeCard({
  p,
  active,
  custom,
  onPick,
  onEdit,
  onDelete,
  onExport,
}: {
  p: PaletteDef
  active: boolean
  custom: boolean
  onPick: () => void
  onEdit: () => void
  onDelete: () => void
  onExport: () => void
}) {
  const [confirm, setConfirm] = useState(false)
  return (
    <Box
      sx={{
        position: 'relative',
        borderRadius: 2,
        border: 1,
        borderColor: active ? 'primary.main' : 'divider',
        bgcolor: active ? 'action.selected' : 'transparent',
        '&:hover .theme-actions': { opacity: 1 },
      }}
    >
      <ButtonBase onClick={onPick} sx={{ display: 'flex', alignItems: 'center', gap: 1.5, p: 1.25, width: '100%', justifyContent: 'flex-start', borderRadius: 2 }}>
        <Box sx={{ display: 'flex', flexShrink: 0, pl: 0.5 }}>
          {p.swatches.slice(0, 4).map((c, i) => (
            <Box key={`${c}${i}`} sx={{ width: 18, height: 18, borderRadius: '50%', bgcolor: c, border: 1, borderColor: 'rgba(128,128,128,0.4)', mx: -0.35 }} />
          ))}
        </Box>
        <Stack sx={{ textAlign: 'left', minWidth: 0 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }} noWrap>
            {p.symbol} {p.label}
          </Typography>
          <Typography variant="caption" color="text.secondary" noWrap>
            {active ? 'In use' : custom ? 'Yours' : 'Built in'}
          </Typography>
        </Stack>
      </ButtonBase>
      <Stack direction="row" className="theme-actions" sx={{ position: 'absolute', right: 6, top: 6, opacity: 0, transition: 'opacity 120ms', bgcolor: 'background.paper', borderRadius: 1 }}>
        <Tooltip title={custom ? 'Edit' : 'Make a copy to edit'}>
          <IconButton size="small" onClick={onEdit}>
            <EditLinear size={13} color="currentColor" />
          </IconButton>
        </Tooltip>
        {custom && (
          <Tooltip title="Export to a file">
            <IconButton size="small" onClick={onExport}>
              <ExportLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        )}
        {custom && (
          <Tooltip title={confirm ? 'Click again to delete' : 'Delete'}>
            <IconButton size="small" color={confirm ? 'error' : 'default'} onBlur={() => setConfirm(false)} onClick={() => (confirm ? onDelete() : setConfirm(true))}>
              <TrashLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        )}
      </Stack>
    </Box>
  )
}

/** Settings → Appearance: mode, themes (with the builder), accent, type,
 * size, chat density and width, wallpaper and glass. */
export function ThemesSection() {
  const s = useThemeStore()
  const [builder, setBuilder] = useState<{ base: PaletteDef; editing: boolean } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const palettes = allPalettes(s.customThemes)
  const isCustom = (id: string) => s.customThemes.some((t) => t.id === id)

  const exportTheme = async (p: PaletteDef) => {
    const { save } = await import('@tauri-apps/plugin-dialog')
    const path = await save({ defaultPath: `${p.id}.vivtheme.json`, filters: [{ name: 'Vivianne theme', extensions: ['json'] }] })
    if (path) await call('cmd_themes_write_file', { path, theme: p }).catch((e) => setError(String(e)))
  }

  const importTheme = async () => {
    setError(null)
    const { open } = await import('@tauri-apps/plugin-dialog')
    const path = await open({ multiple: false, filters: [{ name: 'Vivianne theme', extensions: ['json'] }] })
    if (typeof path !== 'string') return
    try {
      const theme = await call<PaletteDef>('cmd_themes_read_file', { path })
      if (!theme) return
      // Never overwrite a built-in or one of yours: a clash gets a new id.
      let id = theme.id
      for (let n = 2; palettes.some((p) => p.id === id); n++) id = `${theme.id}-${n}`.slice(0, 40)
      await s.saveTheme({ ...theme, id, swatches: theme.swatches ?? [] })
      s.setPalette(id)
    } catch (e) {
      setError(String(e))
    }
  }

  const chooseWallpaper = async () => {
    setError(null)
    const { open } = await import('@tauri-apps/plugin-dialog')
    const path = await open({ multiple: false, filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'] }] })
    if (typeof path === 'string') await s.setWallpaper(path).catch((e) => setError(String(e)))
  }

  return (
    <Stack sx={{ gap: 1.5 }}>
      <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
        Appearance
      </Typography>

      <Row label="Mode" hint={s.modePref === 'system' ? `Following your system (now ${s.mode})` : undefined}>
        <Segmented<ModePref>
          value={s.modePref}
          options={[
            ['light', 'Light'],
            ['dark', 'Dark'],
            ['system', 'System'],
          ]}
          onChange={(v) => s.setMode(v)}
        />
      </Row>

      <Stack direction="row" sx={{ alignItems: 'center', mt: 0.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 500, flex: 1 }}>
          Theme
        </Typography>
        {hasTauri() && (
          <Button size="small" startIcon={<ImportLinear size={14} color="currentColor" />} onClick={() => void importTheme()} sx={{ textTransform: 'none' }}>
            Import
          </Button>
        )}
        <Button
          size="small"
          startIcon={<AddLinear size={14} color="currentColor" />}
          onClick={() => setBuilder({ base: palettes.find((p) => p.id === s.paletteId) ?? palettes[0], editing: false })}
          sx={{ textTransform: 'none' }}
        >
          New theme
        </Button>
      </Stack>
      <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(190px, 1fr))', gap: 1 }}>
        {palettes.map((p) => (
          <ThemeCard
            key={p.id}
            p={p}
            active={p.id === s.paletteId}
            custom={isCustom(p.id)}
            onPick={() => s.setPalette(p.id)}
            onEdit={() => setBuilder({ base: p, editing: isCustom(p.id) })}
            onDelete={() => void s.deleteTheme(p.id)}
            onExport={() => void exportTheme(p)}
          />
        ))}
      </Box>

      <Row label="Accent colour" hint={s.accent ? 'Overrides the theme’s own accent' : 'The theme’s own'}>
        {ACCENTS.map((c) => (
          <ButtonBase
            key={c}
            aria-label={`Accent ${c}`}
            onClick={() => s.update({ accent: c })}
            sx={{
              width: 18,
              height: 18,
              borderRadius: '50%',
              bgcolor: c,
              outline: s.accent === c ? '2px solid' : 'none',
              outlineColor: 'text.primary',
              outlineOffset: 2,
            }}
          />
        ))}
        <Box
          component="input"
          type="color"
          aria-label="Any colour"
          value={s.accent ?? '#4a9eff'}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) => s.update({ accent: e.target.value })}
          sx={{ width: 26, height: 22, p: 0, border: 1, borderColor: 'divider', borderRadius: 1, bgcolor: 'transparent', cursor: 'pointer' }}
        />
        {s.accent && (
          <Tooltip title="Back to the theme's accent">
            <IconButton size="small" onClick={() => s.update({ accent: null })}>
              <CloseLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        )}
      </Row>
      {s.accent && (
        <Typography variant="caption" color="text.secondary" sx={{ mt: -1 }}>
          Text on it is {onColor(s.accent) === '#ffffff' ? 'white' : 'black'}, whichever reads better.
        </Typography>
      )}

      <Divider />

      <Row label="Font">
        <Segmented<FontChoice>
          value={s.font}
          options={(Object.keys(FONTS) as FontChoice[]).map((f) => [f, FONTS[f].label])}
          onChange={(font) => s.update({ font })}
        />
      </Row>
      <Row label="Size" hint={`${Math.round(s.uiScale * 100)}%, everything in the window`}>
        <Box sx={{ width: 180 }}>
          <Slider value={s.uiScale} min={0.9} max={1.25} step={0.05} aria-label="Size" onChange={(_e, v) => s.update({ uiScale: v as number })} />
        </Box>
      </Row>
      <Row label="Corners" hint={`${s.radius}px`}>
        <Box sx={{ width: 180 }}>
          <Slider value={s.radius} min={2} max={16} step={1} aria-label="Corner radius" onChange={(_e, v) => s.update({ radius: v as number })} />
        </Box>
      </Row>

      <Divider />

      <Row label="Chat density">
        <Segmented<Density>
          value={s.density}
          options={[
            ['compact', 'Compact'],
            ['comfortable', 'Comfortable'],
            ['roomy', 'Roomy'],
          ]}
          onChange={(density) => s.update({ density })}
        />
      </Row>
      <Row label="Chat width">
        <Segmented<ChatWidth>
          value={s.chatWidth}
          options={[
            ['narrow', 'Narrow'],
            ['normal', 'Normal'],
            ['wide', 'Wide'],
            ['full', 'Full'],
          ]}
          onChange={(chatWidth) => s.update({ chatWidth })}
        />
      </Row>
      <VisualsRow />
      <ReadAloudRow />

      <Divider />

      <Row label="Wallpaper" hint={s.wallpaper ? 'Shows behind the panes' : 'An image behind the window'}>
        {hasTauri() && (
          <Button size="small" variant="outlined" onClick={() => void chooseWallpaper()} sx={{ textTransform: 'none' }}>
            {s.wallpaper ? 'Change' : 'Choose image'}
          </Button>
        )}
        {s.wallpaper && (
          <Button size="small" color="inherit" onClick={() => void s.setWallpaper(null)} sx={{ textTransform: 'none', color: 'text.secondary' }}>
            Remove
          </Button>
        )}
      </Row>
      {s.wallpaper && (
        <>
          <Row label="Glass" hint="How much of the wallpaper shows through, blurred">
            <Box sx={{ width: 180 }}>
              <Slider value={s.glass} min={0} max={1} step={0.05} aria-label="Glass" onChange={(_e, v) => s.update({ glass: v as number })} />
            </Box>
          </Row>
          <Row label="Dim" hint="Darkens the wallpaper so text stays readable">
            <Box sx={{ width: 180 }}>
              <Slider value={s.wallpaperDim} min={0} max={0.8} step={0.05} aria-label="Dim" onChange={(_e, v) => s.update({ wallpaperDim: v as number })} />
            </Box>
          </Row>
        </>
      )}

      <Divider />
      <RobotHeads />

      {error && <Alert severity="error">{error}</Alert>}

      <Box>
        <Button size="small" color="inherit" onClick={() => s.reset()} sx={{ textTransform: 'none', color: 'text.secondary' }}>
          Reset appearance
        </Button>
      </Box>

      {builder && <ThemeBuilder base={builder.base} editing={builder.editing} onClose={() => setBuilder(null)} />}
    </Stack>
  )
}

/** Every agent's robot head: size, when they move, and Vivianne's own. */
function RobotHeads() {
  const f = useFacePrefs()
  return (
    <Stack sx={{ gap: 0.5 }}>
      <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
        Robot heads
      </Typography>
      <Row label="Size" hint="Every agent's head, everywhere">
        <Box sx={{ width: 180 }}>
          <Slider value={f.scale} min={0.85} max={1.25} step={0.05} aria-label="Head size" onChange={(_e, v) => f.update({ scale: v as number })} />
        </Box>
      </Row>
      <Row label="Movement" hint="Busy: heads move only while their agent works">
        <Segmented
          value={f.motion}
          options={[
            ['busy', 'When busy'],
            ['always', 'Always'],
            ['never', 'Never'],
          ]}
          onChange={(motion) => f.update({ motion })}
        />
      </Row>
      <Typography variant="body2" sx={{ fontWeight: 500, mt: 1, mb: 0.5 }}>
        Vivianne
      </Typography>
      <FaceEditor
        name="Vivianne"
        color={f.viv.color ?? '#5b5b66'}
        face={f.viv}
        onChange={({ color, face }) => f.update({ viv: { ...(face ?? {}), color: color.startsWith('#') ? color : SUBAGENT_COLOR_HEX[color as keyof typeof SUBAGENT_COLOR_HEX] } })}
      />
    </Stack>
  )
}

/** How much live UI (calculators, charts, mini-apps) Vivianne puts in her
 * replies. Read by the backend into her instructions each turn. */
function VisualsRow() {
  const [v, setV] = useState<'fewer' | 'auto' | 'more'>('auto')
  useEffect(() => {
    void call<Record<string, unknown>>('cmd_get_settings').then((st) => {
      const x = st?.chatVisuals
      if (x === 'fewer' || x === 'more') setV(x)
    })
  }, [])
  const set = async (next: 'fewer' | 'auto' | 'more') => {
    setV(next)
    const st = (await call<Record<string, unknown>>('cmd_get_settings')) ?? {}
    await call('cmd_save_settings', { settings: { ...st, chatVisuals: next } })
  }
  return (
    <Row label="Visuals in answers" hint="Charts, calculators and small interactive apps inside replies">
      <Segmented
        value={v}
        options={[
          ['fewer', 'Fewer'],
          ['auto', 'Auto'],
          ['more', 'More'],
        ]}
        onChange={(n) => void set(n)}
      />
    </Row>
  )
}

/** Whether a reply to a spoken prompt is read out when it's done. */
function ReadAloudRow() {
  const on = useReadAloudPref((s) => s.whenDictated)
  const set = useReadAloudPref((s) => s.set)
  return (
    <Row label="Read replies aloud after I dictate" hint="Speak the answer when the prompt was spoken with the mic. Every reply also has its own read button">
      <Switch size="small" checked={on} onChange={(e) => set(e.target.checked)} />
    </Row>
  )
}
