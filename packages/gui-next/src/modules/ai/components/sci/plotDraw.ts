import type { SciColors } from './dslCommon'
import { axisText, type PlotSpec, type Series } from './plotDsl'
import { seriesColorOf as seriesColor } from './plotColors'
import { escapeHtml, plainLabel } from './symbols'

/**
 * Draws a ```plot script with function-plot (axes, grid, zoom and pan, curve
 * sampling) and adds what it doesn't have as an SVG layer in the same
 * coordinates: implicit curves, vector fields, error bars and reference
 * lines. Slider values live in `values`; changing one redraws in place, so
 * the zoom the reader chose survives.
 */

export interface PlotHandle {
  svg: SVGSVGElement | null
  setValue(name: string, v: number): void
  destroy(): void
}

const evalAt = (s: { fn: (v: Record<string, number>) => number }, vars: Record<string, number>) => {
  const y = s.fn(vars)
  return Number.isFinite(y) ? y : NaN
}

/** A y range that shows the curves: sampled, with outliers (poles) ignored. */
function autoYDomain(spec: PlotSpec, xd: [number, number], values: Record<string, number>): [number, number] {
  const ys: number[] = []
  const N = 240
  for (const s of spec.series) {
    if (s.kind === 'fn') {
      for (let i = 0; i <= N; i++) {
        const x = spec.x.log ? xd[0] * (xd[1] / xd[0]) ** (i / N) : xd[0] + ((xd[1] - xd[0]) * i) / N
        const y = evalAt(s, { ...values, x })
        if (Number.isFinite(y)) ys.push(y)
      }
    } else if (s.kind === 'points') {
      for (const p of s.pts) ys.push(p.y - (p.yerr ?? 0), p.y + (p.yerr ?? 0))
    } else if (s.kind === 'polar') {
      for (let i = 0; i <= N; i++) {
        const th = s.range[0] + ((s.range[1] - s.range[0]) * i) / N
        const r = evalAt(s, { ...values, theta: th })
        if (Number.isFinite(r)) ys.push(r * Math.sin(th), -Math.abs(r))
      }
    } else if (s.kind === 'parametric') {
      for (let i = 0; i <= N; i++) {
        const t = s.range[0] + ((s.range[1] - s.range[0]) * i) / N
        const y = s.fy({ ...values, t })
        if (Number.isFinite(y)) ys.push(y)
      }
    } else if (s.kind === 'hline') ys.push(s.at)
  }
  if (!ys.length) return [-xd[1] + xd[0] ? -(xd[1] - xd[0]) / 2 : -5, (xd[1] - xd[0]) / 2]
  ys.sort((a, b) => a - b)
  const lo = ys[Math.floor(ys.length * 0.01)]
  const hi = ys[Math.ceil(ys.length * 0.99) - 1]
  const span = hi - lo || Math.abs(hi) || 1
  let a = lo - span * 0.08
  let b = hi + span * 0.08
  if (spec.y.log) {
    a = Math.max(lo * 0.7, 1e-12)
    b = hi * 1.4
  }
  return [a, b]
}

/** Draws into `root`, an empty element the caller owns and has already attached. */
export async function drawPlot(root: HTMLElement, spec: PlotSpec, colors: SciColors, width: number, initial: Record<string, number>): Promise<PlotHandle> {
  const mod = await import('function-plot')
  // The package is CommonJS: depending on the bundler, `default` is the function or the exports object holding it.
  const functionPlot = (typeof mod.default === 'function' ? mod.default : (mod.default as unknown as { default: typeof mod.default }).default) as typeof mod.default
  const values = { ...initial }
  const xd = spec.x.domain ?? ([spec.x.log ? 0.1 : -10, spec.x.log ? 1000 : 10] as [number, number])
  const yd = spec.y.domain ?? autoYDomain(spec, xd, values)

  // Style words and the dash/thickness of every curve.
  const attr = (s: Series) => ({
    ...(s.dash ? { 'stroke-dasharray': '7 5' } : s.dot ? { 'stroke-dasharray': '1.5 4', 'stroke-linecap': 'round' } : {}),
    ...(s.thick ? { 'stroke-width': 3 } : { 'stroke-width': 2 }),
  })
  // function-plot keys its curves by `fn`; a no-op `fn` keeps every datum distinct.
  const unique = () => () => NaN
  const data = spec.series.flatMap((s, i): Record<string, unknown>[] => {
    const color = seriesColor(spec, i, colors.dark)
    switch (s.kind) {
      case 'fn':
        return [{ fn: (scope: { x: number }) => s.fn({ ...values, x: scope.x }), graphType: 'polyline', color, attr: attr(s) }]
      case 'polar':
        return [{ r: (scope: { theta: number }) => s.fn({ ...values, theta: scope.theta }), fnType: 'polar', graphType: 'polyline', range: s.range, nSamples: 720, color, attr: attr(s) }]
      case 'parametric':
        return [
          {
            x: (scope: { t: number }) => s.fx({ ...values, t: scope.t }),
            y: (scope: { t: number }) => s.fy({ ...values, t: scope.t }),
            fnType: 'parametric',
            graphType: 'polyline',
            range: s.range,
            nSamples: 720,
            color,
            attr: attr(s),
          },
        ]
      case 'points':
        return [{ fn: unique(), points: s.pts.map((p) => [p.x, p.y]), fnType: 'points', graphType: s.connect ? 'polyline' : 'scatter', color, attr: s.connect ? attr(s) : { r: 3.6, fill: color, 'fill-opacity': 0.9, stroke: color } }]
      default:
        return []
    }
  })

  const chart = functionPlot({
    target: root,
    width,
    height: spec.height,
    grid: spec.grid,
    disableZoom: false,
    xAxis: { type: spec.x.log ? 'log' : 'linear', domain: xd, label: plainLabel(axisText(spec.x)) },
    yAxis: { type: spec.y.log ? 'log' : 'linear', domain: yd, label: plainLabel(axisText(spec.y)) },
    data: data as never,
  } as never)

  const overlayOf = () => {
    const content = root.querySelector<SVGGElement>('svg g.content')!
    let g = content.querySelector<SVGGElement>(':scope > g.sci-overlay')
    if (!g) {
      g = document.createElementNS('http://www.w3.org/2000/svg', 'g')
      g.setAttribute('class', 'sci-overlay')
      content.appendChild(g)
    }
    return g
  }

  const meta = (chart as unknown as { meta: { xScale: { (v: number): number; invert(v: number): number; domain(): number[] }; yScale: { (v: number): number; invert(v: number): number; domain(): number[] }; width: number; height: number } }).meta
  // function-plot tucks the axis labels inside the plot; give them margins of
  // their own instead, by widening the drawing's view box.
  const svgEl = root.querySelector('svg')!
  const baseW = Number(svgEl.getAttribute('width'))
  const baseH = Number(svgEl.getAttribute('height'))
  svgEl.setAttribute('viewBox', `-22 0 ${baseW + 22} ${baseH + 24}`)
  svgEl.setAttribute('width', String(baseW + 22))
  svgEl.setAttribute('height', String(baseH + 24))
  const placeLabels = () => {
    const y = root.querySelector('text.y.axis-label')
    if (y) {
      y.setAttribute('transform', `translate(-46 ${meta.height / 2}) rotate(-90)`)
      y.setAttribute('text-anchor', 'middle')
      y.setAttribute('y', '0')
      y.removeAttribute('dy')
    }
    const x = root.querySelector('text.x.axis-label')
    if (x) {
      x.setAttribute('y', String(meta.height + 34))
      x.setAttribute('x', String(meta.width / 2))
      x.setAttribute('text-anchor', 'middle')
    }
  }
  const paintOverlay = () => {
    const { xScale: X, yScale: Y, width: W, height: H } = meta
    const out: string[] = []
    spec.series.forEach((s, i) => {
      const color = seriesColor(spec, i, colors.dark)
      const dash = s.dash ? ' stroke-dasharray="7 5"' : s.dot ? ' stroke-dasharray="1.5 4"' : ''
      if (s.kind === 'implicit') {
        // Marching squares over the visible window.
        const nx = Math.max(24, Math.round(W / 5))
        const ny = Math.max(16, Math.round(H / 5))
        const f = new Float64Array((nx + 1) * (ny + 1))
        for (let j = 0; j <= ny; j++) for (let k = 0; k <= nx; k++) f[j * (nx + 1) + k] = evalAt(s, { ...values, x: X.invert((k / nx) * W), y: Y.invert((j / ny) * H) })
        const px = (k: number) => (k / nx) * W
        const py = (j: number) => (j / ny) * H
        const d: string[] = []
        const lerp = (a: number, b: number, fa: number, fb: number) => a + ((b - a) * fa) / (fa - fb)
        for (let j = 0; j < ny; j++) {
          for (let k = 0; k < nx; k++) {
            const v = [f[j * (nx + 1) + k], f[j * (nx + 1) + k + 1], f[(j + 1) * (nx + 1) + k + 1], f[(j + 1) * (nx + 1) + k]]
            if (v.some((q) => Number.isNaN(q))) continue
            const pts: [number, number][] = []
            const edge = (a: number, b: number, ax: number, ay: number, bx: number, by: number) => {
              if (v[a] === 0 ? v[b] !== 0 && false : v[a] > 0 !== v[b] > 0) pts.push([lerp(ax, bx, v[a], v[b]), lerp(ay, by, v[a], v[b])])
            }
            edge(0, 1, px(k), py(j), px(k + 1), py(j))
            edge(1, 2, px(k + 1), py(j), px(k + 1), py(j + 1))
            edge(3, 2, px(k), py(j + 1), px(k + 1), py(j + 1))
            edge(0, 3, px(k), py(j), px(k), py(j + 1))
            if (pts.length >= 2) d.push(`M${pts[0][0].toFixed(1)} ${pts[0][1].toFixed(1)}L${pts[1][0].toFixed(1)} ${pts[1][1].toFixed(1)}`)
            if (pts.length === 4) d.push(`M${pts[2][0].toFixed(1)} ${pts[2][1].toFixed(1)}L${pts[3][0].toFixed(1)} ${pts[3][1].toFixed(1)}`)
          }
        }
        out.push(`<path d="${d.join('')}" fill="none" stroke="${color}" stroke-width="${s.thick ? 3 : 2}" stroke-linecap="round"${dash}/>`)
      } else if (s.kind === 'field') {
        const cols = s.density
        const rows = Math.max(3, Math.round((s.density * H) / W))
        const cw = W / cols
        const ch = H / rows
        const vecs: { x: number; y: number; vx: number; vy: number; m: number }[] = []
        let mmax = 0
        for (let j = 0; j < rows; j++) {
          for (let k = 0; k < cols; k++) {
            const x = X.invert((k + 0.5) * cw)
            const y = Y.invert((j + 0.5) * ch)
            const vx = s.fx({ ...values, x, y })
            const vy = s.fy({ ...values, x, y })
            const m = Math.hypot(vx, vy)
            if (!Number.isFinite(m)) continue
            mmax = Math.max(mmax, m)
            vecs.push({ x: (k + 0.5) * cw, y: (j + 0.5) * ch, vx, vy, m })
          }
        }
        const L = Math.min(cw, ch) * 0.82
        for (const v of vecs) {
          if (!v.m) continue
          const len = L * (0.35 + 0.65 * (v.m / (mmax || 1)))
          // The y scale runs downwards on screen, so the field's y is flipped.
          const ux = v.vx / v.m
          const uy = -v.vy / v.m
          const x0 = v.x - (ux * len) / 2
          const y0 = v.y - (uy * len) / 2
          const x1 = v.x + (ux * len) / 2
          const y1 = v.y + (uy * len) / 2
          const hx = -uy
          const hy = ux
          out.push(`<path d="M${x0.toFixed(1)} ${y0.toFixed(1)}L${x1.toFixed(1)} ${y1.toFixed(1)}M${x1.toFixed(1)} ${y1.toFixed(1)}L${(x1 - ux * 6 + hx * 3).toFixed(1)} ${(y1 - uy * 6 + hy * 3).toFixed(1)}M${x1.toFixed(1)} ${y1.toFixed(1)}L${(x1 - ux * 6 - hx * 3).toFixed(1)} ${(y1 - uy * 6 - hy * 3).toFixed(1)}" fill="none" stroke="${color}" stroke-width="1.3" stroke-linecap="round" opacity="${(0.45 + 0.55 * (v.m / (mmax || 1))).toFixed(2)}"/>`)
        }
      } else if (s.kind === 'points') {
        for (const p of s.pts) {
          const cx = X(p.x)
          const cy = Y(p.y)
          if (p.yerr) out.push(`<path d="M${cx} ${Y(p.y - p.yerr)}V${Y(p.y + p.yerr)}M${cx - 4} ${Y(p.y - p.yerr)}H${cx + 4}M${cx - 4} ${Y(p.y + p.yerr)}H${cx + 4}" fill="none" stroke="${color}" stroke-width="1.5"/>`)
          if (p.xerr) out.push(`<path d="M${X(p.x - p.xerr)} ${cy}H${X(p.x + p.xerr)}M${X(p.x - p.xerr)} ${cy - 4}V${cy + 4}M${X(p.x + p.xerr)} ${cy - 4}V${cy + 4}" fill="none" stroke="${color}" stroke-width="1.5"/>`)
        }
      } else if (s.kind === 'hline' || s.kind === 'vline') {
        const h = s.kind === 'hline'
        const pos = h ? Y(s.at) : X(s.at)
        out.push(`<line ${h ? `x1="0" x2="${W}" y1="${pos}" y2="${pos}"` : `y1="0" y2="${H}" x1="${pos}" x2="${pos}"`} stroke="${s.color ?? colors.muted}" stroke-width="1.3" stroke-dasharray="${s.dot ? '1.5 4' : '6 4'}"/>`)
        if (s.label) out.push(`<text x="${h ? W - 6 : pos + 5}" y="${h ? pos - 5 : 14}" text-anchor="${h ? 'end' : 'start'}" font-size="11" fill="${s.color ?? colors.muted}">${escapeHtml(plainLabel(s.label))}</text>`)
      }
    })
    overlayOf().innerHTML = out.join('')
    placeLabels()
  }
  ;(chart as unknown as { on(e: string, f: () => void): void }).on('after:draw', paintOverlay)
  paintOverlay()

  return {
    get svg() {
      return root.querySelector('svg')
    },
    setValue(name, v) {
      values[name] = v
      ;(chart as unknown as { draw(): void }).draw()
    },
    destroy() {
      root.remove()
    },
  }
}
