import { DslError, keyValue, parseColor, parseNumber, scriptLines, type Line } from './dslCommon'
import { compileExpr, evalConst, ExprError, RESERVED, type Fn } from './mathExpr'

/**
 * The ```plot script: graphs for physics and mathematics.
 *
 *   title: Damped oscillator
 *   x: Time t [s]  0..12            axis label, [unit], domain, optional `log`
 *   y: Displacement x [m]  -2..2
 *   param A = 1.5  0..3  step 0.1   a slider
 *   fn: A*exp(-g*x)*cos(2*x) "x(t)"
 *   polar: 1 + cos(theta)
 *   parametric: cos(t), sin(2*t)  t=0..2pi
 *   implicit: x^2 + y^2 = 4
 *   field: -y, x
 *   points: (1, 2.1) (2, 3.9) (3, 6.2)  err=0.3 "data"
 */

export interface PlotAxis {
  label?: string
  unit?: string
  domain?: [number, number]
  log: boolean
}

export interface Param {
  name: string
  value: number
  min: number
  max: number
  step: number
  label?: string
  line: number
}

interface Styled {
  label?: string
  color?: string
  dash?: boolean
  dot?: boolean
  thick?: boolean
  line: number
}

export type Series =
  | (Styled & { kind: 'fn'; src: string; fn: Fn })
  | (Styled & { kind: 'polar'; src: string; fn: Fn; range: [number, number] })
  | (Styled & { kind: 'parametric'; xs: string; ys: string; fx: Fn; fy: Fn; range: [number, number] })
  | (Styled & { kind: 'implicit'; src: string; fn: Fn })
  | (Styled & { kind: 'field'; fx: Fn; fy: Fn; density: number })
  | (Styled & { kind: 'points'; pts: { x: number; y: number; xerr?: number; yerr?: number }[]; connect: boolean })
  | (Styled & { kind: 'hline' | 'vline'; at: number })

export interface PlotSpec {
  title?: string
  x: PlotAxis
  y: PlotAxis
  grid: boolean
  height: number
  params: Param[]
  series: Series[]
}

const KEYS = new Set(['title', 'x', 'y', 'xaxis', 'yaxis', 'grid', 'height', 'param', 'fn', 'function', 'polar', 'parametric', 'implicit', 'field', 'points', 'data', 'hline', 'vline'])

const bad = (e: unknown, line: number): never => {
  if (e instanceof DslError) throw e
  throw new DslError(e instanceof ExprError || e instanceof Error ? e.message : String(e), line)
}

/** Pull `"label"`, `#colour` and the style words out of the end of a value. */
function styleOf(value: string, line: number): { rest: string } & Omit<Styled, 'line'> {
  let rest = value
  const out: Omit<Styled, 'line'> = {}
  const q = /"([^"]*)"/.exec(rest)
  if (q) {
    out.label = q[1]
    rest = rest.replace(q[0], ' ')
  }
  const col = /(?:^|\s)(#[0-9a-fA-F]{3,8})(?=\s|$)/.exec(rest)
  if (col) {
    out.color = parseColor(col[1], line)
    rest = rest.replace(col[0], ' ')
  }
  for (const w of ['dashed', 'dotted', 'thick']) {
    const m = new RegExp(`(?:^|\\s)${w}(?=\\s|$)`).exec(rest)
    if (m) {
      if (w === 'dashed') out.dash = true
      else if (w === 'dotted') out.dot = true
      else out.thick = true
      rest = rest.replace(m[0], ' ')
    }
  }
  return { ...out, rest: rest.trim() }
}

function parseAxis(value: string, line: number): PlotAxis {
  let s = value.trim()
  const axis: PlotAxis = { log: false }
  const log = /(?:^|\s)log$/i.exec(s)
  if (log) {
    axis.log = true
    s = s.slice(0, log.index).trim()
  }
  const range = /(-?[\w.+*/()πe-]+?)\s*(?:\.\.|\sto\s)\s*(-?[\w.+*/()π-]+)$/i.exec(s)
  if (range) {
    try {
      axis.domain = [evalConst(range[1]), evalConst(range[2])]
    } catch (e) {
      bad(e, line)
    }
    if (axis.domain![0] >= axis.domain![1]) throw new DslError('An axis range runs from small to large: 0..10.', line)
    s = s.slice(0, range.index).trim()
  }
  const unit = /\[([^\]]*)\]\s*$/.exec(s)
  if (unit) {
    axis.unit = unit[1].trim()
    s = s.slice(0, unit.index).trim()
  }
  if (s) axis.label = s
  return axis
}

function parseRangeText(text: string, line: number): [number, number] {
  const m = /^\s*(.+?)\s*\.\.\s*(.+?)\s*$/.exec(text)
  if (!m) throw new DslError(`Expected a range like 0..2pi, got "${text}".`, line)
  try {
    return [evalConst(m[1]), evalConst(m[2])]
  } catch (e) {
    return bad(e, line)
  }
}

/** Split `a, b` at the top-level comma. */
function splitComma(s: string): string[] {
  let depth = 0
  for (let i = 0; i < s.length; i++) {
    if (s[i] === '(') depth++
    else if (s[i] === ')') depth--
    else if (s[i] === ',' && depth === 0) return [s.slice(0, i).trim(), s.slice(i + 1).trim()]
  }
  return [s.trim()]
}

function parsePoints(rest: string, line: number): { x: number; y: number; xerr?: number; yerr?: number }[] {
  let text = rest
  let defErr: number | undefined
  let defXerr: number | undefined
  const opts = /(^|\s)(err|yerr|xerr)=(\S+)/g
  for (let m = opts.exec(rest); m; m = opts.exec(rest)) {
    const v = parseNumber(m[3], line, 'error size')
    if (m[2] === 'xerr') defXerr = v
    else defErr = v
  }
  text = text.replace(/(^|\s)(err|yerr|xerr)=\S+/g, ' ')
  const tuples: string[] = []
  const paren = /\(([^)]*)\)/g
  for (let m = paren.exec(text); m; m = paren.exec(text)) tuples.push(m[1])
  if (!tuples.length) tuples.push(...text.split(';').map((t) => t.trim()).filter(Boolean))
  if (!tuples.length) throw new DslError('points needs data: points: (1, 2) (2, 4) (3, 5.9)  err=0.2', line)
  return tuples.map((t) => {
    const nums = t.split(/[\s,]+/).filter(Boolean).map((n) => parseNumber(n, line, 'number'))
    if (nums.length < 2 || nums.length > 4) throw new DslError(`A point is (x, y), (x, y, yerr) or (x, y, xerr, yerr); got "${t}".`, line)
    if (nums.length === 4) return { x: nums[0], y: nums[1], xerr: nums[2], yerr: nums[3] }
    return { x: nums[0], y: nums[1], yerr: nums[2] ?? defErr, xerr: defXerr }
  })
}

export function parsePlot(src: string): PlotSpec {
  const spec: PlotSpec = { x: { log: false }, y: { log: false }, grid: true, height: 340, params: [], series: [] }
  const lines: Line[] = scriptLines(src)
  if (!lines.length) throw new DslError('Empty plot: add a line such as fn: sin(x)', 1)
  const names = new Set<string>()
  const pending: { n: number; key: string; value: string }[] = []

  // Parameters first, wherever they appear, so every expression can use them.
  for (const { n, text } of lines) {
    // `param A = 1 0..3` is written without a colon.
    const bare = /^param\s+(.*)$/.exec(text)
    const kv = bare ? { key: 'param', value: bare[1] } : keyValue(text, KEYS)
    if (!kv) throw new DslError(`Cannot read "${text.slice(0, 40)}". Lines look like "fn: sin(x)", "x: Time [s] 0..10", "param A = 1 0..3".`, n)
    if (kv.key !== 'param') {
      pending.push({ n, ...kv })
      continue
    }
    const { rest, label } = styleOf(kv.value, n)
    const m = /^([A-Za-z_]\w*)\s*=?\s*(\S+)\s+(.+?)(?:\s+step\s+(\S+))?$/.exec(rest)
    if (!m) throw new DslError('param needs a name, a start value and a range: param A = 1.5  0..3  step 0.1', n)
    if (RESERVED.has(m[1]) || ['x', 'y', 't', 'theta'].includes(m[1])) throw new DslError(`"${m[1]}" is already a function, constant or variable: choose another name.`, n)
    if (names.has(m[1])) throw new DslError(`Parameter "${m[1]}" is defined twice.`, n)
    names.add(m[1])
    try {
      const [min, max] = parseRangeText(m[3], n)
      const value = evalConst(m[2])
      if (min >= max) throw new DslError('A slider range runs from small to large: 0..3.', n)
      const step = m[4] ? evalConst(m[4]) : (max - min) / 100
      spec.params.push({ name: m[1], value: Math.min(max, Math.max(min, value)), min, max, step, label, line: n })
    } catch (e) {
      bad(e, n)
    }
  }

  const pnames = [...names]
  const compile = (src2: string, vars: string[], line: number): Fn => {
    try {
      return compileExpr(src2, [...vars, ...pnames])
    } catch (e) {
      return bad(e, line)
    }
  }
  for (const { n, key, value } of pending) {
    switch (key) {
      case 'title':
        spec.title = value
        break
      case 'x':
      case 'xaxis':
        spec.x = parseAxis(value, n)
        break
      case 'y':
      case 'yaxis':
        spec.y = parseAxis(value, n)
        break
      case 'grid':
        spec.grid = !/^(off|false|no|none)$/i.test(value)
        break
      case 'height': {
        const h = parseNumber(value, n, 'height in px')
        if (h < 160 || h > 900) throw new DslError('height is in px, 160 to 900.', n)
        spec.height = h
        break
      }
      case 'fn':
      case 'function': {
        const { rest, ...st } = styleOf(value, n)
        // `y = f(x)` and `f(x) = ...` are accepted as well as the bare expression.
        const body = rest.replace(/^\s*(?:y|f\(x\))\s*=\s*/, '')
        spec.series.push({ kind: 'fn', src: body, fn: compile(body, ['x'], n), line: n, ...st })
        break
      }
      case 'polar': {
        const { rest, ...st } = styleOf(value, n)
        const th = /\s+theta\s*=\s*(.+)$/.exec(rest)
        const body = (th ? rest.slice(0, th.index) : rest).replace(/^\s*r\s*=\s*/, '')
        const range = th ? parseRangeText(th[1], n) : ([0, 2 * Math.PI] as [number, number])
        spec.series.push({ kind: 'polar', src: body, fn: compile(body, ['theta'], n), range, line: n, ...st })
        break
      }
      case 'parametric': {
        const { rest, ...st } = styleOf(value, n)
        const tm = /\s+t\s*=\s*(.+)$/.exec(rest)
        const body = tm ? rest.slice(0, tm.index) : rest
        const [xs, ys] = splitComma(body)
        if (!ys) throw new DslError('parametric needs x(t) and y(t) separated by a comma: parametric: cos(t), sin(t)  t=0..2pi', n)
        const range = tm ? parseRangeText(tm[1], n) : ([0, 2 * Math.PI] as [number, number])
        spec.series.push({ kind: 'parametric', xs, ys, fx: compile(xs, ['t'], n), fy: compile(ys, ['t'], n), range, line: n, ...st })
        break
      }
      case 'implicit': {
        const { rest, ...st } = styleOf(value, n)
        const sides = rest.split('=')
        if (sides.length > 2) throw new DslError('An implicit curve has one "=": x^2 + y^2 = 4', n)
        const src2 = sides.length === 2 ? `(${sides[0]}) - (${sides[1]})` : rest
        spec.series.push({ kind: 'implicit', src: rest, fn: compile(src2, ['x', 'y'], n), line: n, ...st })
        break
      }
      case 'field': {
        const { rest, ...st } = styleOf(value, n)
        const dm = /\s+density=(\d+)/.exec(rest)
        const body = dm ? rest.replace(dm[0], '') : rest
        const [a, b] = splitComma(body)
        if (!b) throw new DslError('field needs two components separated by a comma: field: -y, x', n)
        spec.series.push({ kind: 'field', fx: compile(a, ['x', 'y'], n), fy: compile(b, ['x', 'y'], n), density: dm ? Math.min(30, Math.max(4, Number(dm[1]))) : 14, line: n, ...st })
        break
      }
      case 'points':
      case 'data': {
        const { rest, ...st } = styleOf(value, n)
        const connect = /(?:^|\s)line(?=\s|$)/.test(rest)
        spec.series.push({ kind: 'points', pts: parsePoints(rest.replace(/(?:^|\s)line(?=\s|$)/, ' '), n), connect, line: n, ...st })
        break
      }
      case 'hline':
      case 'vline': {
        const { rest, ...st } = styleOf(value, n)
        spec.series.push({ kind: key, at: parseNumber(rest.replace(/^[xy]\s*=\s*/, ''), n, 'position'), line: n, ...st })
        break
      }
    }
  }
  if (!spec.series.length) throw new DslError('Nothing to draw: add fn:, polar:, parametric:, implicit:, field: or points:.', lines[0].n)
  if (spec.x.log && spec.x.domain && spec.x.domain[0] <= 0) throw new DslError('A log x axis needs a positive range, e.g. x: Frequency [Hz] 1..1e5 log', lines[0].n)
  if (spec.y.log && spec.y.domain && spec.y.domain[0] <= 0) throw new DslError('A log y axis needs a positive range, e.g. y: Gain 0.01..100 log', lines[0].n)
  return spec
}

/** The text for an axis label: "Time t (s)". */
export function axisText(a: PlotAxis): string {
  if (!a.label && !a.unit) return ''
  return a.unit ? `${a.label ?? ''} ${a.label ? `(${a.unit})` : `[${a.unit}]`}`.trim() : (a.label ?? '')
}
