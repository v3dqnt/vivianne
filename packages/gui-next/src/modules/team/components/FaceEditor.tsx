import { useState } from 'react'
import { Box, ButtonBase, Stack, Typography } from '../../../ui'
import { SUBAGENT_COLORS, SUBAGENT_COLOR_HEX } from '../../settings/lib/subagentProfiles'
import { AgentFace, shapeFor, type FaceState } from './AgentFace'
import type { FaceStyle } from '../store/faceStore'

const SHAPES: NonNullable<FaceStyle['shape']>[] = ['rectangle', 'square', 'circle', 'hexagon']
const TRIMS = ['#c7ccd6', '#8b93a7', '#3a3f4b', '#f5d08a', '#e7a6b5', '#9fe0c9']
const SCREENS = ['#eef6ff', '#7cf3ff', '#9dff8a', '#ffd166', '#ff8fab', '#c4a7ff']
const PREVIEW: FaceState[] = ['idle', 'thinking', 'searching', 'working', 'listening', 'speaking', 'happy', 'error']

function Swatch({ color, on, onClick, label }: { color: string; on: boolean; onClick: () => void; label: string }) {
  return (
    <ButtonBase
      aria-label={label}
      aria-pressed={on}
      onClick={onClick}
      sx={{ width: 20, height: 20, borderRadius: '50%', bgcolor: color, border: 1, borderColor: 'divider', outline: on ? '2px solid' : 'none', outlineColor: 'text.primary', outlineOffset: 2, flexShrink: 0 }}
    />
  )
}

/** A colour row: preset swatches and a picker for anything else. */
function ColorRow({ label, value, presets, onChange }: { label: string; value: string | undefined; presets: string[]; onChange: (c: string | undefined) => void }) {
  const custom = value && !presets.includes(value)
  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, flexWrap: 'wrap' }}>
      <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary', width: 56, flexShrink: 0 }}>{label}</Typography>
      <ButtonBase
        aria-label={`${label}: default`}
        onClick={() => onChange(undefined)}
        sx={{ height: 20, px: 0.75, borderRadius: '999px', fontSize: '0.6875rem', border: 1, borderColor: value ? 'divider' : 'text.primary', color: 'text.secondary' }}
      >
        Auto
      </ButtonBase>
      {presets.map((c) => (
        <Swatch key={c} color={c} on={value === c} onClick={() => onChange(c)} label={`${label} ${c}`} />
      ))}
      <Box
        component="label"
        title="Any colour"
        sx={{
          position: 'relative',
          width: 20,
          height: 20,
          borderRadius: '50%',
          cursor: 'pointer',
          background: custom ? value : 'conic-gradient(red, yellow, lime, cyan, blue, magenta, red)',
          outline: custom ? '2px solid' : 'none',
          outlineColor: 'text.primary',
          outlineOffset: 2,
          flexShrink: 0,
        }}
      >
        <input type="color" value={value ?? '#888888'} onChange={(e) => onChange(e.target.value)} style={{ opacity: 0, position: 'absolute', inset: 0, width: '100%', height: '100%', cursor: 'pointer' }} />
      </Box>
    </Stack>
  )
}

/**
 * How an agent's robot head looks: its outline, shell colour, trim and LED
 * colour, with a live head that can show any of its moods.
 */
export function FaceEditor({
  name,
  color,
  face,
  onChange,
}: {
  name: string
  /** The shell colour: a swatch name or `#hex`. */
  color: string
  face: FaceStyle | null | undefined
  onChange: (next: { color: string; face: FaceStyle | null }) => void
}) {
  const [mood, setMood] = useState<FaceState>('happy')
  const hex = color.startsWith('#') ? color : (SUBAGENT_COLOR_HEX[color as (typeof SUBAGENT_COLORS)[number]] ?? SUBAGENT_COLOR_HEX.blue)
  const f = face ?? {}
  const set = (patch: Partial<FaceStyle>, nextColor = color) => {
    const merged = { ...f, ...patch }
    const clean = Object.fromEntries(Object.entries(merged).filter(([, v]) => v)) as FaceStyle
    onChange({ color: nextColor, face: Object.keys(clean).length ? clean : null })
  }
  const pick = <T,>(list: T[]) => list[Math.floor(Math.random() * list.length)]

  return (
    <Stack direction="row" sx={{ gap: 2, alignItems: 'flex-start', flexWrap: 'wrap' }}>
      <Stack sx={{ alignItems: 'center', gap: 0.75, width: 120, flexShrink: 0 }}>
        <Box sx={{ width: 96, height: 96, display: 'grid', placeItems: 'center' }}>
          <AgentFace name={name || '?'} color={hex} face={face} size={80} state={mood} animate interactive />
        </Box>
        <Stack direction="row" sx={{ gap: 0.375, flexWrap: 'wrap', justifyContent: 'center' }}>
          {PREVIEW.map((s) => (
            <ButtonBase
              key={s}
              onClick={() => setMood(s)}
              sx={{ px: 0.625, height: 18, borderRadius: '999px', fontSize: '0.625rem', color: mood === s ? 'text.primary' : 'text.disabled', bgcolor: mood === s ? 'action.selected' : 'transparent' }}
            >
              {s}
            </ButtonBase>
          ))}
        </Stack>
      </Stack>
      <Stack sx={{ gap: 1, flex: 1, minWidth: 240 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75 }}>
          <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary', width: 56, flexShrink: 0 }}>Shape</Typography>
          {SHAPES.map((s) => {
            const on = (f.shape ?? shapeFor(name || '?')) === s
            return (
              <ButtonBase
                key={s}
                aria-label={s}
                aria-pressed={on}
                onClick={() => set({ shape: s })}
                sx={{ p: 0.25, borderRadius: '8px', border: 1, borderColor: on ? 'text.primary' : 'divider' }}
              >
                <AgentFace name={name || '?'} color={hex} face={{ ...f, shape: s }} size={24} />
              </ButtonBase>
            )
          })}
        </Stack>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, flexWrap: 'wrap' }}>
          <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary', width: 56, flexShrink: 0 }}>Shell</Typography>
          {SUBAGENT_COLORS.map((c) => (
            <Swatch key={c} color={SUBAGENT_COLOR_HEX[c]} on={color === c} onClick={() => set({}, c)} label={c} />
          ))}
          <Box
            component="label"
            title="Any colour"
            sx={{
              position: 'relative',
              width: 20,
              height: 20,
              borderRadius: '50%',
              cursor: 'pointer',
              background: color.startsWith('#') ? color : 'conic-gradient(red, yellow, lime, cyan, blue, magenta, red)',
              outline: color.startsWith('#') ? '2px solid' : 'none',
              outlineColor: 'text.primary',
              outlineOffset: 2,
            }}
          >
            <input type="color" value={hex} onChange={(e) => set({}, e.target.value)} style={{ opacity: 0, position: 'absolute', inset: 0, width: '100%', height: '100%', cursor: 'pointer' }} />
          </Box>
        </Stack>
        <ColorRow label="Trim" value={f.trim} presets={TRIMS} onChange={(trim) => set({ trim })} />
        <ColorRow label="Lights" value={f.screen} presets={SCREENS} onChange={(screen) => set({ screen })} />
        <Stack direction="row" sx={{ gap: 0.75, pl: '62px' }}>
          <ButtonBase
            onClick={() => onChange({ color: pick([...SUBAGENT_COLORS]), face: { shape: pick(SHAPES), trim: pick(TRIMS), screen: pick(SCREENS) } })}
            sx={{ height: 24, px: 1, borderRadius: '8px', fontSize: '0.75rem', border: 1, borderColor: 'divider', '&:hover': { bgcolor: 'action.hover' } }}
          >
            Randomize
          </ButtonBase>
          <ButtonBase
            onClick={() => onChange({ color, face: null })}
            sx={{ height: 24, px: 1, borderRadius: '8px', fontSize: '0.75rem', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover' } }}
          >
            Reset
          </ButtonBase>
        </Stack>
      </Stack>
    </Stack>
  )
}
