/**
 * The arithmetic an interactive answer can do: numbers, the inputs and
 * values it defines, + - * / % ^, comparisons, `and`/`or`/`not`,
 * `cond ? a : b`, and a fixed set of functions. A small recursive-descent
 * parser — never `eval` — so text from a model can't run code.
 */

export type Scope = Record<string, number | string | boolean>

const FUNCS: Record<string, (...a: number[]) => number> = {
  min: Math.min,
  max: Math.max,
  abs: Math.abs,
  round: (x, d = 0) => Math.round(x * 10 ** d) / 10 ** d,
  floor: Math.floor,
  ceil: Math.ceil,
  sqrt: Math.sqrt,
  pow: Math.pow,
  log: Math.log,
  log10: Math.log10,
  exp: Math.exp,
  sin: Math.sin,
  cos: Math.cos,
  tan: Math.tan,
  atan2: Math.atan2,
  clamp: (x, lo, hi) => Math.min(hi, Math.max(lo, x)),
  // Payment per period on a loan: rate per period, periods, principal.
  pmt: (r, n, pv) => (r === 0 ? pv / n : (pv * r) / (1 - (1 + r) ** -n)),
  // Future value of regular payments plus a starting sum.
  fv: (r, n, pmt, pv = 0) => (r === 0 ? pv + pmt * n : pv * (1 + r) ** n + (pmt * ((1 + r) ** n - 1)) / r),
}
const CONSTS: Record<string, number> = { pi: Math.PI, e: Math.E }

type Tok = { t: 'num'; v: number } | { t: 'str'; v: string } | { t: 'id'; v: string } | { t: 'op'; v: string }

const lexed = new Map<string, Tok[]>()

function lex(src: string): Tok[] {
  const hit = lexed.get(src)
  if (hit) return hit
  const toks = lexUncached(src)
  // ponytail: cleared wholesale; an LRU if specs ever hold thousands of expressions.
  if (lexed.size > 500) lexed.clear()
  lexed.set(src, toks)
  return toks
}

function lexUncached(src: string): Tok[] {
  const out: Tok[] = []
  let i = 0
  while (i < src.length) {
    const c = src[i]
    if (/\s/.test(c)) {
      i++
    } else if (/[0-9.]/.test(c)) {
      const m = /^(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/i.exec(src.slice(i))
      if (!m) throw new Error(`Bad number at ${i}`)
      out.push({ t: 'num', v: Number(m[0]) })
      i += m[0].length
    } else if (/[A-Za-z_]/.test(c)) {
      const m = /^[A-Za-z_][A-Za-z0-9_]*/.exec(src.slice(i))!
      out.push({ t: 'id', v: m[0] })
      i += m[0].length
    } else if (c === '"' || c === "'") {
      const end = src.indexOf(c, i + 1)
      if (end < 0) throw new Error('Unclosed string')
      out.push({ t: 'str', v: src.slice(i + 1, end) })
      i = end + 1
    } else {
      const two = src.slice(i, i + 2)
      if (['<=', '>=', '==', '!=', '&&', '||', '**'].includes(two)) {
        out.push({ t: 'op', v: two === '**' ? '^' : two })
        i += 2
      } else if ('+-*/%^()<>?:,!'.includes(c)) {
        out.push({ t: 'op', v: c })
        i++
      } else {
        throw new Error(`Unexpected "${c}"`)
      }
    }
  }
  return out
}

type V = number | string | boolean
const num = (v: V): number => (typeof v === 'number' ? v : typeof v === 'boolean' ? (v ? 1 : 0) : Number(v))
const truthy = (v: V) => (typeof v === 'string' ? v.length > 0 : num(v) !== 0)

/** Evaluate `src` against `scope`. Throws a readable error on bad input. */
export function evaluate(src: string, scope: Scope = {}): V {
  const toks = lex(src)
  let p = 0
  const peek = () => toks[p]
  const isOp = (v: string) => peek()?.t === 'op' && peek()!.v === v
  const isId = (v: string) => peek()?.t === 'id' && peek()!.v === v
  const expect = (v: string) => {
    if (!isOp(v)) throw new Error(`Expected "${v}"`)
    p++
  }

  const ternary = (): V => {
    const c = or()
    if (isOp('?')) {
      p++
      const a = ternary()
      expect(':')
      const b = ternary()
      return truthy(c) ? a : b
    }
    return c
  }
  const or = (): V => {
    let l = and()
    while (isOp('||') || isId('or')) {
      p++
      const r = and()
      l = truthy(l) || truthy(r)
    }
    return l
  }
  const and = (): V => {
    let l = cmp()
    while (isOp('&&') || isId('and')) {
      p++
      const r = cmp()
      l = truthy(l) && truthy(r)
    }
    return l
  }
  const cmp = (): V => {
    let l = add()
    while (peek()?.t === 'op' && ['<', '>', '<=', '>=', '==', '!='].includes(String(peek()!.v))) {
      const op = toks[p++].v
      const r = add()
      if (op === '==') l = l === r || num(l) === num(r)
      else if (op === '!=') l = !(l === r || num(l) === num(r))
      else {
        const a = num(l)
        const b = num(r)
        l = op === '<' ? a < b : op === '>' ? a > b : op === '<=' ? a <= b : a >= b
      }
    }
    return l
  }
  const add = (): V => {
    let l = mul()
    while (isOp('+') || isOp('-')) {
      const op = toks[p++].v
      const r = mul()
      l = op === '+' ? (typeof l === 'string' || typeof r === 'string' ? `${l}${r}` : num(l) + num(r)) : num(l) - num(r)
    }
    return l
  }
  const mul = (): V => {
    let l = unary()
    while (isOp('*') || isOp('/') || isOp('%')) {
      const op = toks[p++].v
      const r = num(unary())
      l = op === '*' ? num(l) * r : op === '/' ? num(l) / r : num(l) % r
    }
    return l
  }
  const unary = (): V => {
    if (isOp('-')) {
      p++
      return -num(unary())
    }
    if (isOp('+')) {
      p++
      return num(unary())
    }
    if (isOp('!') || isId('not')) {
      p++
      return !truthy(unary())
    }
    return pow()
  }
  const pow = (): V => {
    const base = atom()
    if (isOp('^')) {
      p++
      return num(base) ** num(unary())
    }
    return base
  }
  const atom = (): V => {
    const t = toks[p++]
    if (!t) throw new Error('Unexpected end')
    if (t.t === 'num' || t.t === 'str') return t.v
    if (t.t === 'op' && t.v === '(') {
      const v = ternary()
      expect(')')
      return v
    }
    if (t.t === 'id') {
      if (t.v === 'true' || t.v === 'false') return t.v === 'true'
      if (isOp('(')) {
        const f = Object.hasOwn(FUNCS, t.v) ? FUNCS[t.v] : undefined
        if (!f) throw new Error(`Unknown function ${t.v}()`)
        p++
        const args: number[] = []
        if (!isOp(')')) {
          args.push(num(ternary()))
          while (isOp(',')) {
            p++
            args.push(num(ternary()))
          }
        }
        expect(')')
        return f(...args)
      }
      if (Object.hasOwn(scope, t.v)) return scope[t.v]
      if (Object.hasOwn(CONSTS, t.v)) return CONSTS[t.v]
      throw new Error(`Unknown name "${t.v}"`)
    }
    throw new Error(`Unexpected "${t.v}"`)
  }

  const v = ternary()
  if (p < toks.length) throw new Error(`Unexpected "${toks[p].v}"`)
  return v
}

/** `{expr}` inside text, replaced by its formatted value. */
export function interpolate(text: string, scope: Scope, fmt: (v: V) => string = (v) => String(v)): string {
  return text.replace(/\{([^{}]+)\}/g, (whole, expr: string) => {
    try {
      return fmt(evaluate(expr, scope))
    } catch {
      return whole
    }
  })
}
