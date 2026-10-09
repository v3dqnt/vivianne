import { useEffect, useMemo, useRef, useState } from 'react'
import { Box, Typography } from '../../../../ui'
import { CHAT_FONT } from '../../lib/chatStyle'
import { SciBlock, useSettled, type SciError } from './SciBlock'
import { useSciColors } from './SvgFigure'
import { DslError, type SciColors } from './dslCommon'
import { parseMol3D, type Mol3DSpec } from './mol3dDsl'
import type { Mol3DHandle } from './mol3dDraw'

const HEIGHT = 380

function Viewer({ spec, colors, handle }: { spec: Mol3DSpec; colors: SciColors; handle: React.MutableRefObject<Mol3DHandle | null> }) {
  const host = useRef<HTMLDivElement | null>(null)
  const [status, setStatus] = useState<'loading' | 'ready' | string>('loading')

  useEffect(() => {
    const el = host.current
    if (!el) return
    let cancelled = false
    // One element per run: a superseded run can never clear the current viewer.
    const root = document.createElement('div')
    root.style.cssText = `position:relative;width:100%;height:${HEIGHT}px`
    el.replaceChildren(root)
    let mine: Mol3DHandle | null = null
    setStatus('loading')
    void (async () => {
      try {
        const draw = await import('./mol3dDraw')
        const data = spec.source.type === 'rcsb' ? await draw.fetchPdb(spec.source.id) : spec.source.data
        if (cancelled) return
        mine = await draw.drawMol3D(root, spec, colors, data)
        if (cancelled) {
          mine.destroy()
          return
        }
        handle.current = mine
        setStatus('ready')
      } catch (e) {
        if (!cancelled) setStatus(e instanceof Error ? e.message : String(e))
      }
    })()
    return () => {
      cancelled = true
      mine?.destroy()
      if (handle.current === mine) handle.current = null
      root.remove()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [spec, colors.dark, colors.ink])

  return (
    <Box sx={{ position: 'relative', height: HEIGHT }}>
      <Box ref={host} sx={{ height: HEIGHT }} />
      {status !== 'ready' && (
        <Box sx={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', px: 2, textAlign: 'center', pointerEvents: 'none' }}>
          <Typography sx={{ fontSize: CHAT_FONT.meta }} color={status === 'loading' ? 'text.secondary' : 'error.main'}>
            {status === 'loading' ? (spec.source.type === 'rcsb' ? `Fetching ${spec.source.id} from RCSB…` : 'Loading…') : status}
          </Typography>
        </Box>
      )}
    </Box>
  )
}

function LargeViewer({ spec, colors }: { spec: Mol3DSpec; colors: SciColors }) {
  const handle = useRef<Mol3DHandle | null>(null)
  return <Viewer spec={spec} colors={colors} handle={handle} />
}

/** A ```mol3d fence: a 3D molecule or protein, drawn by 3Dmol.js (loaded on first use). */
export function Mol3DBlock({ code }: { code: string }) {
  const source = useSettled(code)
  const colors = useSciColors()
  const parsed = useMemo<{ spec: Mol3DSpec } | { error: SciError }>(() => {
    try {
      return { spec: parseMol3D(source) }
    } catch (e) {
      return { error: e instanceof DslError ? { message: e.message, line: e.line } : { message: e instanceof Error ? e.message : String(e) } }
    }
  }, [source])
  const handle = useRef<Mol3DHandle | null>(null)
  const spec = 'spec' in parsed ? parsed.spec : null

  const getPng = async () => {
    const uri = handle.current?.png()
    return uri ? await (await fetch(uri)).blob() : null
  }

  return (
    <SciBlock
      label="3D structure"
      title={spec?.title ?? (spec?.source.type === 'rcsb' ? spec.source.id : undefined)}
      source={code}
      error={'error' in parsed ? parsed.error : null}
      getPng={getPng}
      hint="drag to rotate · scroll to zoom"
      render={({ large }) => (spec ? large ? <LargeViewer spec={spec} colors={colors} /> : <Viewer spec={spec} colors={colors} handle={handle} /> : null)}
    />
  )
}
