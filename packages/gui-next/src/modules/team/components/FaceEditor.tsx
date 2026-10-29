import { useState } from 'react'
import { Box, ButtonBase, Stack, Typography } from '../../../ui'
import { SUBAGENT_COLORS, SUBAGENT_COLOR_HEX } from '../../settings/lib/subagentProfiles'
import { AgentFace, shapeFor, type FaceState } from './AgentFace'
import type { FaceStyle } from '../store/faceStore'

const SHAPES: NonNullable<FaceStyle['shape']>[] = ['rectangle', 'square', 'circle', 'hexagon']
const TRIMS = ['#c7ccd6', '#8b93a7', '#3a3f4b', '#f5d08a', '#e7a6b5', '#9fe0c9']
const SCREENS = ['#eef6ff', '#7cf3ff', '#9dff8a', '#ffd166', '#ff8fab', '#c4a7ff']
export const MOODS: FaceState[] = ['idle', 'thinking', 'searching', 'working', 'listening', 'speaking', 'happy', 'error']

/** The shell colour as hex, from a swatch name or a hex. */
export function shellHex(color: string): string {
  return color.startsWith('#') ? color : (SUBAGENT_COLOR_HEX[color as (typeof SUBAGENT_COLORS)[number]] ?? SUBAGENT_COLOR_HEX.blue)
}

const pick = <T,>(list: readonly T[]) => list[Math.floor(Math.random() * list.length)]

/** A whole new look, at random. */
export function randomLook(): { color: string; face: FaceStyle } {
  return { color: pick(SUBAGENT_COLORS), face: { shape: pick(SHAPES), trim: pick(TRIMS), screen: pick(SCREENS) } }
}

const SWATCH = 22
const ring = (on: boolean) => ({ outline: on ? '2px solid' : 'none', outlineColor: 'text.primary', outlineOffset: '2px' }) as const

function Swatch({ color, on, onClick, label }: { color: string; on: boolean; onClick: () => void; label: string }) {
  return (
    <ButtonBase
      aria-label={label}
      aria-pressed={on}
      onClick={onClick}
      sx={{ width: SWATCH, height: SWATCH, borderRadius: '50%', bgcolor: color, boxShadow: 'inset 0 0 0 1px rgba(255,255,255,0.12)', flexShrink: 0, ...ring(on), transition: 'transform 0.12s', '&:hover': { transform: 'scale(1.1)' } }}
    />
  )
}

/** Any colour: the system picker behind a swatch. */
function CustomSwatch({ value, on, onChange, label }: { value: string; on: boolean; onChange: (c: string) => void; label: string }) {
  return (
    <Box
      component="label"
      title={`${label}: any colour`}
      sx={{
        position: 'relative',
        width: SWATCH,
        height: SWATCH,
        borderRadius: '50%',
        cursor: 'pointer',
        flexShrink: 0,
        background: on ? value : 'conic-gradient(#f87171, #fbbf24, #4ade80, #22d3ee, #818cf8, #f472b6, #f87171)',
        ...ring(on),
      }}
    >
      <input type="color" value={value} onChange={(e) => onChange(e.target.value)} style={{ opacity: 0, position: 'absolute', inset: 0, width: '100%', height: '100%', cursor: 'pointer' }} />
    </Box>
  )
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: '12px', minHeight: 32 }}>
      <Typography sx={{ fontSize: '12px', color: 'text.secondary', width: 52, flexShrink: 0 }}>{label}</Typography>
      <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', flexWrap: 'wrap' }}>
        {children}
      </Stack>
    </Stack>
  )
}

/** Auto (the head works it out) or a preset or any colour. */
function ColorRow({ label, value, presets, onChange }: { label: string; value: string | undefined; presets: string[]; onChange: (c: string | undefined) => void }) {
  const custom = !!value && !presets.includes(value)
  return (
    <Row label={label}>
      <ButtonBase
        aria-label={`${label}: automatic`}
        aria-pressed={!value}
        onClick={() => onChange(undefined)}
        sx={{ height: SWATCH, px: '8px', borderRadius: '999px', fontSize: '11px', fontWeight: 500, color: value ? 'text.secondary' : 'text.primary', bgcolor: value ? 'transparent' : 'action.selected', boxShadow: (t) => `inset 0 0 0 1px ${t.palette.divider}` }}
      >
        Auto
      </ButtonBase>
      {presets.map((c) => (
        <Swatch key={c} color={c} on={value === c} onClick={() => onChange(c)} label={`${label} ${c}`} />
      ))}
      <CustomSwatch value={value ?? '#888888'} on={custom} onChange={onChange} label={label} />
    </Row>
  )
}

/** The head itself, large, in the mood picked below it. */
export function FaceStage({ name, color, face, size = 128 }: { name: string; color: string; face: FaceStyle | null | undefined; size?: number }) {
  const [mood, setMood] = useState<FaceState>('happy')
  return (
    <Stack sx={{ alignItems: 'center', gap: '12px' }}>
      <Box sx={{ width: size + 24, height: size + 24, display: 'grid', placeItems: 'center' }}>
        <AgentFace name={name || '?'} color={shellHex(color)} face={face} size={size} state={mood} animate interactive />
      </Box>
      <Stack direction="row" sx={{ gap: '4px', flexWrap: 'wrap', justifyContent: 'center', maxWidth: 220 }}>
        {MOODS.map((s) => (
          <ButtonBase
            key={s}
            onClick={() => setMood(s)}
            aria-pressed={mood === s}
            sx={{ px: '8px', height: 22, borderRadius: '999px', fontSize: '11px', textTransform: 'capitalize', color: mood === s ? 'text.primary' : 'text.secondary', bgcolor: mood === s ? 'action.selected' : 'transparent', '&:hover': { color: 'text.primary' } }}
          >
            {s}
          </ButtonBase>
        ))}
      </Stack>
    </Stack>
  )
}

/** Shape, shell, trim and lights. */
export function FaceControls({
  name,
  color,
  face,
  onChange,
}: {
  name: string
  color: string
  face: FaceStyle | null | undefined
  onChange: (next: { color: string; face: FaceStyle | null }) => void
}) {
  const hex = shellHex(color)
  const f = face ?? {}
  const set = (patch: Partial<FaceStyle>, nextColor = color) => {
    const merged = { ...f, ...patch }
    const clean = Object.fromEntries(Object.entries(merged).filter(([, v]) => v)) as FaceStyle
    onChange({ color: nextColor, face: Object.keys(clean).length ? clean : null })
  }
  return (
    <Stack sx={{ gap: '6px' }}>
      <Row label="Shape">
        {SHAPES.map((s) => {
          const on = (f.shape ?? shapeFor(name || '?')) === s
          return (
            <ButtonBase
              key={s}
              aria-label={s}
              aria-pressed={on}
              onClick={() => set({ shape: s })}
              sx={{ width: 40, height: 40, borderRadius: '10px', bgcolor: on ? 'action.selected' : 'action.hover', boxShadow: on ? (t) => `inset 0 0 0 1.5px ${t.palette.text.primary}` : 'none', '&:hover': { bgcolor: 'action.selected' } }}
            >
              <AgentFace name={name || '?'} color={hex} face={{ ...f, shape: s }} size={26} />
            </ButtonBase>
          )
        })}
      </Row>
      <Row label="Shell">
        {SUBAGENT_COLORS.map((c) => (
          <Swatch key={c} color={SUBAGENT_COLOR_HEX[c]} on={color === c} onClick={() => set({}, c)} label={c} />
        ))}
        <CustomSwatch value={hex} on={color.startsWith('#')} onChange={(c) => set({}, c)} label="Shell" />
      </Row>
      <ColorRow label="Trim" value={f.trim} presets={TRIMS} onChange={(trim) => set({ trim })} />
      <ColorRow label="Lights" value={f.screen} presets={SCREENS} onChange={(screen) => set({ screen })} />
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
  return (
    <Stack direction="row" sx={{ gap: '20px', alignItems: 'flex-start', flexWrap: 'wrap' }}>
      <FaceStage name={name} color={color} face={face} size={80} />
      <Stack sx={{ gap: '12px', flex: 1, minWidth: 240 }}>
        <FaceControls name={name} color={color} face={face} onChange={onChange} />
        <Stack direction="row" sx={{ gap: '8px', pl: '64px' }}>
          <ButtonBase onClick={() => onChange(randomLook())} sx={{ height: 28, px: '12px', borderRadius: '8px', fontSize: '12px', fontWeight: 500, bgcolor: 'action.hover', '&:hover': { bgcolor: 'action.selected' } }}>
            Randomize
          </ButtonBase>
          <ButtonBase onClick={() => onChange({ color, face: null })} sx={{ height: 28, px: '12px', borderRadius: '8px', fontSize: '12px', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover' } }}>
            Reset
          </ButtonBase>
        </Stack>
      </Stack>
    </Stack>
  )
}
