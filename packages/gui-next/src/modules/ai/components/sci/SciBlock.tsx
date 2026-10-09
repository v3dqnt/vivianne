import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { Box, ButtonBase, CircularProgress, Dialog, Stack, Typography, useTheme } from '../../../../ui'
import { CHAT_FONT, CHAT_HEADER_SX, CHAT_META_SX, CHAT_SURFACE_SX, CHAT_TITLE_SX } from '../../lib/chatStyle'

/** What a figure's parser or renderer said was wrong, and where. */
export interface SciError {
  message: string
  /** 1-based line of the script. */
  line?: number
}

export interface SciBlockProps {
  /** The fence this is, shown as the figure's label: "Chemistry", "Plot"… */
  label: string
  /** The figure's own title, when it has one. */
  title?: string
  source: string
  error?: SciError | null
  /** Still loading the renderer or drawing the first time. */
  loading?: boolean
  /** Draws the figure. Called again, with `large`, for the enlarged view. */
  render: (opts: { large: boolean }) => ReactNode
  /** The figure as a standalone SVG string, for Copy SVG. */
  getSvg?: () => string | null
  /** The figure as a PNG, for Copy PNG; made from the SVG when absent. */
  getPng?: () => Promise<Blob | null>
  /** Small print on the right of the header: "drag to orbit". */
  hint?: string
  /** Height of the preview area while it loads. */
  minHeight?: number
}

/** An SVG string as a PNG, drawn at twice the size on the given background. */
export async function svgToPng(svg: string, bg: string): Promise<Blob | null> {
  const url = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml;charset=utf-8' }))
  try {
    const img = new Image()
    img.decoding = 'async'
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve()
      img.onerror = () => reject(new Error('The figure could not be rasterised.'))
      img.src = url
    })
    const w = img.naturalWidth || 800
    const h = img.naturalHeight || 600
    const canvas = document.createElement('canvas')
    canvas.width = w * 2
    canvas.height = h * 2
    const ctx = canvas.getContext('2d')
    if (!ctx) return null
    ctx.fillStyle = bg
    ctx.fillRect(0, 0, canvas.width, canvas.height)
    ctx.drawImage(img, 0, 0, canvas.width, canvas.height)
    return await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/png'))
  } finally {
    URL.revokeObjectURL(url)
  }
}

function download(blob: Blob, name: string) {
  const a = document.createElement('a')
  a.href = URL.createObjectURL(blob)
  a.download = name
  a.click()
  setTimeout(() => URL.revokeObjectURL(a.href), 2000)
}

/**
 * The frame every science figure sits in: the same card as the rest of the
 * chat, with a header to switch between the figure and its source, copy it as
 * SVG or PNG, and open it larger; and one place that explains a script that
 * will not draw, quoting the line at fault.
 */
export function SciBlock({ label, title, source, error, loading, render, getSvg, getPng, hint, minHeight = 120 }: SciBlockProps) {
  const theme = useTheme()
  const [view, setView] = useState<'figure' | 'source'>('figure')
  const [large, setLarge] = useState(false)
  const [note, setNote] = useState<string | null>(null)
  const noteTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  useEffect(() => () => clearTimeout(noteTimer.current), [])

  const say = useCallback((text: string) => {
    setNote(text)
    clearTimeout(noteTimer.current)
    noteTimer.current = setTimeout(() => setNote(null), 1800)
  }, [])

  const copySvg = async () => {
    const svg = getSvg?.()
    if (!svg) return say('Nothing to copy yet')
    try {
      await navigator.clipboard.writeText(svg)
      say('SVG copied')
    } catch {
      download(new Blob([svg], { type: 'image/svg+xml' }), `${label.toLowerCase()}.svg`)
      say('SVG saved')
    }
  }

  const copyPng = async () => {
    try {
      const svg = getSvg?.()
      const blob = (await getPng?.()) ?? (svg ? await svgToPng(svg, theme.palette.background.paper) : null)
      if (!blob) return say('Nothing to copy yet')
      try {
        await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })])
        say('PNG copied')
      } catch {
        download(blob, `${label.toLowerCase()}.png`)
        say('PNG saved')
      }
    } catch (e) {
      say(e instanceof Error ? e.message : 'Could not make a PNG')
    }
  }

  const showSource = view === 'source' || !!error
  const lines = source.split('\n')
  const canExport = !error && (!!getSvg || !!getPng)

  const tab = (id: 'figure' | 'source', text: string) => (
    <ButtonBase
      onClick={() => setView(id)}
      disabled={id === 'figure' && !!error}
      sx={{
        px: 1,
        py: 0.25,
        borderRadius: '8px',
        fontSize: CHAT_FONT.meta,
        fontWeight: 500,
        color: showSource === (id === 'source') ? 'text.primary' : 'text.secondary',
        bgcolor: showSource === (id === 'source') ? 'action.selected' : 'transparent',
        '&:hover': { bgcolor: 'action.hover' },
      }}
    >
      {text}
    </ButtonBase>
  )
  const action = (text: string, onClick: () => void) => (
    <ButtonBase
      onClick={onClick}
      sx={{ px: 0.75, py: 0.25, borderRadius: '8px', fontSize: CHAT_FONT.meta, color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}
    >
      {text}
    </ButtonBase>
  )

  return (
    <>
      <Box sx={{ ...CHAT_SURFACE_SX, my: 1 }}>
        <Box sx={{ ...CHAT_HEADER_SX, flexWrap: 'wrap', gap: 0.5 }}>
          <Typography sx={CHAT_TITLE_SX}>{title ?? label}</Typography>
          {title && (
            <Typography color="text.secondary" sx={CHAT_META_SX}>
              {label}
            </Typography>
          )}
          <Stack direction="row" sx={{ ml: 'auto', alignItems: 'center', gap: 0.25 }}>
            {note && (
              <Typography color="text.secondary" sx={{ ...CHAT_META_SX, mr: 0.5 }}>
                {note}
              </Typography>
            )}
            {hint && !showSource && !note && (
              <Typography color="text.secondary" sx={{ ...CHAT_META_SX, mr: 0.75}}>
                {hint}
              </Typography>
            )}
            {tab('figure', 'Figure')}
            {tab('source', 'Source')}
            {canExport && getSvg && action('SVG', copySvg)}
            {canExport && action('PNG', copyPng)}
            {!error && action('Larger', () => setLarge(true))}
          </Stack>
        </Box>
        {error && (
          <Box sx={{ px: 1.5, py: 1, borderTop: 1, borderColor: 'divider', bgcolor: 'action.hover' }}>
            <Typography sx={{ fontSize: CHAT_FONT.title, fontWeight: 600, color: 'error.main' }}>
              {error.line ? `Line ${error.line}: ` : ''}
              {error.message}
            </Typography>
          </Box>
        )}
        {showSource ? (
          <Box
            component="pre"
            sx={{ m: 0, px: 0, py: 1, borderTop: 1, borderColor: 'divider', fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.75rem', lineHeight: 1.6, overflowX: 'auto' }}
          >
            {lines.map((text, i) => {
              const bad = error?.line === i + 1
              return (
                <Box key={i} component="div" sx={{ display: 'flex', px: 1.5, bgcolor: bad ? 'rgba(229, 72, 77, 0.14)' : 'transparent' }}>
                  <Box component="span" sx={{ width: 28, flexShrink: 0, color: bad ? 'error.main' : 'text.disabled', userSelect: 'none' }}>
                    {i + 1}
                  </Box>
                  <Box component="span" sx={{ whiteSpace: 'pre' }}>
                    {text || ' '}
                  </Box>
                </Box>
              )
            })}
          </Box>
        ) : (
          <Box sx={{ position: 'relative', borderTop: 1, borderColor: 'divider', minHeight: loading ? minHeight : undefined, overflowX: 'auto' }}>
            {loading && (
              <Stack sx={{ position: 'absolute', inset: 0, alignItems: 'center', justifyContent: 'center', gap: 1, pointerEvents: 'none' }}>
                <CircularProgress size={18} />
                <Typography color="text.secondary" sx={CHAT_META_SX}>
                  Drawing…
                </Typography>
              </Stack>
            )}
            {render({ large: false })}
          </Box>
        )}
      </Box>
      <Dialog open={large} onClose={() => setLarge(false)} maxWidth="lg" fullWidth>
        <Box sx={{ ...CHAT_HEADER_SX, borderBottom: 1, borderColor: 'divider' }}>
          <Typography sx={CHAT_TITLE_SX}>{title ?? label}</Typography>
          <Box sx={{ ml: 'auto' }}>{action('Close', () => setLarge(false))}</Box>
        </Box>
        <Box sx={{ p: 2, overflow: 'auto' }}>{large && render({ large: true })}</Box>
      </Dialog>
    </>
  )
}

/** Debounces a script while it is still streaming into the chat. */
export function useSettled<T>(value: T, ms = 300): T {
  const [settled, setSettled] = useState(value)
  useEffect(() => {
    const t = setTimeout(() => setSettled(value), ms)
    return () => clearTimeout(t)
  }, [value, ms])
  return settled
}
