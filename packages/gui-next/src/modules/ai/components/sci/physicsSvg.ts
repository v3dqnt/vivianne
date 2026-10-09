import type { Prim, Scene } from './physicsDsl'
import type { SciColors } from './dslCommon'
import { escapeHtml, plainLabel } from './symbols'

/**
 * A scene as a standalone SVG string: what Copy SVG / PNG exports. The live
 * figure is drawn by JSXGraph, whose maths labels are HTML on top of the
 * drawing and would not travel; this draws the same primitives with plain text.
 */
export function sceneToSvg(scene: Scene, c: SciColors, t = 0, width = 720): string {
  const [x1, y1, x2, y2] = scene.bbox
  const w = x2 - x1
  const h = y1 - y2
  const height = Math.max(200, Math.round((width * h) / w))
  const X = (x: number) => ((x - x1) / w) * width
  const Y = (y: number) => ((y1 - y) / h) * height
  const title = scene.title ? 28 : 0
  const ink = c.ink
  const dashAttr = (d?: number) => (d === 1 ? ' stroke-dasharray="7 5"' : d === 2 ? ' stroke-dasharray="1.5 4" stroke-linecap="round"' : '')
  const parts: string[] = []
  const px = (v: number) => Math.round(v * 10) / 10
  const draw = (p: Prim) => {
    if (p.t === 'line') {
      const pts = p.pts.map(([x, y]) => `${px(X(x))},${px(Y(y))}`).join(' ')
      const stroke = p.color ?? ink
      const fill = p.fill ? `fill="${p.fill}" fill-opacity="${p.fillOpacity ?? 1}"` : 'fill="none"'
      let marker = ''
      if (p.arrow && p.pts.length >= 2) {
        const arrowAt = (tip: [number, number], from: [number, number]) => {
          const dx = X(tip[0]) - X(from[0])
          const dy = Y(tip[1]) - Y(from[1])
          const l = Math.hypot(dx, dy) || 1
          const ux = dx / l
          const uy = dy / l
          const s = 9
          return `<polygon points="${px(X(tip[0]))},${px(Y(tip[1]))} ${px(X(tip[0]) - ux * s - uy * 4)},${px(Y(tip[1]) - uy * s + ux * 4)} ${px(X(tip[0]) - ux * s + uy * 4)},${px(Y(tip[1]) - uy * s - ux * 4)}" fill="${stroke}"/>`
        }
        const n = p.pts.length
        if (p.arrow !== 'start') marker += arrowAt(p.pts[n - 1], p.pts[n - 2])
        if (p.arrow !== 'end') marker += arrowAt(p.pts[0], p.pts[1])
      }
      parts.push(`<${p.closed ? 'polygon' : 'polyline'} points="${pts}" ${fill} stroke="${stroke}" stroke-width="${p.width ?? 2}" stroke-linejoin="round"${dashAttr(p.dash)}/>${marker}`)
    } else if (p.t === 'text') {
      const anchor = p.anchor === 'l' ? 'start' : p.anchor === 'r' ? 'end' : 'middle'
      const dy = p.v === 't' ? (p.size ?? 14) * 0.9 : p.v === 'm' ? (p.size ?? 14) * 0.35 : 0
      parts.push(`<text x="${px(X(p.at[0]))}" y="${px(Y(p.at[1]) + dy)}" text-anchor="${anchor}" font-size="${p.size ?? 14}" fill="${p.color ?? ink}"${p.bold ? ' font-weight="700"' : ''}>${escapeHtml(plainLabel(p.text))}</text>`)
    } else {
      const n = p.samples ?? 300
      const pts: string[] = []
      for (let i = 0; i <= n; i++) {
        const x = p.from + ((p.to - p.from) * i) / n
        pts.push(`${px(X(x))},${px(Y(p.f(x, t)))}`)
      }
      parts.push(`<polyline points="${pts.join(' ')}" fill="none" stroke="${p.color ?? ink}" stroke-width="${p.width ?? 2}"${dashAttr(p.dash)}/>`)
    }
  }
  if (scene.axes) parts.push(`<line x1="0" x2="${width}" y1="${px(Y(0))}" y2="${px(Y(0))}" stroke="${c.muted}" stroke-width="1"/>`)
  scene.prims.forEach(draw)
  const head = scene.title ? `<text x="${width / 2}" y="19" text-anchor="middle" font-size="14" font-weight="600" fill="${ink}">${escapeHtml(plainLabel(scene.title))}</text>` : ''
  return `<svg xmlns="http://www.w3.org/2000/svg" class="sci-svg" viewBox="0 0 ${width} ${height + title}" width="${width}" height="${height + title}" font-family="system-ui, -apple-system, 'Segoe UI', sans-serif" color="${ink}">${head}<g transform="translate(0 ${title})">${parts.join('')}</g></svg>`
}
