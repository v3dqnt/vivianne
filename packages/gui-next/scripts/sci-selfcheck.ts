/**
 * Self-check for the science parsers, the renderers and the `check_diagram`
 * verdicts. The package has no test runner, so this is a plain script.
 *
 *   cd packages/gui-next && npx tsx scripts/sci-selfcheck.ts
 */
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { parseChem } from '../src/modules/ai/components/sci/chemDsl'
import { renderChem } from '../src/modules/ai/components/sci/chemRender'
import { DslError } from '../src/modules/ai/components/sci/dslCommon'
import { parseBio, parseNewick } from '../src/modules/ai/components/sci/bioDsl'
import { parsePlot } from '../src/modules/ai/components/sci/plotDsl'
import { compilePhysics } from '../src/modules/ai/components/sci/physicsDsl'
import { parseMol3D } from '../src/modules/ai/components/sci/mol3dDsl'
import { compileExpr } from '../src/modules/ai/components/sci/mathExpr'
import { checkScript } from '../src/modules/ai/components/sci/check'
import { plainText, formulaText, labelHtml } from '../src/modules/ai/components/sci/symbols'
import { plasmidSvg, sequenceSvg, translate, treeSvg } from '../src/modules/ai/components/sci/bioRender'
import { cellSvg } from '../src/modules/ai/components/sci/bioCell'
import { sceneToSvg } from '../src/modules/ai/components/sci/physicsSvg'

const require = createRequire(import.meta.url)
let passed = 0
const test = async (name: string, fn: () => void | Promise<void>) => {
  try {
    await fn()
    passed++
    console.log(`ok   ${name}`)
  } catch (e) {
    console.error(`FAIL ${name}\n     ${(e as Error).stack}`)
    process.exitCode = 1
  }
}
const colors = { ink: '#111111', muted: '#666666', line: '#dddddd', paper: '#ffffff', dark: false, accents: ['#f5b400', '#e34948'] }

const rd = await require('@rdkit/rdkit')()
rd.prefer_coordgen(true)

const throwsAt = (fn: () => unknown, line: number, re?: RegExp) =>
  assert.throws(fn, (e: unknown) => e instanceof DslError && e.line === line && (!re || re.test(e.message)))

// ── chemistry ───────────────────────────────────────────────────────────

await test('chem: aspirin renders with formula and weight', () => {
  const r = renderChem(rd, parseChem('name: Aspirin\nCC(=O)Oc1ccccc1C(=O)O\nhighlight: atoms 0-3'), colors)
  assert.equal(r.molecules[0].formula, 'C₉H₈O₄')
  assert.ok(Math.abs(r.molecules[0].mw - 180.16) < 0.01)
  assert.ok(r.svg.includes('<svg') && r.svg.includes('C₉H₈O₄'))
})
await test('chem: unclosed ring is rejected with its line', () => {
  assert.throws(() => renderChem(rd, parseChem('\nC1CC'), colors), (e: unknown) => e instanceof DslError && e.line === 2 && /ring/i.test(e.message))
})
await test('chem: reaction scheme with conditions', () => {
  const r = renderChem(rd, parseChem('CC(=O)O.OCC>>CC(=O)OCC.O\nconditions: "H2SO4, Δ / reflux"'), colors)
  assert.equal(r.molecules.length, 4)
  assert.ok(r.svg.includes('reflux'))
})
await test('chem: multi-step mechanism and a grid of molecules', () => {
  const r = renderChem(rd, parseChem('CC(=O)O>>CC(=O)[OH2+]>>CC(O)(O)OC\nconditions: "H+" "CH3OH"'), colors)
  assert.equal(r.molecules.length, 3)
  const g = renderChem(rd, parseChem('grid: 2\nCCO ethanol\nCCN ethylamine\nc1ccccc1 benzene'), colors)
  assert.equal(g.molecules.length, 3)
})
await test('chem: bad option name, SMARTS highlight, molfile', () => {
  throwsAt(() => parseChem('colour: red\nCCO'), 1, /Unknown option/)
  assert.ok(renderChem(rd, parseChem('CC(=O)O\nhighlight: smarts C(=O)O #e34948'), colors).svg.length > 100)
  throwsAt(() => renderChem(rd, parseChem('CCO\nhighlight: atoms 0-9'), colors), 2, /doesn't exist/)
  const mol = rd.get_mol('CCO').get_molblock()
  assert.equal(renderChem(rd, parseChem(mol), colors).molecules[0].formula, 'C₂H₆O')
})
await test('chem: stereo and charges survive', () => {
  const r = renderChem(rd, parseChem('C[C@H](N)C(=O)O L-alanine\n[Na+].[Cl-]'), colors)
  assert.ok(r.molecules[0].smiles.includes('@'))
  assert.equal(r.molecules[1].formula, 'ClNa')
})

// ── symbols ─────────────────────────────────────────────────────────────

await test('symbols: Greek, arrows and scripts become plain text', () => {
  assert.equal(plainText('\\alpha -> \\beta, x^2, H_2O'), 'α → β, x², H₂O')
  assert.equal(formulaText('SO4^2-'), 'SO₄²⁻')
  assert.ok(labelHtml('$\\ce{H2O}$ and $\\omega$').includes('katex'))
})

// ── biology ─────────────────────────────────────────────────────────────

await test('bio: sequence with features and translation', () => {
  const spec = parseBio('sequence\nseq: ATGGCCATTGTAATGGGCCGC\nfeature: 1-9 "start" #3987e5\ntranslate: 1')
  assert.equal(spec.kind, 'sequence')
  assert.equal(translate('ATGGCCATTGTAATGGGCCGC'), 'MAIVMGR')
  assert.ok(sequenceSvg(spec as never, colors).includes('<svg'))
})
await test('bio: errors carry line numbers', () => {
  throwsAt(() => parseBio('sequence\nseq: ATGC\nfeature: 3-90 "x"'), 3, /sequence is 4 long/)
  throwsAt(() => parseBio('sequence\ntype: dna\nseq: ATGJ'), 3, /not a nucleotide/)
  throwsAt(() => parseBio('plasmid X\nsize: 100\nfeature: 1..500 "f"'), 3, /outside/)
  throwsAt(() => parseBio('tree\nnewick: ((A,B);'), 2, /closed|Expected/)
  throwsAt(() => parseBio('banana'), 1, /first line/)
  throwsAt(() => parseBio('pathway\nnode a protein "A"\na activates zz'), 3, /not a declared node/)
})
await test('bio: newick parses names, lengths and support values', () => {
  const t = parseNewick('((A:0.1,B:0.2)90:0.3,C:0.5);', 1)
  assert.equal(t.children.length, 2)
  assert.equal(t.children[0].name, '90')
  assert.equal(t.children[0].children[1].length, 0.2)
})
await test('bio: plasmid, tree and cell render', () => {
  assert.ok(plasmidSvg(parseBio('plasmid pUC19\nsize: 2686\nfeature: 146..469 "lacZ" cw\nsite: EcoRI 396') as never, colors).includes('EcoRI'))
  assert.ok(treeSvg(parseBio('tree\n((A:1,B:1):1,C:2);\nhighlight: A, B "AB"') as never, colors).includes('AB'))
  assert.ok(treeSvg(parseBio('tree\nlayout: circular\n((A:1,B:1):1,C:2);') as never, colors).includes('<svg'))
  assert.ok(cellSvg(parseBio('cell plant\nhighlight: chloroplast') as never, colors).includes('Chloroplast'))
  throwsAt(() => parseBio('cell animal\nshow: chloroplast'), 2, /not drawn/)
})
await test('bio: pathway lays out with elkjs', async () => {
  const { layoutPathway, pathwaySvg } = await import('../src/modules/ai/components/sci/bioPathway')
  const spec = parseBio('pathway Test\ncompartment cyt "Cytosol"\nnode a metabolite "A" in cyt\nnode b metabolite "B" in cyt\nnode E protein "E" in cyt\na -> b by E\nE inhibits E') as never
  const lay = await layoutPathway(spec)
  assert.ok(lay.edges.length >= 3 && lay.edges.every((e) => e.points.length >= 2))
  assert.ok(pathwaySvg(spec, lay, colors).includes('Cytosol'))
})

// ── plots ───────────────────────────────────────────────────────────────

await test('plot: parses axes, sliders, series and data', () => {
  const spec = parsePlot(
    'title: T\nx: Time t [s] 0..12\ny: x [m] -2..2\nparam A = 1.5 0..3 step 0.1\nfn: A*sin(x) "wave" #3987e5 dashed\npolar: 1+cos(theta)\nparametric: cos(t), sin(2*t) t=0..2pi\nimplicit: x^2+y^2=4\nfield: -y, x\npoints: (1,2) (2,4,0.3) err=0.1',
  )
  assert.equal(spec.series.length, 6)
  assert.deepEqual(spec.x.domain, [0, 12])
  assert.equal(spec.params[0].name, 'A')
  assert.equal(spec.x.unit, 's')
})
await test('plot: errors carry line numbers', () => {
  throwsAt(() => parsePlot('fn: sin(x)\nfn: 2x'), 2, /multiplication/)
  throwsAt(() => parsePlot('fn: w*x'), 1, /Unknown name "w"/)
  throwsAt(() => parsePlot('x: T 5..1\nfn: x'), 1, /small to large/)
  throwsAt(() => parsePlot('title: only'), 1, /Nothing to draw/)
})
await test('plot: expressions evaluate without eval', () => {
  assert.equal(compileExpr('2*x^2 - sin(pi/2)', ['x'])({ x: 3 }), 17)
  assert.equal(compileExpr('-x^2', ['x'])({ x: 3 }), -9)
})

// ── physics ─────────────────────────────────────────────────────────────

await test('physics: every kind compiles and exports', () => {
  const kinds: Record<string, string> = {
    mech: 'mechanics\nramp 30deg "θ"\nblock m on ramp s=4 "m"\nforce m down 6 "mg"\nforce m normal 5 "N"',
    spring: 'mechanics\nwall 0\nblock m at (4,0.6) "m"\nspring (0,0.6) m "k"\nfloor',
    pulley: 'mechanics\npulley P at (0,6)\nblock a at (-0.6,3) "m1"\nblock b at (0.6,2) "m2"\nrope a P b',
    optics: 'optics\nlens L at 0 f=8\nobject at -20 h=3',
    mirror: 'optics\nmirror M at 0 concave f=8\nobject at -20 h=3',
    snell: 'optics\ninterface I at 0 n1=1 n2=1.5\nray (-5,-3) angle=30',
    field: 'field\ncharge a +1 at (-2,0)\ncharge b -1 at (2,0)\nequipotentials on',
    circuit: 'circuit\nbattery V (0,0) (0,4) "9 V"\nresistor R (0,4) (4,4)\ncapacitor C (4,4) (4,0)\ndiode D (4,0) (0,0)\nopamp U at (8,2)\nground (0,0)',
    wave: 'wave\nwave w1 A=1 lambda=2\nwave w2 A=1 lambda=3\nsum w1 w2\nannotate',
    standing: 'wave\nstanding n=3 L=6',
    levels: 'levels\nhydrogen n=1..4\ntransition 3 -> 2',
  }
  for (const [name, src] of Object.entries(kinds)) {
    const scene = compilePhysics(src)
    assert.ok(scene.prims.length > 0, name)
    assert.ok(sceneToSvg(scene, colors).includes('<svg'), name)
  }
})
await test('physics: lens equation gives the image', () => {
  const scene = compilePhysics('optics\nlens L at 0 f=10\nobject at -30 h=3')
  assert.ok(scene.notes[0].includes('real') && scene.notes[0].includes('15.00'), scene.notes[0])
})
await test('physics: errors carry line numbers', () => {
  throwsAt(() => compilePhysics('mechanics\nblock m at (0,0)\nforce q up 3'), 3, /not a defined object/)
  throwsAt(() => compilePhysics('circuit\nresistor R (0,0)'), 2, /between two points/)
  throwsAt(() => compilePhysics('optics\nlens L at 0'), 2, /focal length/)
  throwsAt(() => compilePhysics('gravy'), 1, /kind/)
})

// ── 3D ──────────────────────────────────────────────────────────────────

await test('mol3d: parses ids, styles and inline data', () => {
  const a = parseMol3D('pdb: 1crn\nstyle: cartoon color=spectrum\nsurface: opacity=0.5 chain=A\nlabel: resi=10 "Cys10"\nspin: on')
  assert.equal(a.source.type === 'rcsb' && a.source.id, '1CRN')
  assert.equal(a.surfaces.length, 1)
  assert.equal(parseMol3D('sdf:\n\n  M  V2000\nM  END').source.type, 'inline')
  throwsAt(() => parseMol3D('pdb: ABC'), 1, /PDB id/)
  throwsAt(() => parseMol3D('style: cartoon'), 1, /No structure/)
})

// ── check_diagram ───────────────────────────────────────────────────────

await test('check: the model-facing verdicts', async () => {
  const bad = await checkScript('chem', 'C1CC', rd)
  assert.ok(bad.startsWith('INVALID (line 1)') && /ring/i.test(bad), bad)
  const ok = await checkScript('chem', 'CC(=O)Oc1ccccc1C(=O)O', rd)
  assert.ok(ok.startsWith('OK') && ok.includes('C₉H₈O₄'), ok)
  assert.ok((await checkScript('physics', 'circuit\nresistor R (0,0)')).startsWith('INVALID (line 2)'))
  assert.ok((await checkScript('plot', 'x: x -5..-1\nfn: log(x)')).includes('WARNING'))
  assert.ok((await checkScript('bio', 'sequence\nseq: ATGGCC')).startsWith('OK'))
})

// ── the guides' own examples ────────────────────────────────────────────

await test('guides: every example in science/*.md checks out', async () => {
  const dir = new URL('../../vivianne-guides/science/', import.meta.url)
  const { readdirSync, readFileSync } = await import('node:fs')
  const { fileURLToPath } = await import('node:url')
  let n = 0
  for (const file of readdirSync(fileURLToPath(dir)).filter((f) => f.endsWith('.md'))) {
    const lines = readFileSync(new URL(file, dir), 'utf8').split('\n')
    for (let i = 0; i < lines.length; i++) {
      const m = /^```(chem|mol3d|bio|plot|physics)\s*$/.exec(lines[i])
      if (!m) continue
      const end = lines.findIndex((l, j) => j > i && l.startsWith('```'))
      const code = lines.slice(i + 1, end).join('\n')
      const out = await checkScript(m[1] as 'chem', code, rd)
      assert.ok(out.startsWith('OK'), `${file}:${i + 1} (${m[1]})\n${out}`)
      n++
    }
  }
  assert.ok(n >= 20, `only ${n} examples found`)
})

console.log(`${passed} passed`)
