import { useEffect, useState } from 'react'
import { SciBlock, useSettled, type SciError } from './SciBlock'
import { SvgFigure, useSciColors, useSvgFigure } from './SvgFigure'
import { DslError } from './dslCommon'
import { parseBio, type BioSpec } from './bioDsl'
import { drawBio } from './bioDraw'

const LABEL: Record<BioSpec['kind'], string> = { sequence: 'Sequence', plasmid: 'Plasmid map', tree: 'Phylogeny', pathway: 'Pathway', cell: 'Cell' }

/** A ```bio fence: sequences, plasmid maps, phylogenies, pathways and cells, drawn as SVG. */
export function BioBlock({ code }: { code: string }) {
  const source = useSettled(code)
  const colors = useSciColors()
  const [svg, setSvg] = useState<string | null>(null)
  const [kind, setKind] = useState<BioSpec['kind'] | null>(null)
  const [error, setError] = useState<SciError | null>(null)
  const fig = useSvgFigure(colors.ink)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const spec = parseBio(source)
        const out = await drawBio(spec, colors)
        if (cancelled) return
        setKind(spec.kind)
        setSvg(out)
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
      label={kind ? LABEL[kind] : 'Biology'}
      source={code}
      error={error}
      loading={!svg && !error}
      getSvg={fig.getSvg}
      render={({ large }) => (svg ? <SvgFigure svg={svg} large={large} holder={large ? undefined : fig.ref} /> : null)}
    />
  )
}
