import type { SciColors } from './dslCommon'
import type { Scene } from './physicsDsl'
import { hasMath, labelHtml, plainLabel } from './symbols'

/**
 * Draws a compiled physics `Scene` as a JSXGraph construction: every polyline
 * is a curve, every function a function graph, every label a text element.
 * The board can be panned by dragging; a time-dependent figure (a wave) has
 * its time moved through `setTime`, which re-evaluates the function graphs.
 */

export interface PhysicsHandle {
  setTime(t: number): void
  destroy(): void
}

// JSXGraph ships types for a global namespace, not for its module exports.
interface Board {
  create(type: string, parents: unknown[], attrs?: Record<string, unknown>): unknown
  update(): void
  defaultAxes?: { x: { setAttribute(a: object): void }; y: { setAttribute(a: object): void } }
}
interface JSXGraphApi {
  initBoard(id: string, attrs: Record<string, unknown>): Board
  freeBoard(b: Board): void
}

let counter = 0

export async function drawPhysics(root: HTMLElement, scene: Scene, c: SciColors, width: number): Promise<PhysicsHandle> {
  const { JSXGraph } = (await import('jsxgraph')) as unknown as { JSXGraph: JSXGraphApi }
  const [x1, y1, x2, y2] = scene.bbox
  const height = Math.round(Math.min(560, Math.max(220, (width * (y1 - y2)) / (x2 - x1))))
  const id = `sci-jxg-${++counter}`
  root.id = id
  root.style.width = `${width}px`
  root.style.height = `${height}px`
  // JSXGraph names its drawing area by id, so the element must be in the document.
  const board = JSXGraph.initBoard(id, {
    boundingBox: [x1, y1, x2, y2],
    keepAspectRatio: true,
    axis: !!scene.axes,
    showCopyright: false,
    showNavigation: false,
    showInfobox: false,
    pan: { enabled: true, needShift: false },
    zoom: { wheel: false, pinch: false },
    renderer: 'svg',
  })
  if (scene.axes && board.defaultAxes) {
    const axis = { strokeColor: c.muted, highlight: false, ticks: { strokeColor: c.muted, label: { strokeColor: c.muted, fontSize: 11 } } }
    board.defaultAxes.x.setAttribute(axis)
    board.defaultAxes.y.setAttribute(axis)
  }
  const state = { t: 0 }
  const base = { fixed: true, highlight: false, withLabel: false, name: '' }
  const dash = (d?: number) => (d === 1 ? 3 : d === 2 ? 1 : 0)

  for (const p of scene.prims) {
    if (p.t === 'line') {
      const pts = p.closed && p.pts.length > 2 ? [...p.pts, p.pts[0]] : p.pts
      const attrs: Record<string, unknown> = {
        ...base,
        strokeColor: p.color ?? c.ink,
        strokeWidth: p.width ?? 2,
        dash: dash(p.dash),
        lineCap: 'round',
        ...(p.fill ? { fillColor: p.fill, fillOpacity: p.fillOpacity ?? 1 } : {}),
        ...(p.arrow === 'end' || p.arrow === 'both' ? { lastArrow: { type: 2, size: 7 } } : {}),
        ...(p.arrow === 'start' || p.arrow === 'both' ? { firstArrow: { type: 2, size: 7 } } : {}),
      }
      board.create('curve', [pts.map((q) => q[0]), pts.map((q) => q[1])], attrs)
    } else if (p.t === 'text') {
      const math = hasMath(p.text)
      board.create('text', [p.at[0], p.at[1], math ? labelHtml(p.text) : plainLabel(p.text)], {
        ...base,
        fontSize: p.size ?? 14,
        strokeColor: p.color ?? c.ink,
        anchorX: p.anchor === 'l' ? 'left' : p.anchor === 'r' ? 'right' : 'middle',
        anchorY: p.v === 't' ? 'top' : p.v === 'b' ? 'bottom' : 'middle',
        parse: false,
        useMathJax: false,
        useKatex: false,
        display: math ? 'html' : 'internal',
        cssStyle: p.bold ? 'font-weight:700' : '',
      })
    } else {
      board.create('functiongraph', [(x: number) => p.f(x, state.t), p.from, p.to], {
        ...base,
        strokeColor: p.color ?? c.ink,
        strokeWidth: p.width ?? 2,
        dash: dash(p.dash),
        numberPoints: p.samples ?? 300,
      })
    }
  }
  board.update()
  return {
    setTime(t: number) {
      state.t = t
      board.update()
    },
    destroy() {
      try {
        JSXGraph.freeBoard(board)
      } catch {
        // The element is already gone.
      }
      root.remove()
    },
  }
}
