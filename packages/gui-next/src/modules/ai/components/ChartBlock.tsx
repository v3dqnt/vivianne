import { useMemo, useState } from 'react'
import { Box, Stack, Typography, ButtonBase, useTheme } from '../../../ui'
import { seriesColors, ALL_PAIRS_SAFE_SLOTS, DIRECT_LABEL_MAX } from '../lib/chartPalette'

/** One `{x, y}` sample. `x` is numeric for line/area/scatter, a label for bar/pie. */
interface Point {
  x: number | string
  y: number
}
interface Series {
  name: string
  points: Point[]
}
export interface ChartSpec {
  type: 'bar' | 'line' | 'area' | 'pie' | 'scatter'
  title?: string
  xLabel?: string
  yLabel?: string
  /** Single-series shorthand. */
  data?: Point[]
  /** Multi-series form. Takes precedence over `data`. */
  series?: Series[]
}

const W = 640
const H = 300
const PAD = { top: 16, right: 16, bottom: 40, left: 52 }
const PLOT_W = W - PAD.left - PAD.right
const PLOT_H = H - PAD.top - PAD.bottom

/** Parse and validate. Returns a message instead of throwing so a malformed
 * spec renders as a readable note rather than a blank space or a crash. */
function parseSpec(code: string): { spec: ChartSpec; series: Series[] } | { error: string } {
  let raw: unknown
  try {
    raw = JSON.parse(code)
  } catch (e) {
    return { error: `Not valid JSON: ${e instanceof Error ? e.message : String(e)}` }
  }
  if (!raw || typeof raw !== 'object') return { error: 'Expected a JSON object.' }
  const spec = raw as ChartSpec
  const kinds = ['bar', 'line', 'area', 'pie', 'scatter']
  if (!kinds.includes(spec.type)) return { error: `"type" must be one of: ${kinds.join(', ')}.` }

  let series: Series[]
  if (Array.isArray(spec.series) && spec.series.length) {
    series = spec.series
  } else if (Array.isArray(spec.data) && spec.data.length) {
    series = [{ name: spec.yLabel || 'Value', points: spec.data }]
  } else {
    return { error: 'Provide "data": [{x, y}, ...] or "series": [{name, points}, ...].' }
  }
  for (const s of series) {
    if (!Array.isArray(s.points) || s.points.length === 0) return { error: `Series "${s.name}" has no points.` }
    if (s.points.some((p) => typeof p?.y !== 'number' || !Number.isFinite(p.y))) {
      return { error: `Series "${s.name}" has a non-numeric y value.` }
    }
  }
  return { spec, series }
}

function niceTicks(min: number, max: number, count = 5): number[] {
  if (min === max) return [min]
  const span = max - min
  const step = Math.pow(10, Math.floor(Math.log10(span / count)))
  const err = (span / count) / step
  const mult = err >= 7.5 ? 10 : err >= 3 ? 5 : err >= 1.5 ? 2 : 1
  const s = step * mult
  const start = Math.ceil(min / s) * s
  const out: number[] = []
  for (let v = start; v <= max + 1e-9; v += s) out.push(Number(v.toFixed(10)))
  return out
}

function fmt(n: number): string {
  if (Math.abs(n) >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (Math.abs(n) >= 1000) return `${(n / 1000).toFixed(1)}K`
  return Number.isInteger(n) ? String(n) : n.toFixed(2)
}

/**
 * Renders a ```chart fenced block as an inline SVG chart.
 *
 * Built as plain SVG rather than pulling in a charting library: at this size
 * the whole surface is ~15 marks, and the dataviz skill's mark specs (rounded
 * data-ends anchored to the baseline, a 2px surface gap between adjacent
 * fills, recessive axes, text in text tokens rather than the series colour)
 * are all things a library's defaults would have to be fought for.
 */
export function ChartBlock({ code }: { code: string }) {
  const theme = useTheme()
  const [showTable, setShowTable] = useState(false)
  const [hover, setHover] = useState<{ x: number; y: number; label: string } | null>(null)

  const mode = theme.palette.mode === 'dark' ? 'dark' : 'light'
  const colors = seriesColors(mode)
  const surface = theme.palette.background.paper
  const parsed = useMemo(() => parseSpec(code), [code])

  if ('error' in parsed) {
    return (
      <Box sx={{ my: 1, p: 1.5, border: 1, borderColor: 'divider', borderRadius: '12px' }}>
        <Typography variant="caption" color="error.main" sx={{ fontWeight: 600, display: 'block' }}>
          Chart spec problem
        </Typography>
        <Typography variant="caption" color="text.secondary">
          {parsed.error}
        </Typography>
      </Box>
    )
  }

  const { spec } = parsed
  let { series } = parsed

  // Never cycle the palette. Past the slot count, or past the all-pairs-safe
  // cap for scatter, the tail folds into one "Other" series.
  const cap = spec.type === 'scatter' ? ALL_PAIRS_SAFE_SLOTS : colors.length
  let folded = 0
  if (series.length > cap) {
    folded = series.length - (cap - 1)
    const kept = series.slice(0, cap - 1)
    const rest = series.slice(cap - 1)
    kept.push({ name: `Other (${folded})`, points: rest[0].points })
    series = kept
  }

  const axisColor = theme.palette.divider
  const textMuted = theme.palette.text.secondary

  const allY = series.flatMap((s) => s.points.map((p) => p.y))
  const yMax = Math.max(0, ...allY)
  const yMin = Math.min(0, ...allY)
  const ticks = niceTicks(yMin, yMax)
  const yTop = Math.max(yMax, ...ticks)
  const yBottom = Math.min(yMin, ...ticks)
  const yScale = (v: number) => PAD.top + PLOT_H - ((v - yBottom) / (yTop - yBottom || 1)) * PLOT_H

  const categories = series[0].points.map((p) => String(p.x))
  const isCategorical = spec.type === 'bar' || spec.type === 'pie'
  const xScale = (i: number, n: number) =>
    isCategorical ? PAD.left + ((i + 0.5) / n) * PLOT_W : PAD.left + (n === 1 ? PLOT_W / 2 : (i / (n - 1)) * PLOT_W)

  const showLegend = series.length >= 2
  const showDirectLabels = series.length <= DIRECT_LABEL_MAX

  const table = (
    <Box sx={{ overflowX: 'auto', mt: 1 }}>
      <Box component="table" sx={{ borderCollapse: 'collapse', fontSize: '0.75rem', width: '100%' }}>
        <thead>
          <Box component="tr" sx={{ borderBottom: 1, borderColor: 'divider' }}>
            <Box component="th" sx={{ textAlign: 'left', p: 0.5, color: 'text.secondary', fontWeight: 600 }}>
              {spec.xLabel || 'x'}
            </Box>
            {series.map((s) => (
              <Box key={s.name} component="th" sx={{ textAlign: 'right', p: 0.5, color: 'text.secondary', fontWeight: 600 }}>
                {s.name}
              </Box>
            ))}
          </Box>
        </thead>
        <tbody>
          {series[0].points.map((p, i) => (
            <Box component="tr" key={i}>
              <Box component="td" sx={{ p: 0.5, color: 'text.secondary' }}>
                {String(p.x)}
              </Box>
              {series.map((s) => (
                <Box key={s.name} component="td" sx={{ p: 0.5, textAlign: 'right', fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
                  {s.points[i] ? fmt(s.points[i].y) : '—'}
                </Box>
              ))}
            </Box>
          ))}
        </tbody>
      </Box>
    </Box>
  )

  return (
    <Box
      sx={{
        my: 1,
        p: 1.5,
        border: 1,
        borderColor: 'divider',
        borderRadius: '12px',
        bgcolor: 'background.paper',
        maxWidth: '100%',
        overflowX: 'auto',
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'baseline', justifyContent: 'space-between', gap: 1, mb: 0.5 }}>
        {spec.title && (
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            {spec.title}
          </Typography>
        )}
        <ButtonBase
          onClick={() => setShowTable((v) => !v)}
          sx={{ ml: 'auto', fontSize: '0.6875rem', color: 'text.secondary', borderRadius: 1, px: 0.75, py: 0.25, '&:hover': { bgcolor: 'action.hover' } }}
        >
          {showTable ? 'Chart' : 'Table'}
        </ButtonBase>
      </Stack>

      {showTable ? (
        table
      ) : (
        <Box sx={{ position: 'relative' }}>
          <Box component="svg" viewBox={`0 0 ${W} ${H}`} sx={{ width: '100%', height: 'auto', display: 'block', overflow: 'visible' }}>
            {spec.type !== 'pie' && (
              <>
                {/* Recessive grid: hairlines only, no axis box. */}
                {ticks.map((t) => (
                  <g key={t}>
                    <line x1={PAD.left} x2={W - PAD.right} y1={yScale(t)} y2={yScale(t)} stroke={axisColor} strokeWidth={1} />
                    <text x={PAD.left - 8} y={yScale(t) + 4} textAnchor="end" fontSize={11} fill={textMuted}>
                      {fmt(t)}
                    </text>
                  </g>
                ))}
                {categories.map((c, i) => (
                  <text
                    key={`${c}-${i}`}
                    x={xScale(i, categories.length)}
                    y={H - PAD.bottom + 16}
                    textAnchor="middle"
                    fontSize={11}
                    fill={textMuted}
                  >
                    {c.length > 10 ? `${c.slice(0, 9)}…` : c}
                  </text>
                ))}
              </>
            )}

            {spec.type === 'bar' &&
              series.map((s, si) =>
                s.points.map((p, i) => {
                  const groupW = PLOT_W / s.points.length
                  // 2px surface gap between adjacent fills, per the mark specs.
                  const barW = Math.max(2, (groupW * 0.7) / series.length - 2)
                  const x = PAD.left + i * groupW + groupW * 0.15 + si * (barW + 2)
                  const y0 = yScale(0)
                  const y1 = yScale(p.y)
                  const top = Math.min(y0, y1)
                  const h = Math.abs(y1 - y0)
                  return (
                    <rect
                      key={`${si}-${i}`}
                      x={x}
                      y={top}
                      width={barW}
                      height={Math.max(1, h)}
                      // Rounded data-end only; the baseline end stays square
                      // because it is anchored to the axis.
                      rx={Math.min(4, barW / 2)}
                      fill={colors[si]}
                      onMouseEnter={() => setHover({ x, y: top, label: `${s.name} · ${String(p.x)}: ${fmt(p.y)}` })}
                      onMouseLeave={() => setHover(null)}
                    />
                  )
                }),
              )}

            {(spec.type === 'line' || spec.type === 'area') &&
              series.map((s, si) => {
                const pts = s.points.map((p, i) => [xScale(i, s.points.length), yScale(p.y)] as const)
                const d = pts.map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x},${y}`).join(' ')
                return (
                  <g key={si}>
                    {spec.type === 'area' && (
                      <path
                        d={`${d} L${pts[pts.length - 1][0]},${yScale(yBottom)} L${pts[0][0]},${yScale(yBottom)} Z`}
                        fill={colors[si]}
                        opacity={0.16}
                      />
                    )}
                    <path d={d} fill="none" stroke={colors[si]} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
                    {pts.map(([x, y], i) => (
                      <circle
                        key={i}
                        cx={x}
                        cy={y}
                        r={4}
                        fill={colors[si]}
                        // 2px surface ring so overlapping marks stay separable.
                        stroke={surface}
                        strokeWidth={2}
                        onMouseEnter={() => setHover({ x, y, label: `${s.name} · ${String(s.points[i].x)}: ${fmt(s.points[i].y)}` })}
                        onMouseLeave={() => setHover(null)}
                      />
                    ))}
                    {showDirectLabels && (
                      <text x={pts[pts.length - 1][0] + 6} y={pts[pts.length - 1][1] + 4} fontSize={11} fill={textMuted}>
                        {s.name}
                      </text>
                    )}
                  </g>
                )
              })}

            {spec.type === 'scatter' &&
              series.map((s, si) =>
                s.points.map((p, i) => {
                  const xs = s.points.map((q) => Number(q.x))
                  const lo = Math.min(...xs)
                  const hi = Math.max(...xs)
                  const x = PAD.left + ((Number(p.x) - lo) / (hi - lo || 1)) * PLOT_W
                  return (
                    <circle
                      key={`${si}-${i}`}
                      cx={x}
                      cy={yScale(p.y)}
                      r={5}
                      fill={colors[si]}
                      stroke={surface}
                      strokeWidth={2}
                      onMouseEnter={() => setHover({ x, y: yScale(p.y), label: `${s.name} · ${p.x}, ${fmt(p.y)}` })}
                      onMouseLeave={() => setHover(null)}
                    />
                  )
                }),
              )}

            {spec.type === 'pie' &&
              (() => {
                const pts = series[0].points
                const total = pts.reduce((sum, p) => sum + Math.max(0, p.y), 0) || 1
                const cx = W / 2
                const cy = H / 2
                const r = Math.min(PLOT_W, PLOT_H) / 2 - 8
                let angle = -Math.PI / 2
                return pts.map((p, i) => {
                  const frac = Math.max(0, p.y) / total
                  const end = angle + frac * Math.PI * 2
                  const large = frac > 0.5 ? 1 : 0
                  const x0 = cx + r * Math.cos(angle)
                  const y0 = cy + r * Math.sin(angle)
                  const x1 = cx + r * Math.cos(end)
                  const y1 = cy + r * Math.sin(end)
                  const mid = (angle + end) / 2
                  const d = `M${cx},${cy} L${x0},${y0} A${r},${r} 0 ${large} 1 ${x1},${y1} Z`
                  angle = end
                  return (
                    <g key={i}>
                      <path
                        d={d}
                        fill={colors[i % colors.length]}
                        stroke={surface}
                        strokeWidth={2}
                        onMouseEnter={() => setHover({ x: cx, y: cy, label: `${String(p.x)}: ${fmt(p.y)} (${Math.round(frac * 100)}%)` })}
                        onMouseLeave={() => setHover(null)}
                      />
                      {frac > 0.06 && (
                        <text
                          x={cx + r * 0.68 * Math.cos(mid)}
                          y={cy + r * 0.68 * Math.sin(mid) + 4}
                          textAnchor="middle"
                          fontSize={11}
                          fill={theme.palette.getContrastText(colors[i % colors.length])}
                        >
                          {Math.round(frac * 100)}%
                        </text>
                      )}
                    </g>
                  )
                })
              })()}

            {spec.yLabel && spec.type !== 'pie' && (
              <text x={12} y={PAD.top + PLOT_H / 2} fontSize={11} fill={textMuted} transform={`rotate(-90 12 ${PAD.top + PLOT_H / 2})`} textAnchor="middle">
                {spec.yLabel}
              </text>
            )}
          </Box>

          {hover && (
            <Box
              sx={{
                position: 'absolute',
                left: `${(hover.x / W) * 100}%`,
                top: `${(hover.y / H) * 100}%`,
                transform: 'translate(-50%, -130%)',
                px: 1,
                py: 0.5,
                borderRadius: 1,
                bgcolor: 'background.paper',
                border: 1,
                borderColor: 'divider',
                boxShadow: 3,
                fontSize: '0.6875rem',
                whiteSpace: 'nowrap',
                pointerEvents: 'none',
              }}
            >
              {hover.label}
            </Box>
          )}
        </Box>
      )}

      {/* Legend is unconditional for >=2 series: identity must never rest on
          colour alone, and in light mode three palette slots sit below 3:1
          against the surface, which obliges visible relief. */}
      {showLegend && !showTable && (
        <Stack direction="row" sx={{ flexWrap: 'wrap', gap: 1.5, mt: 1 }}>
          {series.map((s, i) => (
            <Stack key={s.name} direction="row" sx={{ alignItems: 'center', gap: 0.5 }}>
              <Box sx={{ width: 8, height: 8, borderRadius: '2px', bgcolor: colors[i % colors.length] }} />
              <Typography variant="caption" color="text.secondary">
                {s.name}
              </Typography>
            </Stack>
          ))}
        </Stack>
      )}

      {folded > 0 && (
        <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 0.5 }}>
          {folded} further series folded into "Other" — the palette is not cycled, so identity stays unambiguous.
        </Typography>
      )}
    </Box>
  )
}
