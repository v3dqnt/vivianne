/**
 * A small, safe expression language for plots: numbers, names, `+ - * / ^`,
 * parentheses and the usual functions. It is parsed to a tree and compiled
 * to closures — nothing is ever passed to `eval` or `new Function`, which the
 * app's content-security policy forbids anyway.
 */

export class ExprError extends Error {}

type Node =
  | { t: 'num'; v: number }
  | { t: 'var'; name: string }
  | { t: 'neg'; a: Node }
  | { t: 'bin'; op: '+' | '-' | '*' | '/' | '^'; a: Node; b: Node }
  | { t: 'call'; fn: string; args: Node[] }

const fact = (n: number) => {
  let r = 1
  for (let i = 2; i <= Math.min(170, Math.round(n)); i++) r *= i
  return r
}

export const FUNCTIONS: Record<string, (...a: number[]) => number> = {
  sin: Math.sin, cos: Math.cos, tan: Math.tan, asin: Math.asin, acos: Math.acos, atan: Math.atan, atan2: Math.atan2,
  sinh: Math.sinh, cosh: Math.cosh, tanh: Math.tanh, exp: Math.exp, log: Math.log, ln: Math.log, log10: Math.log10, log2: Math.log2,
  sqrt: Math.sqrt, cbrt: Math.cbrt, abs: Math.abs, sign: Math.sign, floor: Math.floor, ceil: Math.ceil, round: Math.round,
  min: Math.min, max: Math.max, pow: Math.pow, mod: (a, b) => ((a % b) + b) % b, fact,
  step: (x) => (x >= 0 ? 1 : 0), sinc: (x) => (x === 0 ? 1 : Math.sin(x) / x),
}
export const CONSTANTS: Record<string, number> = { pi: Math.PI, e: Math.E, tau: Math.PI * 2 }
/** Names that can't be used for parameters. */
export const RESERVED = new Set([...Object.keys(FUNCTIONS), ...Object.keys(CONSTANTS)])

interface Tok {
  k: 'num' | 'id' | 'op' | 'end'
  s: string
  at: number
}

function lex(src: string): Tok[] {
  const out: Tok[] = []
  const s = src.replace(/[−–]/g, '-').replace(/[·×]/g, '*').replace(/π/g, 'pi')
  let i = 0
  while (i < s.length) {
    const c = s[i]
    if (/\s/.test(c)) {
      i++
    } else if (/[0-9.]/.test(c)) {
      const m = /^(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?/.exec(s.slice(i))
      if (!m) throw new ExprError(`Bad number at "${s.slice(i, i + 6)}".`)
      out.push({ k: 'num', s: m[0], at: i })
      i += m[0].length
      if (/[A-Za-z_(]/.test(s[i] ?? '')) throw new ExprError(`Write ${m[0]}*${s[i] === '(' ? '(…)' : 'name'} — a multiplication needs its "*" (found "${m[0]}${s.slice(i, i + 3)}").`)
    } else if (/[A-Za-z_]/.test(c)) {
      const m = /^[A-Za-z_][A-Za-z_0-9]*/.exec(s.slice(i))!
      out.push({ k: 'id', s: m[0], at: i })
      i += m[0].length
    } else if (c === '*' && s[i + 1] === '*') {
      out.push({ k: 'op', s: '^', at: i })
      i += 2
    } else if ('+-*/^(),'.includes(c)) {
      out.push({ k: 'op', s: c, at: i })
      i++
    } else {
      throw new ExprError(`Unexpected character "${c}".`)
    }
  }
  out.push({ k: 'end', s: '', at: s.length })
  return out
}

export function parseExpr(src: string): Node {
  const toks = lex(src)
  let p = 0
  const peek = () => toks[p]
  const eat = (s?: string) => {
    const t = toks[p]
    if (s && t.s !== s) throw new ExprError(`Expected "${s}" but found "${t.s || 'the end of the expression'}".`)
    p++
    return t
  }
  const primary = (): Node => {
    const t = peek()
    if (t.k === 'num') {
      p++
      return { t: 'num', v: Number(t.s) }
    }
    if (t.k === 'id') {
      p++
      if (peek().s === '(') {
        p++
        const args: Node[] = []
        if (peek().s !== ')') {
          for (;;) {
            args.push(expr())
            if (peek().s === ',') p++
            else break
          }
        }
        eat(')')
        return { t: 'call', fn: t.s, args }
      }
      return { t: 'var', name: t.s }
    }
    if (t.s === '(') {
      p++
      const e = expr()
      eat(')')
      return e
    }
    throw new ExprError(t.k === 'end' ? 'The expression ends too early.' : `Unexpected "${t.s}".`)
  }
  const power = (): Node => {
    const base = primary()
    if (peek().s === '^') {
      p++
      return { t: 'bin', op: '^', a: base, b: unary() }
    }
    return base
  }
  const unary = (): Node => {
    if (peek().s === '-') {
      p++
      return { t: 'neg', a: unary() }
    }
    if (peek().s === '+') {
      p++
      return unary()
    }
    return power()
  }
  const term = (): Node => {
    let a = unary()
    while (peek().s === '*' || peek().s === '/') a = { t: 'bin', op: eat().s as '*' | '/', a, b: unary() }
    return a
  }
  const expr = (): Node => {
    let a = term()
    while (peek().s === '+' || peek().s === '-') a = { t: 'bin', op: eat().s as '+' | '-', a, b: term() }
    return a
  }
  const tree = expr()
  if (peek().k !== 'end') throw new ExprError(`Unexpected "${peek().s}" after the expression.`)
  return tree
}

/** The names an expression uses, excluding constants and functions. */
export function freeVars(n: Node, into = new Set<string>()): Set<string> {
  switch (n.t) {
    case 'var':
      if (!(n.name in CONSTANTS)) into.add(n.name)
      break
    case 'neg':
      freeVars(n.a, into)
      break
    case 'bin':
      freeVars(n.a, into)
      freeVars(n.b, into)
      break
    case 'call':
      n.args.forEach((a) => freeVars(a, into))
      break
  }
  return into
}

export type Fn = (vars: Record<string, number>) => number

function build(n: Node): Fn {
  switch (n.t) {
    case 'num':
      return () => n.v
    case 'var': {
      const k = n.name
      if (k in CONSTANTS) {
        const v = CONSTANTS[k]
        return () => v
      }
      return (vars) => vars[k]
    }
    case 'neg': {
      const a = build(n.a)
      return (v) => -a(v)
    }
    case 'bin': {
      const a = build(n.a)
      const b = build(n.b)
      switch (n.op) {
        case '+':
          return (v) => a(v) + b(v)
        case '-':
          return (v) => a(v) - b(v)
        case '*':
          return (v) => a(v) * b(v)
        case '/':
          return (v) => a(v) / b(v)
        default:
          return (v) => {
            const x = a(v)
            const y = b(v)
            // A negative base to a fractional power such as 1/3 is a real cube root.
            if (x < 0 && !Number.isInteger(y)) {
              const inv = 1 / y
              if (Number.isInteger(inv) && inv % 2 !== 0) return -Math.pow(-x, y)
            }
            return Math.pow(x, y)
          }
      }
    }
    case 'call': {
      const f = FUNCTIONS[n.fn]
      const args = n.args.map(build)
      return (v) => f(...args.map((a) => a(v)))
    }
  }
}

/**
 * Compile `src` for the given variable names (plus constants and functions),
 * rejecting any other name with a message that says what to do about it.
 */
export function compileExpr(src: string, allowed: Iterable<string>): Fn {
  const tree = parseExpr(src)
  const ok = new Set(allowed)
  const check = (n: Node) => {
    if (n.t === 'call') {
      if (!(n.fn in FUNCTIONS)) throw new ExprError(`Unknown function "${n.fn}". Available: ${Object.keys(FUNCTIONS).slice(0, 18).join(', ')}.`)
      const want = n.fn === 'atan2' || n.fn === 'pow' || n.fn === 'mod' ? 2 : n.fn === 'min' || n.fn === 'max' ? -1 : 1
      if (want > 0 && n.args.length !== want) throw new ExprError(`${n.fn}() takes ${want} argument${want > 1 ? 's' : ''}.`)
      n.args.forEach(check)
    } else if (n.t === 'bin') {
      check(n.a)
      check(n.b)
    } else if (n.t === 'neg') check(n.a)
  }
  check(tree)
  for (const name of freeVars(tree)) {
    if (!ok.has(name)) {
      const list = [...ok].join(', ')
      throw new ExprError(`Unknown name "${name}". Available here: ${list || 'none'}; define other values with "param ${name} = 1 0..5".`)
    }
  }
  return build(tree)
}

/** A constant expression such as `2*pi` or `-1.5`. */
export function evalConst(src: string): number {
  const v = compileExpr(src.replace(/(\d)\s*(pi|π)(?![A-Za-z])/g, '$1*pi'), [])({})
  if (!Number.isFinite(v)) throw new ExprError(`"${src}" is not a finite number.`)
  return v
}
