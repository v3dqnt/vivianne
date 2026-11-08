// Finds, and with --fix corrects, every place a component steps off the design
// scale (src/ui/scale.ts): font sizes, weights, corner radii, control heights.
//
//   node scripts/design-scale.mjs          list what is off the scale
//   node scripts/design-scale.mjs --fix    rewrite it onto the scale
//
// `src/ui/scale.test.ts` runs `scan` and fails when anything is off, so a new
// component that invents a 12.5px size is caught the day it is written.

import { readdirSync, readFileSync, writeFileSync, statSync } from 'node:fs'
import { join, relative, sep } from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { snapFontSize, snapRadius, snapControlHeight, snapWeight, pxOf, toRem, ROUND_RADII } from '../src/ui/scale.ts'

const ts = createRequire(import.meta.url)('typescript')
const SRC = fileURLToPath(new URL('../src', import.meta.url))

/** Surfaces with their own look (the floating notch and desktop-control overlays, brand art, 3D/chart drawing code) and the theme, whose numbers are the scale's own source. */
const SKIP = [`${sep}modules${sep}instant${sep}`, `${sep}modules${sep}dip${sep}`, `${sep}ui${sep}brand${sep}`, `${sep}ui${sep}scale.ts`, `${sep}ui${sep}theme.ts`, `${sep}sci${sep}`, '.test.']

export function sourceFiles(dir = SRC, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) sourceFiles(p, out)
    else if (/\.(tsx|ts)$/.test(name) && !SKIP.some((s) => p.includes(s))) out.push(p)
  }
  return out
}

const propName = (p) => (p.name && (ts.isIdentifier(p.name) || ts.isStringLiteral(p.name)) ? p.name.text : null)
const literal = (n) => (n && (ts.isNumericLiteral(n) ? Number(n.text) : ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n) ? n.text : undefined))

/** What a Typography `variant` sets, in px (the theme's typography, see ui/theme.ts). */
const VARIANT_PX = { caption: 12, body2: 13, subtitle2: 13, body1: 14, button: 14, subtitle1: 15, h6: 16, h5: 18, h4: 20, overline: 11 }

/** The size an `sx` object inherits from its element's `variant`, or null. */
function variantSize(node) {
  const expr = node.parent
  if (!expr || !ts.isJsxExpression(expr)) return null
  const attr = expr.parent
  if (!attr || !ts.isJsxAttribute(attr) || attr.name.text !== 'sx') return null
  const el = attr.parent?.parent
  if (!el) return null
  const variant = el.attributes?.properties?.find((a) => ts.isJsxAttribute(a) && a.name.text === 'variant')
  const v = variant?.initializer && ts.isStringLiteral(variant.initializer) ? variant.initializer.text : null
  return v && VARIANT_PX[v] ? VARIANT_PX[v] : null
}

/** The edits that put one file's text on the scale: [{ start, end, text, rule, from, to }]. */
export function scan(text, fileName = 'x.tsx') {
  const sf = ts.createSourceFile(fileName, text, ts.ScriptTarget.Latest, true, fileName.endsWith('x') ? ts.ScriptKind.TSX : ts.ScriptKind.TS)
  const edits = []
  const visit = (node) => {
    if (ts.isObjectLiteralExpression(node)) {
      const props = new Map()
      for (const p of node.properties) if (ts.isPropertyAssignment(p) && propName(p)) props.set(propName(p), p)
      const sizeProp = props.get('fontSize')
      const sizePx = sizeProp ? pxOf(literal(sizeProp.initializer) ?? '') : variantSize(node)
      const edit = (p, to, rule, from) => {
        const init = p.initializer
        const out = typeof to === 'number' ? String(to) : `'${to}'`
        if (text.slice(init.getStart(sf), init.getEnd()) !== out) edits.push({ start: init.getStart(sf), end: init.getEnd(), text: out, rule, from, to })
      }

      if (sizeProp && sizePx !== null && sizePx >= 8 && typeof literal(sizeProp.initializer) !== 'undefined') {
        edit(sizeProp, toRem(snapFontSize(sizePx)), 'fontSize', literal(sizeProp.initializer))
      }

      const weightProp = props.get('fontWeight')
      const w = weightProp && literal(weightProp.initializer)
      if (typeof w === 'number' || (typeof w === 'string' && /^\d+$/.test(w))) {
        const transform = props.get('textTransform') && literal(props.get('textTransform').initializer)
        const next = snapWeight(Number(w), sizePx, transform === 'uppercase')
        edit(weightProp, next, 'fontWeight', Number(w))
      }

      const radiusProp = props.get('borderRadius')
      const r = radiusProp && literal(radiusProp.initializer)
      if (typeof r === 'number' && r > 0 && r <= 3) {
        // A multiple of the user's radius setting (10px by default): snap in px, write back as a multiple.
        const to = snapRadius(r * 10) / 10
        if (to !== r) edit(radiusProp, to, 'borderRadius', r)
      } else if (typeof r === 'string' && !ROUND_RADII.has(r)) {
        const px = /^\d+(\.\d+)?px$/.test(r) ? parseFloat(r) : null
        if (px !== null) {
          const to = snapRadius(px)
          if (to !== px) edit(radiusProp, `${to}px`, 'borderRadius', r)
        }
      }

      // Heights only where the object styles a control (a flex row, a padded box), never a bar or a dot.
      const controlish = ['display', 'alignItems', 'justifyContent', 'px', 'borderRadius', 'gap'].some((k) => props.has(k))
      for (const k of ['height', 'minHeight']) {
        const hp = props.get(k)
        const h = hp && literal(hp.initializer)
        if (controlish && typeof h === 'number') {
          const to = snapControlHeight(h)
          if (to !== h) edit(hp, to, k, h)
        }
      }
    }
    ts.forEachChild(node, visit)
  }
  visit(sf)
  return edits.sort((a, b) => a.start - b.start)
}

export function apply(text, edits) {
  let out = text
  for (const e of [...edits].sort((a, b) => b.start - a.start)) out = out.slice(0, e.start) + e.text + out.slice(e.end)
  return out
}

/** Everything off the scale across the source: { file, rule, from, to }[]. */
export function scanAll() {
  const found = []
  for (const file of sourceFiles()) {
    for (const e of scan(readFileSync(file, 'utf8'), file)) found.push({ file: relative(SRC, file), rule: e.rule, from: e.from, to: e.to })
  }
  return found
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const fix = process.argv.includes('--fix')
  const counts = {}
  let files = 0
  for (const file of sourceFiles()) {
    const text = readFileSync(file, 'utf8')
    const edits = scan(text, file)
    if (!edits.length) continue
    files++
    for (const e of edits) counts[e.rule] = (counts[e.rule] ?? 0) + 1
    if (fix) writeFileSync(file, apply(text, edits))
  }
  console.log(`${fix ? 'Fixed' : 'Off the scale'}: ${Object.values(counts).reduce((a, b) => a + b, 0)} in ${files} files`, counts)
}
