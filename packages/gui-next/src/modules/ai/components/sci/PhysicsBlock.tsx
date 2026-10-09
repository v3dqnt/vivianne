import { useEffect, useMemo, useRef, useState } from 'react'
import { Box, ButtonBase, Slider, Stack, Typography } from '../../../../ui'
import { CHAT_FONT } from '../../lib/chatStyle'
import { SciBlock, useSettled, type SciError } from './SciBlock'
import { useSciColors } from './SvgFigure'
import { DslError, type SciColors } from './dslCommon'
import { compilePhysics, type Scene } from './physicsDsl'
import { sceneToSvg } from './physicsSvg'
import type { PhysicsHandle } from './physicsDraw'

/** One physics figure: a JSXGraph board in its own element, redrawn when the script or theme changes. */
function BoardView({ scene, colors, time, large }: { scene: Scene; colors: SciColors; time: number; large: boolean }) {
  const host = useRef<HTMLDivElement | null>(null)
  const handle = useRef<PhysicsHandle | null>(null)
  const latest = useRef(time)
  latest.current = time
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const el = host.current
    if (!el) return
    let cancelled = false
    // Each run has its own element, so a superseded run can never clear a newer figure.
    const root = document.createElement('div')
    root.style.maxWidth = '100%'
    el.replaceChildren(root)
    const width = Math.max(260, Math.min(large ? 1000 : 680, el.clientWidth - 16 || 640))
    void (async () => {
      try {
        const { drawPhysics } = await import('./physicsDraw')
        const h = await drawPhysics(root, scene, colors, width)
        if (cancelled) {
          h.destroy()
          return
        }
        handle.current = h
        h.setTime(latest.current)
        setError(null)
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e))
      }
    })()
    return () => {
      cancelled = true
      handle.current?.destroy()
      handle.current = null
      root.remove()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scene, colors.dark, colors.ink, large])

  useEffect(() => {
    handle.current?.setTime(time)
  }, [time])

  return (
    <>
      <Box ref={host} sx={{ p: 1, display: 'flex', justifyContent: 'center', minHeight: 200, overflow: 'hidden', '& .JXGtext': { userSelect: 'none' }, '& svg': { overflow: 'visible' } }} />
      {error && <Typography sx={{ px: 1.5, pb: 1, fontSize: CHAT_FONT.meta, color: 'error.main' }}>{error}</Typography>}
    </>
  )
}

/** A ```physics fence: free-body, optics, field, circuit, wave and energy-level diagrams on JSXGraph. */
export function PhysicsBlock({ code }: { code: string }) {
  const source = useSettled(code)
  const colors = useSciColors()
  const parsed = useMemo<{ scene: Scene } | { error: SciError }>(() => {
    try {
      return { scene: compilePhysics(source) }
    } catch (e) {
      return { error: e instanceof DslError ? { message: e.message, line: e.line } : { message: e instanceof Error ? e.message : String(e) } }
    }
  }, [source])
  const scene = 'scene' in parsed ? parsed.scene : null
  const [time, setTime] = useState(0)
  const [playing, setPlaying] = useState(false)
  const timeRef = useRef(0)
  timeRef.current = time

  // A wave starts moving, unless the script says `animate off`.
  useEffect(() => {
    setTime(0)
    setPlaying(!!scene?.time?.play && !window.matchMedia?.('(prefers-reduced-motion: reduce)').matches)
  }, [scene])

  useEffect(() => {
    if (!playing || !scene?.time) return
    let frame = 0
    let last = performance.now()
    const period = scene.time.period
    const tick = (now: number) => {
      const dt = (now - last) / 1000
      last = now
      // One period takes about four seconds on screen, whatever the units.
      setTime((t) => (t + (dt * period) / 4) % period)
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [playing, scene])

  const getSvg = () => (scene ? sceneToSvg(scene, colors, timeRef.current) : null)

  return (
    <SciBlock
      label="Physics"
      title={scene?.title}
      source={code}
      error={'error' in parsed ? parsed.error : null}
      getSvg={getSvg}
      hint={scene?.time ? 'drag to pan' : 'drag to pan'}
      render={({ large }) =>
        scene ? (
          <>
            <BoardView scene={scene} colors={colors} time={time} large={large} />
            {scene.time && (
              <Stack direction="row" sx={{ px: 2, pb: 1, alignItems: 'center', gap: 1.5 }}>
                <ButtonBase onClick={() => setPlaying((p) => !p)} sx={{ px: 1, py: 0.25, borderRadius: '8px', fontSize: CHAT_FONT.meta, color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}>
                  {playing ? 'Pause' : 'Play'}
                </ButtonBase>
                <Slider
                  value={time}
                  min={0}
                  max={scene.time.period}
                  step={scene.time.period / 200}
                  aria-label="time"
                  onChange={(_e, v) => {
                    setPlaying(false)
                    setTime(v)
                  }}
                />
                <Typography sx={{ fontSize: CHAT_FONT.meta, minWidth: 56, textAlign: 'right', fontFamily: '"Geist Mono", ui-monospace, monospace' }}>t = {time.toFixed(2)}</Typography>
              </Stack>
            )}
            {scene.notes.length > 0 && (
              <Stack sx={{ px: 2, pb: 1.25, gap: 0.25 }}>
                {scene.notes.map((n, i) => (
                  <Typography key={i} color="text.secondary" sx={{ fontSize: CHAT_FONT.meta }}>
                    {n}
                  </Typography>
                ))}
              </Stack>
            )}
          </>
        ) : null
      }
    />
  )
}
