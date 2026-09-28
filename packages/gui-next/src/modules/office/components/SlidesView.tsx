import { useEffect, useRef, useState } from 'react'
import { Box, Typography } from '../../../ui'
import { init as initPptx } from 'pptx-preview'

/** PowerPoint, one slide under the next, scaled to the panel's width. */
export function SlidesView({ bytes }: { bytes: Uint8Array }) {
  const host = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const el = host.current
    if (!el) return
    let t = 0
    const ro = new ResizeObserver(([entry]) => {
      window.clearTimeout(t)
      // Re-laying out every slide is expensive; wait until the drag settles.
      t = window.setTimeout(() => setWidth(Math.floor(entry.contentRect.width)), 180)
    })
    ro.observe(el)
    return () => {
      ro.disconnect()
      window.clearTimeout(t)
    }
  }, [])

  useEffect(() => {
    const el = host.current
    if (!el || width < 50) return
    el.innerHTML = ''
    setError(null)
    const w = Math.max(240, width - 32)
    const previewer = initPptx(el, { width: w, height: Math.round((w * 9) / 16), mode: 'list' })
    previewer
      .preview(bytes.slice().buffer)
      .then(() => {
        // The library frames every slide inside one fixed-height, black,
        // scrolling box sized from the height passed in — which clips a 4:3
        // deck and nests a second scrollbar in the panel. Let it grow to its
        // slides and scroll with the panel instead.
        const wrapper = el.querySelector<HTMLElement>('.pptx-preview-wrapper')
        if (wrapper) {
          wrapper.style.height = 'auto'
          wrapper.style.overflow = 'visible'
          wrapper.style.background = 'transparent'
        }
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
    return () => {
      try {
        previewer.destroy()
      } catch {
        // already torn down with its container
      }
    }
  }, [bytes, width])

  return (
    <Box sx={{ flex: 1, overflow: 'auto', bgcolor: 'action.hover', p: 2 }}>
      {error && (
        <Typography variant="caption" color="error.main" sx={{ display: 'block', pb: 1 }}>
          Couldn’t render this presentation: {error}
        </Typography>
      )}
      <Box ref={host} sx={{ '& > div': { mx: 'auto' }, '& .pptx-preview-slide-wrapper': { boxShadow: '0 2px 12px rgba(0,0,0,0.25)', mb: 2 } }} />
    </Box>
  )
}
