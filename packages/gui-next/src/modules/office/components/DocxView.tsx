import { useEffect, useRef, useState } from 'react'
import { Box, Typography } from '../../../ui'
import { renderAsync } from 'docx-preview'
import { MIME } from '../lib/docs'

/** Word, laid out as pages. */
export function DocxView({ bytes }: { bytes: Uint8Array }) {
  const ref = useRef<HTMLDivElement>(null)
  const scroller = useRef<HTMLDivElement>(null)
  const [error, setError] = useState<string | null>(null)
  const [zoom, setZoom] = useState(1)
  const [rendered, setRendered] = useState(0)

  // Fit the page to the panel's width, as Word's "page width" zoom does — an
  // A4 page is wider than a docked panel, and scrolling sideways to read
  // every line is worse than slightly smaller text. Never enlarged past 100%.
  useEffect(() => {
    const box = scroller.current
    if (!box) return
    const fit = () => {
      const page = ref.current?.querySelector<HTMLElement>('section.docx')
      if (!page) return
      const available = box.clientWidth - 32
      setZoom(Math.min(1, Math.max(0.3, available / page.offsetWidth)))
    }
    fit()
    const ro = new ResizeObserver(fit)
    ro.observe(box)
    return () => ro.disconnect()
  }, [rendered])
  useEffect(() => {
    const el = ref.current
    if (!el) return
    el.innerHTML = ''
    setError(null)
    renderAsync(new Blob([bytes as BlobPart], { type: MIME.docx }), el, undefined, {
      inWrapper: true,
      ignoreLastRenderedPageBreak: true,
      className: 'docx',
    })
      .then(() => setRendered((n) => n + 1))
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
  }, [bytes])
  return (
    <Box ref={scroller} sx={{ flex: 1, overflow: 'auto', bgcolor: 'action.hover' }}>
      {error && (
        <Typography variant="caption" color="error.main" sx={{ display: 'block', p: 2 }}>
          Couldn’t render this document: {error}
        </Typography>
      )}
      <Box
        ref={ref}
        sx={{
          zoom,
          // The library paints a grey desk behind the pages; the panel's own
          // surface does that job.
          '& .docx-wrapper': { background: 'transparent', padding: '16px', gap: '16px' },
          '& .docx-wrapper > section.docx': { boxShadow: '0 2px 12px rgba(0,0,0,0.25)', mb: 0 },
        }}
      />
    </Box>
  )
}
