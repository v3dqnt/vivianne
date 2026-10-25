import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { Alert, Box, ButtonBase, InputBase, Stack, Switch, Slider, Typography, alpha, useTheme } from '../../../ui'
import { ChartBlock } from './ChartBlock'
import { evaluate, interpolate, type Scope } from '../lib/uiExpr'
import { loadPersisted, savePersisted } from '../../../lib/persist'
import { useAgentStore } from '../store/agentStore'

/**
 * Interactive answers: a ```ui fence holds a small JSON spec that renders as
 * live, native UI inside the reply — inputs, numbers that recompute as you
 * move them, charts drawn from formulas, sortable tables, and buttons that
 * send a follow-up. The format is in vivianne-guides/interactive-answers.
 */

/** Where a button's follow-up goes. The main chat by default; a DM or room
 * can provide its own. */
export const FollowUpContext = createContext<((text: string) => void) | null>(null)

type Fmt = 'number' | 'currency' | 'percent' | 'integer' | 'text'
interface InputSpec {
  id: string
  type: 'slider' | 'number' | 'select' | 'toggle' | 'text'
  label?: string
  min?: number
  max?: number
  step?: number
  value?: number | string | boolean
  options?: (string | { label: string; value: string | number })[]
  format?: Fmt
  unit?: string
}
type Block =
  | { type: 'text'; text: string }
  | { type: 'stats'; items: { label: string; value: string | number; format?: Fmt; unit?: string; hint?: string }[] }
  | { type: 'inputs'; ids?: string[] }
  | {
      type: 'chart'
      chart?: 'line' | 'bar' | 'area' | 'scatter' | 'pie'
      title?: string
      xLabel?: string
      yLabel?: string
      /** x from..to by step (expressions); each series' y is an expression in `x`. */
      x?: { from: string | number; to: string | number; step?: string | number }
      series?: { name: string; y: string }[]
      /** Or fixed data, as ChartBlock takes it. */
      data?: { x: number | string; y: number }[]
    }
  | { type: 'table'; columns: string[]; rows: (string | number)[][] }
  | { type: 'buttons'; items: { label: string; send?: string; set?: Record<string, string | number | boolean>; primary?: boolean }[] }
  | { type: 'callout'; tone?: 'info' | 'warning' | 'success' | 'error'; text: string }
  | { type: 'tabs'; tabs: { label: string; blocks: Block[] }[] }
  | { type: 'list'; items: string[]; ordered?: boolean }
  // Summary blocks: for reporting what was found or done.
  | { type: 'heading'; text: string }
  | { type: 'steps'; items: { title: string; text?: string; state?: 'done' | 'active' | 'todo' | 'error' }[] }
  | { type: 'cards'; items: { title: string; text?: string; badge?: string; tone?: Tone; icon?: string }[] }
  | { type: 'kv'; items: { label: string; value: string | number }[] }
  | { type: 'progress'; label?: string; value: string | number; hint?: string }
  | { type: 'badges'; items: { label: string; tone?: Tone }[] }

type Tone = 'info' | 'success' | 'warning' | 'error' | 'neutral'

interface Spec {
  /** A status chip beside the title: "Done", "Needs review"... */
  status?: { label: string; tone?: Tone }
  /** One emoji or symbol shown before the title. */
  icon?: string
  title?: string
  description?: string
  inputs?: InputSpec[]
  /** Named values, in order; each can use the inputs and those before it. */
  computed?: Record<string, string>
  blocks?: Block[]
}

function parse(code: string): Spec | string {
  try {
    const raw = JSON.parse(code)
    if (!raw || typeof raw !== 'object') return 'Expected a JSON object.'
    return raw as Spec
  } catch (e) {
    return `Not valid JSON: ${e instanceof Error ? e.message : String(e)}`
  }
}

function format(v: unknown, fmt: Fmt = 'number', unit?: string): string {
  if (typeof v === 'boolean') return v ? 'Yes' : 'No'
  if (typeof v === 'string' || fmt === 'text') return `${v}${unit ? ` ${unit}` : ''}`
  const n = Number(v)
  if (!Number.isFinite(n)) return '—'
  const s =
    fmt === 'currency'
      ? n.toLocaleString(undefined, { style: 'currency', currency: 'USD', maximumFractionDigits: Math.abs(n) >= 100 ? 0 : 2 })
      : fmt === 'percent'
        ? `${(n * 100).toLocaleString(undefined, { maximumFractionDigits: 1 })}%`
        : fmt === 'integer'
          ? Math.round(n).toLocaleString()
          : n.toLocaleString(undefined, { maximumFractionDigits: Math.abs(n) >= 100 ? 0 : 2 })
  return unit ? `${s} ${unit}` : s
}

/** Answers keep their inputs across reloads, keyed by the spec itself. */
function storageKey(code: string) {
  let h = 0
  for (let i = 0; i < code.length; i++) h = (h * 31 + code.charCodeAt(i)) | 0
  return `viv.ui.${h}`
}

export function InteractiveBlock({ code }: { code: string }) {
  const theme = useTheme()
  const spec = useMemo(() => parse(code), [code])
  const key = storageKey(code)
  const [values, setValues] = useState<Scope>(() => {
    const init: Scope = {}
    if (typeof spec === 'object') for (const i of spec.inputs ?? []) init[i.id] = i.value ?? (i.type === 'toggle' ? false : i.type === 'text' || i.type === 'select' ? '' : (i.min ?? 0))
    return loadPersisted<Scope>(key, init)
  })
  // What the user set survives a reload; written once they stop moving it.
  useEffect(() => {
    const t = window.setTimeout(() => savePersisted(key, values), 300)
    return () => window.clearTimeout(t)
  }, [key, values])
  const followUp = useContext(FollowUpContext)
  const toneColor = useTone()
  const sendMain = useAgentStore((s) => s.sendMessage)

  if (typeof spec === 'string') {
    const arriving = !/[}\]]\s*$/.test(code.trim())
    if (arriving) return <Typography sx={{ my: 1, fontSize: '0.8125rem', color: 'text.secondary' }}>Putting this together{'\u2026'}</Typography>
    return <Alert severity="warning" sx={{ my: 1, fontSize: '0.8125rem' }}>This interactive answer couldn't be shown. {spec}</Alert>
  }

  const set = (patch: Scope) => setValues((v) => ({ ...v, ...patch }))

  // Inputs, then computed values in order, each seeing the ones before.
  const scope: Scope = { ...values }
  const errors: string[] = []
  for (const [name, expr] of Object.entries(spec.computed ?? {})) {
    try {
      scope[name] = evaluate(expr, scope)
    } catch (e) {
      errors.push(`${name}: ${e instanceof Error ? e.message : String(e)}`)
      scope[name] = NaN
    }
  }
  const val = (x: string | number | undefined) => {
    if (typeof x === 'number' || x === undefined) return x
    try {
      return evaluate(x, scope)
    } catch {
      return x
    }
  }
  const text = (t: string) => interpolate(t, scope, (v) => format(v))
  const send = (t: string) => (followUp ?? ((m: string) => void sendMain(m, [])))(text(t))

  const renderInput = (i: InputSpec) => {
    const v = values[i.id]
    const label = (
      <Stack direction="row" sx={{ justifyContent: 'space-between', gap: 1 }}>
        <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>{i.label ?? i.id}</Typography>
        {(i.type === 'slider' || i.type === 'number') && <Typography sx={{ fontSize: '0.8125rem', fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}>{format(v, i.format, i.unit)}</Typography>}
      </Stack>
    )
    if (i.type === 'slider')
      return (
        <Stack key={i.id} sx={{ gap: 0.25 }}>
          {label}
          <Slider value={Number(v)} min={i.min ?? 0} max={i.max ?? 100} step={i.step ?? 1} aria-label={i.label ?? i.id} onChange={(_e, n) => set({ [i.id]: n as number })} />
        </Stack>
      )
    if (i.type === 'number' || i.type === 'text')
      return (
        <Stack key={i.id} sx={{ gap: 0.5 }}>
          {i.type === 'text' ? <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>{i.label ?? i.id}</Typography> : label}
          <InputBase
            value={String(v ?? '')}
            type={i.type === 'number' ? 'number' : 'text'}
            onChange={(e) => set({ [i.id]: i.type === 'number' ? Number(e.target.value) : e.target.value })}
            sx={{ fontSize: '0.875rem', px: 1.25, py: 0.5, borderRadius: '8px', bgcolor: alpha(theme.palette.text.primary, 0.05) }}
          />
        </Stack>
      )
    if (i.type === 'toggle')
      return (
        <Stack key={i.id} direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between' }}>
          <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>{i.label ?? i.id}</Typography>
          <Switch checked={!!v} onChange={(e) => set({ [i.id]: e.target.checked })} />
        </Stack>
      )
    const opts = (i.options ?? []).map((o) => (typeof o === 'string' ? { label: o, value: o } : o))
    return (
      <Stack key={i.id} sx={{ gap: 0.5 }}>
        <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>{i.label ?? i.id}</Typography>
        <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
          {opts.map((o) => {
            const on = v === o.value
            return (
              <ButtonBase
                key={String(o.value)}
                onClick={() => set({ [i.id]: o.value })}
                sx={{ px: 1.25, height: 28, borderRadius: '999px', fontSize: '0.8125rem', border: 1, borderColor: on ? 'primary.main' : 'divider', color: on ? 'primary.main' : 'text.primary', bgcolor: on ? alpha(theme.palette.primary.main, 0.1) : 'transparent' }}
              >
                {o.label}
              </ButtonBase>
            )
          })}
        </Stack>
      </Stack>
    )
  }

  const renderBlock = (b: Block, k: number): ReactNode => {
    switch (b.type) {
      case 'text':
        return (
          <Typography key={k} sx={{ fontSize: '0.875rem', lineHeight: 1.6, whiteSpace: 'pre-wrap' }}>
            {rich(text(b.text))}
          </Typography>
        )
      case 'stats':
        return (
          <Box key={k} sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(130px, 1fr))', gap: 1 }}>
            {b.items.map((s, j) => (
              <Stack key={j} sx={{ p: 1.25, borderRadius: '10px', bgcolor: alpha(theme.palette.text.primary, 0.04), gap: 0.25, minWidth: 0 }}>
                <Typography sx={{ fontSize: '0.6875rem', color: 'text.secondary', textTransform: 'uppercase', letterSpacing: 0.4 }}>{s.label}</Typography>
                <Typography sx={{ fontSize: '1.25rem', fontWeight: 650, fontVariantNumeric: 'tabular-nums', lineHeight: 1.25 }}>{format(val(s.value), s.format, s.unit)}</Typography>
                {s.hint && <Typography sx={{ fontSize: '0.6875rem', color: 'text.disabled' }}>{text(s.hint)}</Typography>}
              </Stack>
            ))}
          </Box>
        )
      case 'inputs': {
        const list = (spec.inputs ?? []).filter((i) => !b.ids || b.ids.includes(i.id))
        return (
          <Box key={k} sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))', gap: 1.5 }}>
            {list.map(renderInput)}
          </Box>
        )
      }
      case 'chart': {
        let chart: object
        if (b.series && b.x) {
          const from = Number(val(b.x.from))
          const to = Number(val(b.x.to))
          const step = Number(val(b.x.step ?? (to - from) / 40)) || 1
          const series = b.series.map((s) => {
            const points: { x: number; y: number }[] = []
            for (let x = from, n = 0; x <= to + 1e-9 && n < 500; x += step, n++) {
              try {
                const y = Number(evaluate(s.y, { ...scope, x }))
                if (Number.isFinite(y)) points.push({ x: Math.round(x * 1e6) / 1e6, y })
              } catch {
                break
              }
            }
            return { name: s.name, points }
          })
          chart = { type: b.chart ?? 'line', title: b.title, xLabel: b.xLabel, yLabel: b.yLabel, series }
        } else {
          chart = { type: b.chart ?? 'bar', title: b.title, xLabel: b.xLabel, yLabel: b.yLabel, data: b.data ?? [] }
        }
        return <ChartBlock key={k} code={JSON.stringify(chart)} />
      }
      case 'table':
        return <SortableTable key={k} columns={b.columns} rows={b.rows.map((r) => r.map((c) => (typeof c === 'string' ? text(c) : c)))} />
      case 'buttons':
        return (
          <Stack key={k} direction="row" sx={{ gap: 0.75, flexWrap: 'wrap' }}>
            {b.items.map((it, j) => (
              <ButtonBase
                key={j}
                onClick={() => {
                  if (it.set) set(Object.fromEntries(Object.entries(it.set).map(([id, x]) => [id, typeof x === 'string' ? (val(x) as Scope[string]) : x])))
                  if (it.send) send(it.send)
                }}
                sx={{
                  px: 1.5,
                  height: 32,
                  borderRadius: '999px',
                  fontSize: '0.8125rem',
                  fontWeight: 600,
                  bgcolor: it.primary ? 'primary.main' : alpha(theme.palette.primary.main, 0.1),
                  color: it.primary ? 'primary.contrastText' : 'primary.main',
                  '&:hover': { bgcolor: it.primary ? 'primary.dark' : alpha(theme.palette.primary.main, 0.18) },
                }}
              >
                {text(it.label)}
                {it.send ? ' ↗' : ''}
              </ButtonBase>
            ))}
          </Stack>
        )
      case 'callout':
        return (
          <Alert key={k} severity={b.tone ?? 'info'} sx={{ fontSize: '0.8125rem' }}>
            {rich(text(b.text))}
          </Alert>
        )
      case 'tabs':
        return <Tabs key={k} tabs={b.tabs} render={(bs) => bs.map(renderBlock)} />
      case 'list': {
        const Tag = b.ordered ? 'ol' : 'ul'
        return (
          <Box key={k} component={Tag} sx={{ m: 0, pl: 2.5, fontSize: '0.875rem', lineHeight: 1.7 }}>
            {b.items.map((it, j) => (
              <li key={j}>{rich(text(it))}</li>
            ))}
          </Box>
        )
      }
      case 'heading':
        return (
          <Typography key={k} sx={{ fontSize: '0.75rem', fontWeight: 700, letterSpacing: 0.5, textTransform: 'uppercase', color: 'text.secondary', pt: 0.5 }}>
            {text(b.text)}
          </Typography>
        )
      case 'steps':
        return (
          <Stack key={k} sx={{ gap: 0 }}>
            {b.items.map((it, j) => {
              const c = toneColor(it.state === 'done' ? 'success' : it.state === 'error' ? 'error' : it.state === 'active' ? 'info' : 'neutral')
              const hollow = it.state === 'todo' || !it.state
              const last = j === b.items.length - 1
              return (
                <Stack key={j} direction="row" sx={{ gap: 1.25 }}>
                  <Stack sx={{ alignItems: 'center', width: 18, flexShrink: 0 }}>
                    <Box
                      sx={{
                        mt: 0.35,
                        width: 14,
                        height: 14,
                        borderRadius: '50%',
                        display: 'grid',
                        placeItems: 'center',
                        fontSize: '0.5625rem',
                        fontWeight: 800,
                        lineHeight: 1,
                        color: hollow ? c : theme.palette.background.paper,
                        bgcolor: hollow ? 'transparent' : c,
                        border: 2,
                        borderColor: c,
                      }}
                    >
                      {it.state === 'done' ? '\u2713' : it.state === 'error' ? '!' : ''}
                    </Box>
                    {!last && <Box sx={{ flex: 1, width: 2, bgcolor: alpha(theme.palette.text.primary, 0.12), my: 0.25 }} />}
                  </Stack>
                  <Stack sx={{ pb: last ? 0 : 1.25, minWidth: 0 }}>
                    <Typography sx={{ fontSize: '0.875rem', fontWeight: 600, lineHeight: 1.45 }}>{rich(text(it.title))}</Typography>
                    {it.text && <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', lineHeight: 1.5 }}>{rich(text(it.text))}</Typography>}
                  </Stack>
                </Stack>
              )
            })}
          </Stack>
        )
      case 'cards':
        return (
          <Box key={k} sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(190px, 1fr))', gap: 1 }}>
            {b.items.map((it, j) => {
              const c = toneColor(it.tone ?? 'neutral')
              return (
                <Stack
                  key={j}
                  sx={{ p: 1.25, gap: 0.5, minWidth: 0, borderRadius: '12px', bgcolor: alpha(c, it.tone ? 0.08 : 0.05), border: 1, borderColor: alpha(c, it.tone ? 0.3 : 0.12) }}
                >
                  <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75 }}>
                    {it.icon && <Box component="span" sx={{ fontSize: '1rem', lineHeight: 1 }}>{it.icon}</Box>}
                    <Typography sx={{ fontSize: '0.875rem', fontWeight: 650, flex: 1, minWidth: 0 }}>{text(it.title)}</Typography>
                    {it.badge && <Chip label={it.badge} tone={it.tone} />}
                  </Stack>
                  {it.text && <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', lineHeight: 1.5 }}>{rich(text(it.text))}</Typography>}
                </Stack>
              )
            })}
          </Box>
        )
      case 'kv':
        return (
          <Box key={k} sx={{ display: 'grid', gridTemplateColumns: 'minmax(90px, max-content) 1fr', columnGap: 2, rowGap: 0.5 }}>
            {b.items.map((it, j) => (
              <Box key={j} sx={{ display: 'contents' }}>
                <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>{it.label}</Typography>
                <Typography sx={{ fontSize: '0.8125rem', fontWeight: 600, wordBreak: 'break-word' }}>{typeof it.value === 'string' ? rich(text(it.value)) : it.value}</Typography>
              </Box>
            ))}
          </Box>
        )
      case 'progress': {
        const v = Math.max(0, Math.min(1, Number(val(b.value)) || 0))
        return (
          <Stack key={k} sx={{ gap: 0.5 }}>
            <Stack direction="row" sx={{ justifyContent: 'space-between' }}>
              <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>{b.label ? text(b.label) : ''}</Typography>
              <Typography sx={{ fontSize: '0.8125rem', fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}>{Math.round(v * 100)}%</Typography>
            </Stack>
            <Box sx={{ height: 8, borderRadius: '4px', bgcolor: alpha(theme.palette.text.primary, 0.08), overflow: 'hidden' }}>
              <Box sx={{ height: '100%', width: `${v * 100}%`, borderRadius: '4px', bgcolor: 'primary.main', transition: 'width 300ms' }} />
            </Box>
            {b.hint && <Typography sx={{ fontSize: '0.6875rem', color: 'text.disabled' }}>{text(b.hint)}</Typography>}
          </Stack>
        )
      }
      case 'badges':
        return (
          <Stack key={k} direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
            {b.items.map((it, j) => (
              <Chip key={j} label={it.label} tone={it.tone} />
            ))}
          </Stack>
        )
      default:
        return null
    }
  }

  const blocks = spec.blocks ?? (spec.inputs?.length ? [{ type: 'inputs' } as Block] : [])
  return (
    <Stack sx={{ my: 1.5, p: 2, gap: 1.75, borderRadius: '16px', bgcolor: alpha(theme.palette.text.primary, 0.025), border: 1, borderColor: alpha(theme.palette.primary.main, 0.18) }}>
      {(spec.title || spec.description) && (
        <Stack sx={{ gap: 0.25 }}>
          {spec.title && (
            <Stack direction="row" sx={{ alignItems: 'center', gap: 0.875, flexWrap: 'wrap' }}>
              {spec.icon && <Box component="span" sx={{ fontSize: '1.125rem', lineHeight: 1 }}>{spec.icon}</Box>}
              <Typography sx={{ fontSize: '0.9375rem', fontWeight: 650 }}>{text(spec.title)}</Typography>
              {spec.status && <Chip label={spec.status.label} tone={spec.status.tone} />}
            </Stack>
          )}
          {spec.description && <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>{text(spec.description)}</Typography>}
        </Stack>
      )}
      {blocks.map(renderBlock)}
      {errors.length > 0 && <Alert severity="warning" sx={{ fontSize: '0.75rem' }}>{errors.join(' · ')}</Alert>}
    </Stack>
  )
}

/** The theme colour for a tone (neutral is the secondary text colour). */
function useTone() {
  const theme = useTheme()
  return (tone: Tone = 'neutral'): string =>
    tone === 'success'
      ? theme.palette.success.main
      : tone === 'warning'
        ? theme.palette.warning.main
        : tone === 'error'
          ? theme.palette.error.main
          : tone === 'info'
            ? theme.palette.primary.main
            : theme.palette.text.secondary
}

/** Inline markdown in a block's text: **bold**, *italic*, `code`, [link](url). */
function rich(s: string): ReactNode {
  const out: ReactNode[] = []
  const re = /\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)|(?<![*\w])\*([^*\s][^*]*?)\*(?![*\w])/g
  let last = 0
  let n = 0
  for (const m of s.matchAll(re)) {
    const at = m.index ?? 0
    if (at > last) out.push(s.slice(last, at))
    if (m[1] !== undefined) out.push(<strong key={n++}>{m[1]}</strong>)
    else if (m[2] !== undefined)
      out.push(
        <Box key={n++} component="code" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.85em', px: 0.5, borderRadius: '4px', bgcolor: 'action.hover' }}>
          {m[2]}
        </Box>,
      )
    else if (m[3] !== undefined)
      out.push(
        <a key={n++} href={m[4]} target="_blank" rel="noopener noreferrer" style={{ color: 'inherit', textDecoration: 'underline' }}>
          {m[3]}
        </a>,
      )
    else out.push(<em key={n++}>{m[5]}</em>)
    last = at + m[0].length
  }
  if (last < s.length) out.push(s.slice(last))
  return out.length ? out : s
}

function Chip({ label, tone }: { label: string; tone?: Tone }) {
  const c = useTone()(tone ?? 'neutral')
  return (
    <Box
      component="span"
      sx={{ px: 0.875, height: 20, display: 'inline-flex', alignItems: 'center', borderRadius: '999px', fontSize: '0.6875rem', fontWeight: 700, whiteSpace: 'nowrap', color: c, bgcolor: alpha(c, 0.14) }}
    >
      {label}
    </Box>
  )
}

function Tabs({ tabs, render }: { tabs: { label: string; blocks: Block[] }[]; render: (b: Block[]) => ReactNode }) {
  const [at, setAt] = useState(0)
  const theme = useTheme()
  return (
    <Stack sx={{ gap: 1.25 }}>
      <Stack direction="row" sx={{ gap: 0.5, p: 0.375, borderRadius: '10px', bgcolor: alpha(theme.palette.text.primary, 0.05), alignSelf: 'flex-start' }}>
        {tabs.map((t, i) => (
          <ButtonBase key={i} onClick={() => setAt(i)} sx={{ px: 1.25, height: 28, borderRadius: '8px', fontSize: '0.8125rem', fontWeight: 600, color: at === i ? 'text.primary' : 'text.secondary', bgcolor: at === i ? 'background.paper' : 'transparent' }}>
            {t.label}
          </ButtonBase>
        ))}
      </Stack>
      {render(tabs[at]?.blocks ?? [])}
    </Stack>
  )
}

function SortableTable({ columns, rows }: { columns: string[]; rows: (string | number)[][] }) {
  const [sort, setSort] = useState<{ col: number; dir: 1 | -1 } | null>(null)
  const sorted = useMemo(() => {
    if (!sort) return rows
    return [...rows].sort((a, b) => {
      const x = a[sort.col]
      const y = b[sort.col]
      const nx = Number(x)
      const ny = Number(y)
      const c = Number.isFinite(nx) && Number.isFinite(ny) ? nx - ny : String(x).localeCompare(String(y))
      return c * sort.dir
    })
  }, [rows, sort])
  return (
    <Box sx={{ overflowX: 'auto' }}>
      <Box component="table" sx={{ width: '100%', borderCollapse: 'collapse', fontSize: '0.8125rem', '& td, & th': { px: 1, py: 0.75, textAlign: 'left', borderBottom: 1, borderColor: 'divider' } }}>
        <thead>
          <tr>
            {columns.map((c, i) => (
              <Box
                component="th"
                key={i}
                onClick={() => setSort((s) => (s?.col === i ? { col: i, dir: s.dir === 1 ? -1 : 1 } : { col: i, dir: 1 }))}
                sx={{ cursor: 'pointer', fontWeight: 600, color: 'text.secondary', whiteSpace: 'nowrap', userSelect: 'none' }}
              >
                {c}
                {sort?.col === i ? (sort.dir === 1 ? ' ↑' : ' ↓') : ''}
              </Box>
            ))}
          </tr>
        </thead>
        <tbody>
          {sorted.map((r, i) => (
            <tr key={i}>
              {r.map((c, j) => (
                <td key={j}>{typeof c === 'number' ? c.toLocaleString() : c}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </Box>
    </Box>
  )
}
