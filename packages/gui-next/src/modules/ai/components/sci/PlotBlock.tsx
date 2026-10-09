import { useEffect, useMemo, useRef, useState } from 'react'
import { Box, Slider, Stack, Typography } from '../../../../ui'
import { CHAT_FONT } from '../../lib/chatStyle'
import { SciBlock, useSettled, type SciError } from './SciBlock'
import { inlineComputedStyles, useSciColors } from './SvgFigure'
import { DslError, type SciColors } from './dslCommon'
import { parsePlot, type PlotSpec } from './plotDsl'
import { flattenSvg, labelHtml } from './symbols'
import type { PlotHandle } from './plotDraw'
import { seriesColorOf } from './plotColors'

/** The plot's own colours for function-plot's class-styled axes and grid. */
function plotStyles(c: SciColors) {
  return {
    '& svg': { display: 'block', maxWidth: '100%', overflow: 'visible' },
    '& .function-plot': { fontFamily: 'inherit', fontSize: 11 },
    '& .function-plot .tick line': { stroke: c.line, opacity: 0.9 },
    '& .function-plot .tick text': { fill: c.muted, fontSize: 11 },
    '& .function-plot .domain': { stroke: c.muted },
    '& .function-plot .origin': { stroke: c.muted, opacity: 0.8 },
    '& .function-plot .axis-label, & .function-plot text.title': { fill: c.ink, fontSize: 12 },
    '& .function-plot .top-right-legend': { display: 'none' },
    '& .function-plot .line': { fill: 'none' },
  } as const
}

/** One plot, drawn into a div and kept in sync with the sliders. */
function PlotView({ spec, values, holder, large, colors }: { spec: PlotSpec; values: Record<string, number>; holder?: React.Ref<HTMLDivElement>; large: boolean; colors: SciColors }) {
  const host = useRef<HTMLDivElement | null>(null)
  const handle = useRef<PlotHandle | null>(null)
  const latest = useRef(values)
  latest.current = values
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const el = host.current
    if (!el) return
    let cancelled = false
    // Each run draws into its own element, so a run that was superseded can never clear a newer one.
    const root = document.createElement('div')
    el.replaceChildren(root)
    const width = Math.max(280, Math.min(large ? 1100 : 700, el.clientWidth || 640))
    void (async () => {
      try {
        const { drawPlot } = await import('./plotDraw')
        const h = await drawPlot(root, spec, colors, width, latest.current)
        if (cancelled) {
          h.destroy()
          return
        }
        handle.current = h
        setError(null)
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e))
      }
    })()
    return () => {
      cancelled = true
      handle.current?.destroy()
      handle.current = null
      root.remove()
    }
    // The figure is rebuilt when the script or the theme changes; sliders redraw in place.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [spec, colors.dark, colors.ink, large])

  useEffect(() => {
    if (!handle.current) return
    for (const [k, v] of Object.entries(values)) handle.current.setValue(k, v)
  }, [values])

  return (
    <>
      <Box
        ref={(n: HTMLDivElement | null) => {
          host.current = n
          if (typeof holder === 'function') holder(n)
          else if (holder) (holder as { current: HTMLDivElement | null }).current = n
        }}
        sx={{ px: 1, pt: 1, display: 'flex', justifyContent: 'center', minHeight: spec.height, ...plotStyles(colors) }}
      />
      {error && <Typography sx={{ px: 1.5, pb: 1, fontSize: CHAT_FONT.meta, color: 'error.main' }}>{error}</Typography>}
    </>
  )
}

const fmt = (v: number) => (Math.abs(v) >= 100 ? v.toFixed(0) : Math.abs(v) >= 10 ? v.toFixed(1) : v.toFixed(2))

function Legend({ spec, colors }: { spec: PlotSpec; colors: SciColors }) {
  const items = spec.series.map((s, i) => ({ s, i })).filter(({ s }) => s.label && s.kind !== 'hline' && s.kind !== 'vline')
  if (!items.length) return null
  return (
    <Stack direction="row" sx={{ px: 1.5, pt: 1, gap: 1.75, flexWrap: 'wrap' }}>
      {items.map(({ s, i }) => (
        <Stack key={i} direction="row" sx={{ alignItems: 'center', gap: 0.75 }}>
          <Box sx={{ width: 14, height: 3, borderRadius: 2, bgcolor: seriesColorOf(spec, i, colors.dark) }} />
          <Typography sx={{ fontSize: CHAT_FONT.meta, '& .katex': { fontSize: '1em' } }} color="text.secondary">
            <span dangerouslySetInnerHTML={{ __html: labelHtml(s.label ?? '') }} />
          </Typography>
        </Stack>
      ))}
    </Stack>
  )
}

/** A ```plot fence: function graphs with sliders, drawn by function-plot (loaded on first use). */
export function PlotBlock({ code }: { code: string }) {
  const source = useSettled(code)
  const colors = useSciColors()
  const parsed = useMemo<{ spec: PlotSpec } | { error: SciError }>(() => {
    try {
      return { spec: parsePlot(source) }
    } catch (e) {
      return { error: e instanceof DslError ? { message: e.message, line: e.line } : { message: e instanceof Error ? e.message : String(e) } }
    }
  }, [source])
  const spec = 'spec' in parsed ? parsed.spec : null
  const [values, setValues] = useState<Record<string, number>>({})
  const holder = useRef<HTMLDivElement | null>(null)

  // Sliders restart at the script's own values when the script changes.
  useEffect(() => {
    if (spec) setValues(Object.fromEntries(spec.params.map((p) => [p.name, p.value])))
  }, [spec])

  const getSvg = () => {
    const svg = holder.current?.querySelector('svg')
    if (!svg) return null
    // function-plot styles through classes; carry them into the copy.
    const clone = svg.cloneNode(true) as SVGSVGElement
    inlineComputedStyles(svg, clone)
    const out = flattenSvg(clone, colors.ink)
    return out.replace(/(<svg[^>]*>)/, `$1<rect width="100%" height="100%" fill="${colors.paper}"/>`)
  }

  return (
    <SciBlock
      label="Plot"
      title={spec?.title}
      source={code}
      error={'error' in parsed ? parsed.error : null}
      getSvg={getSvg}
      hint={spec?.params.length ? 'drag the sliders · scroll to zoom' : 'scroll to zoom · drag to pan'}
      render={({ large }) =>
        spec ? (
          <>
            <Legend spec={spec} colors={colors} />
            <PlotView spec={spec} values={values} colors={colors} large={large} holder={large ? undefined : holder} />
            {spec.params.length > 0 && (
              <Stack sx={{ px: 2, pb: 1.5, pt: 0.5, gap: 0.25 }}>
                {spec.params.map((p) => (
                  <Stack key={p.name} direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
                    <Typography sx={{ fontSize: CHAT_FONT.meta, minWidth: 96, '& .katex': { fontSize: '1em' } }} color="text.secondary">
                      <span dangerouslySetInnerHTML={{ __html: labelHtml(p.label ?? p.name) }} />
                    </Typography>
                    <Slider value={values[p.name] ?? p.value} min={p.min} max={p.max} step={p.step} aria-label={p.name} onChange={(_e, v) => setValues((cur) => ({ ...cur, [p.name]: v }))} />
                    <Typography sx={{ fontSize: CHAT_FONT.meta, minWidth: 48, textAlign: 'right', fontFamily: '"Geist Mono", ui-monospace, monospace' }}>{fmt(values[p.name] ?? p.value)}</Typography>
                  </Stack>
                ))}
              </Stack>
            )}
          </>
        ) : null
      }
    />
  )
}
