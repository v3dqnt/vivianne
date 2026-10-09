import katex from 'katex'
import 'katex/contrib/mhchem'

/**
 * Text in science figures: Greek letters, arrows, sub- and superscripts,
 * and `$...$` maths inside the labels of every ```chem, ```bio, ```plot and
 * ```physics script.
 *
 * Two ways to show a label, because the figures live in two worlds:
 *   - `labelHtml` renders `$...$` with KaTeX, for anything the page draws as
 *     HTML (JSXGraph text, plot axes, an SVG `foreignObject`);
 *   - `plainLabel` rewrites the same text as Unicode, for plain SVG text and
 *     for the copy-as-SVG/PNG export, where a KaTeX font would not travel.
 * A label written either way reads the same in both.
 */

const GREEK: Record<string, string> = {
  alpha: 'α', beta: 'β', gamma: 'γ', delta: 'δ', epsilon: 'ε', varepsilon: 'ε', zeta: 'ζ', eta: 'η', theta: 'θ', vartheta: 'ϑ',
  iota: 'ι', kappa: 'κ', lambda: 'λ', mu: 'μ', nu: 'ν', xi: 'ξ', pi: 'π', varpi: 'ϖ', rho: 'ρ', varrho: 'ϱ', sigma: 'σ',
  varsigma: 'ς', tau: 'τ', upsilon: 'υ', phi: 'φ', varphi: 'ϕ', chi: 'χ', psi: 'ψ', omega: 'ω',
  Gamma: 'Γ', Delta: 'Δ', Theta: 'Θ', Lambda: 'Λ', Xi: 'Ξ', Pi: 'Π', Sigma: 'Σ', Upsilon: 'Υ', Phi: 'Φ', Psi: 'Ψ', Omega: 'Ω',
}

const SYMBOLS: Record<string, string> = {
  to: '→', rightarrow: '→', leftarrow: '←', gets: '←', leftrightarrow: '↔', Rightarrow: '⇒', Leftarrow: '⇐', Leftrightarrow: '⇔',
  rightleftharpoons: '⇌', uparrow: '↑', downarrow: '↓', mapsto: '↦',
  pm: '±', mp: '∓', times: '×', cdot: '·', div: '÷', circ: '°', degree: '°', infty: '∞', approx: '≈', neq: '≠', ne: '≠',
  leq: '≤', le: '≤', geq: '≥', ge: '≥', ll: '≪', gg: '≫', sim: '∼', propto: '∝', equiv: '≡', partial: '∂', nabla: '∇',
  hbar: 'ħ', ell: 'ℓ', angstrom: 'Å', AA: 'Å', sqrt: '√', sum: 'Σ', int: '∫', prod: 'Π', in: '∈', cdots: '⋯', ldots: '…',
  dots: '…', bullet: '•', star: '⋆', oplus: '⊕', ominus: '⊖', perp: '⊥', parallel: '∥', angle: '∠', deg: '°',
  ',': ' ', ';': ' ', ' ': ' ', '!': '',
}

const SUP: Record<string, string> = {
  '0': '⁰', '1': '¹', '2': '²', '3': '³', '4': '⁴', '5': '⁵', '6': '⁶', '7': '⁷', '8': '⁸', '9': '⁹',
  '+': '⁺', '-': '⁻', '−': '⁻', '=': '⁼', '(': '⁽', ')': '⁾', n: 'ⁿ', i: 'ⁱ',
}
const SUB: Record<string, string> = {
  '0': '₀', '1': '₁', '2': '₂', '3': '₃', '4': '₄', '5': '₅', '6': '₆', '7': '₇', '8': '₈', '9': '₉',
  '+': '₊', '-': '₋', '−': '₋', '=': '₌', '(': '₍', ')': '₎', a: 'ₐ', e: 'ₑ', o: 'ₒ', x: 'ₓ', h: 'ₕ', k: 'ₖ', l: 'ₗ',
  m: 'ₘ', n: 'ₙ', p: 'ₚ', s: 'ₛ', t: 'ₜ', i: 'ᵢ', j: 'ⱼ',
}

/** `^{...}` / `^x` or `_{...}` / `_x`, to Unicode where every character has a form. */
function script(text: string, marker: '^' | '_', table: Record<string, string>): string {
  const re = marker === '^' ? /\^(?:\{([^{}]*)\}|([A-Za-z0-9+\-−]))/g : /_(?:\{([^{}]*)\}|([A-Za-z0-9+\-−]))/g
  return text.replace(re, (_whole, braced: string | undefined, bare: string | undefined) => {
    const body = braced ?? bare ?? ''
    const out = [...body].map((c) => table[c])
    return out.every((c) => c !== undefined) ? out.join('') : `${marker}${body}`
  })
}

/** TeX-ish text as plain Unicode: `\alpha` → α, `->` → →, `x^2` → x², `H_2O` → H₂O. */
export function plainText(src: string): string {
  let s = src
  s = s.replace(/\\(?:ce|pu)\{([^{}]*)\}/g, (_m, f: string) => formulaText(f))
  s = s.replace(/\\(?:mathrm|text|mathit|mathbf|textbf|operatorname)\{([^{}]*)\}/g, '$1')
  s = s.replace(/\\([A-Za-z]+|[,; !])/g, (whole, name: string) => GREEK[name] ?? SYMBOLS[name] ?? whole)
  s = s.replace(/<=>|<->|<-->/g, '⇌').replace(/-->|->/g, '→').replace(/<--|<-/g, '←').replace(/=>/g, '⇒')
  s = script(s, '^', SUP)
  s = script(s, '_', SUB)
  return s.replace(/[{}]/g, '')
}

/** A chemical formula with its digits and charges set as sub- and superscripts: `SO4^2-` → SO₄²⁻. */
export function formulaText(f: string): string {
  const withCharge = f.replace(/\^\{?([0-9]*[+\-−])\}?/g, (_m, ch: string) => [...ch].map((c) => SUP[c] ?? c).join(''))
  const arrowed = withCharge.replace(/<=>/g, ' ⇌ ').replace(/->/g, ' → ').replace(/<-/g, ' ← ')
  return arrowed.replace(/([A-Za-z)\]])(\d+)/g, (_m, a: string, d: string) => a + [...d].map((c) => SUB[c] ?? c).join(''))
}

/** A label split into runs of plain text and `$...$` maths. */
export function splitMath(label: string): { math: boolean; text: string }[] {
  const out: { math: boolean; text: string }[] = []
  const re = /(?<!\\)\$([^$]+?)(?<!\\)\$/g
  let last = 0
  for (let m = re.exec(label); m; m = re.exec(label)) {
    if (m.index > last) out.push({ math: false, text: label.slice(last, m.index) })
    out.push({ math: true, text: m[1] })
    last = m.index + m[0].length
  }
  if (last < label.length) out.push({ math: false, text: label.slice(last) })
  return out
}

/** Whether a label carries `$...$`. */
export function hasMath(label: string): boolean {
  return splitMath(label).some((p) => p.math)
}

export function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

/** A label as plain Unicode text for SVG: maths runs are rewritten, `$` dropped. */
export function plainLabel(label: string): string {
  return splitMath(label)
    .map((p) => plainText(p.text))
    .join('')
}

/** A label as HTML: `$...$` rendered by KaTeX (chemistry with `\ce{}` included), the rest escaped. */
export function labelHtml(label: string): string {
  return splitMath(label)
    .map((p) => {
      if (!p.math) return escapeHtml(plainText(p.text))
      try {
        return katex.renderToString(p.text, { throwOnError: false, strict: 'ignore' })
      } catch {
        return escapeHtml(plainText(p.text))
      }
    })
    .join('')
}

export interface SvgTextOpts {
  anchor?: 'start' | 'middle' | 'end'
  size?: number
  fill?: string
  weight?: number | string
  italic?: boolean
  /** Added to the element, e.g. `class="x"`. */
  attrs?: string
}

/**
 * One line of text for an SVG figure. Plain labels become `<text>`. A label
 * with `$...$` becomes a `foreignObject` holding KaTeX, tagged with the plain
 * text so `flattenSvg` can swap it back for the export.
 */
export function svgText(x: number, y: number, label: string, o: SvgTextOpts = {}): string {
  const size = o.size ?? 11
  const anchor = o.anchor ?? 'middle'
  const fill = o.fill ?? 'currentColor'
  const plain = plainLabel(label)
  const common = `font-size="${size}" fill="${fill}"${o.weight ? ` font-weight="${o.weight}"` : ''}${o.italic ? ' font-style="italic"' : ''} ${o.attrs ?? ''}`
  if (!hasMath(label)) {
    return `<text x="${x}" y="${y}" text-anchor="${anchor}" ${common}>${escapeHtml(plain)}</text>`
  }
  const w = Math.max(40, plain.length * size * 0.75 + 20)
  const h = size * 2.4
  const left = anchor === 'middle' ? x - w / 2 : anchor === 'end' ? x - w : x
  const justify = anchor === 'middle' ? 'center' : anchor === 'end' ? 'flex-end' : 'flex-start'
  return (
    `<foreignObject class="sci-math" x="${left}" y="${y - h * 0.68}" width="${w}" height="${h}" data-plain="${escapeHtml(plain).replace(/"/g, '&quot;')}" data-anchor="${anchor}" data-x="${x}" data-y="${y}" data-size="${size}" data-fill="${fill}">` +
    `<div xmlns="http://www.w3.org/1999/xhtml" style="display:flex;justify-content:${justify};align-items:center;height:100%;font-size:${size}px;color:${fill};white-space:nowrap;line-height:1">${labelHtml(label)}</div></foreignObject>`
  )
}

/**
 * An SVG as a standalone string for copy/export: KaTeX `foreignObject`s are
 * replaced by their plain-text form (they would not rasterise), the ink
 * colour is resolved, and the size is made explicit.
 */
export function flattenSvg(svg: SVGSVGElement, ink: string): string {
  const clone = svg.cloneNode(true) as SVGSVGElement
  clone.querySelectorAll('foreignObject.sci-math').forEach((fo) => {
    const t = document.createElementNS('http://www.w3.org/2000/svg', 'text')
    t.setAttribute('x', fo.getAttribute('data-x') ?? '0')
    t.setAttribute('y', fo.getAttribute('data-y') ?? '0')
    t.setAttribute('text-anchor', fo.getAttribute('data-anchor') ?? 'middle')
    t.setAttribute('font-size', fo.getAttribute('data-size') ?? '11')
    t.setAttribute('fill', fo.getAttribute('data-fill') ?? 'currentColor')
    t.textContent = fo.getAttribute('data-plain') ?? ''
    fo.replaceWith(t)
  })
  const vb = clone.viewBox.baseVal
  if (!clone.getAttribute('width') && vb?.width) clone.setAttribute('width', String(vb.width))
  if (!clone.getAttribute('height') && vb?.height) clone.setAttribute('height', String(vb.height))
  clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg')
  clone.setAttribute('color', ink)
  clone.removeAttribute('class')
  clone.setAttribute('style', 'font-family: system-ui, sans-serif;')
  return new XMLSerializer().serializeToString(clone)
}
