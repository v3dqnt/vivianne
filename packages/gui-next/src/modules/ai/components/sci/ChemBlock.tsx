import { useEffect, useState } from 'react'
import { SciBlock, useSettled, type SciError } from './SciBlock'
import { SvgFigure, useSciColors, useSvgFigure } from './SvgFigure'
import { DslError } from './dslCommon'
import { parseChem } from './chemDsl'

/**
 * A ```chem fence: structures, reactions and mechanisms drawn by RDKit. The
 * 7MB WebAssembly build loads the first time one appears.
 */
export function ChemBlock({ code }: { code: string }) {
  const source = useSettled(code)
  const colors = useSciColors()
  const [svg, setSvg] = useState<string | null>(null)
  const [error, setError] = useState<SciError | null>(null)
  const fig = useSvgFigure(colors.ink)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const spec = parseChem(source)
        const [{ loadRDKit }, { renderChem }] = await Promise.all([import('./rdkit'), import('./chemRender')])
        const rd = await loadRDKit()
        const out = renderChem(rd, spec, colors)
        if (cancelled) return
        setSvg(out.svg)
        setError(null)
      } catch (e) {
        if (cancelled) return
        setError(e instanceof DslError ? { message: e.message, line: e.line } : { message: e instanceof Error ? e.message : String(e) })
      }
    })()
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, colors.ink, colors.dark])

  return (
    <SciBlock
      label="Chemistry"
      source={code}
      error={error}
      loading={!svg && !error}
      getSvg={fig.getSvg}
      hint="hover a molecule for formula and weight"
      render={({ large }) => (svg ? <SvgFigure svg={svg} large={large} holder={large ? undefined : fig.ref} /> : null)}
    />
  )
}
