import { useEffect, useState } from 'react'
import { Dialog as RD } from 'radix-ui'
import { Box, IconButton, Stack, Tooltip, Typography } from '../../../ui'
import { CloseLinear, CopyLinear, TickCircleLinear, ZoomInLinear, ZoomOutLinear, ImageBrokenLinear, ArrowRightLinear } from '../../../icons'
import { call } from '../../../ipc'
import { usePanelStore } from '../../dock/store/panelStore'
import type { MessageContent } from '../lib/types'

const LOCAL = /^(?:[a-zA-Z]:[\\/]|\/|\\\\|file:)/

/**
 * Where an image's pixels come from. Data URLs and web addresses load as
 * they are; a path on this computer is read through the backend (the webview
 * can't load local files), which also downscales huge ones.
 */
function useImageSrc(src: string): { url: string | null; failed: boolean; local: string | null } {
  const local = LOCAL.test(src) ? src.replace(/^file:\/\/\/?/, '') : null
  const [state, setState] = useState<{ url: string | null; failed: boolean }>({ url: local ? null : src, failed: false })
  useEffect(() => {
    if (!local) {
      setState({ url: src, failed: false })
      return
    }
    let stale = false
    setState({ url: null, failed: false })
    void call<{ kind: string; base64?: string; mediaType?: string; source?: string }>('cmd_read_file_preview', { path: decodeURI(local) })
      .then((d) => {
        if (stale) return
        if (d?.kind === 'image' && d.base64) setState({ url: `data:${d.mediaType};base64,${d.base64}`, failed: false })
        else if (d?.kind === 'svg' && d.source) setState({ url: `data:image/svg+xml;charset=utf-8,${encodeURIComponent(d.source)}`, failed: false })
        else setState({ url: null, failed: true })
      })
      .catch(() => !stale && setState({ url: null, failed: true }))
    return () => {
      stale = true
    }
  }, [src, local])
  return { ...state, local }
}

/** An image in the conversation: a thumbnail that opens a viewer. */
export function ChatImage({ src, alt, inline = false }: { src: string; alt?: string; inline?: boolean }) {
  const { url, failed, local } = useImageSrc(src)
  const [open, setOpen] = useState(false)
  const [broken, setBroken] = useState(false)
  if (failed || broken) {
    return (
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, px: 1.25, py: 0.75, my: inline ? 1 : 0, borderRadius: '10px', border: 1, borderColor: 'divider', color: 'text.secondary', maxWidth: '100%' }}>
        <ImageBrokenLinear size={15} color="currentColor" />
        <Typography noWrap sx={{ fontSize: '0.75rem', minWidth: 0 }} title={src}>
          {alt || "Couldn't load this image"} · {src.startsWith('data:') ? 'embedded image' : src}
        </Typography>
      </Stack>
    )
  }
  return (
    <>
      <Box
        component="button"
        onClick={() => url && setOpen(true)}
        aria-label={alt ? `Open image: ${alt}` : 'Open image'}
        sx={{
          display: 'block',
          p: 0,
          my: inline ? 1 : 0,
          border: 1,
          borderColor: 'divider',
          borderRadius: '12px',
          overflow: 'hidden',
          bgcolor: 'action.hover',
          cursor: 'zoom-in',
          maxWidth: inline ? '100%' : 360,
          minWidth: url ? 0 : 160,
          minHeight: url ? 0 : 110,
          transition: 'border-color 0.15s',
          '&:hover': { borderColor: 'text.disabled' },
        }}
      >
        {url && (
          <Box
            component="img"
            src={url}
            alt={alt ?? ''}
            onError={() => setBroken(true)}
            sx={{ display: 'block', maxWidth: '100%', maxHeight: inline ? 420 : 260, objectFit: 'contain' }}
          />
        )}
      </Box>
      {url && <ImageViewer open={open} onClose={() => setOpen(false)} url={url} alt={alt} local={local} />}
    </>
  )
}

/** The full-size view: fit or actual size, copy, and open the file. */
function ImageViewer({ open, onClose, url, alt, local }: { open: boolean; onClose: () => void; url: string; alt?: string; local: string | null }) {
  const [actual, setActual] = useState(false)
  const [copied, setCopied] = useState(false)
  const copy = async () => {
    try {
      const blob = await (await fetch(url)).blob()
      const png = blob.type === 'image/png' ? blob : await toPng(blob)
      await navigator.clipboard.write([new ClipboardItem({ 'image/png': png })])
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1200)
    } catch {
      // Clipboard refused (permissions): nothing to undo.
    }
  }
  const btn = { color: 'common.white', bgcolor: 'rgba(255,255,255,0.08)', '&:hover': { bgcolor: 'rgba(255,255,255,0.18)' }, width: 34, height: 34 }
  return (
    <RD.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <RD.Portal>
        <RD.Overlay style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.82)', backdropFilter: 'blur(6px)', zIndex: 1400 }} />
        <RD.Content aria-describedby={undefined} style={{ position: 'fixed', inset: 0, zIndex: 1401, display: 'flex', flexDirection: 'column', outline: 'none' }} onClick={onClose}>
          <RD.Title style={{ position: 'absolute', width: 1, height: 1, overflow: 'hidden', clip: 'rect(0 0 0 0)' }}>{alt || 'Image'}</RD.Title>
          <Stack direction="row" onClick={(e) => e.stopPropagation()} sx={{ alignSelf: 'flex-end', gap: 0.75, p: 1.5 }}>
            <Tooltip title={actual ? 'Fit to screen' : 'Actual size'}>
              <IconButton aria-label={actual ? 'Fit to screen' : 'Actual size'} onClick={() => setActual((a) => !a)} sx={btn}>
                {actual ? <ZoomOutLinear size={16} color="currentColor" /> : <ZoomInLinear size={16} color="currentColor" />}
              </IconButton>
            </Tooltip>
            <Tooltip title={copied ? 'Copied' : 'Copy image'}>
              <IconButton aria-label="Copy image" onClick={() => void copy()} sx={btn}>
                {copied ? <TickCircleLinear size={16} color="currentColor" /> : <CopyLinear size={16} color="currentColor" />}
              </IconButton>
            </Tooltip>
            {local && (
              <Tooltip title="Open in Preview">
                <IconButton
                  aria-label="Open in Preview"
                  onClick={() => {
                    usePanelStore.getState().openDocument(local)
                    onClose()
                  }}
                  sx={btn}
                >
                  <ArrowRightLinear size={16} color="currentColor" />
                </IconButton>
              </Tooltip>
            )}
            <RD.Close asChild>
              <IconButton aria-label="Close" sx={btn}>
                <CloseLinear size={16} color="currentColor" />
              </IconButton>
            </RD.Close>
          </Stack>
          <Box sx={{ flex: 1, minHeight: 0, overflow: 'auto', display: 'flex', alignItems: actual ? 'flex-start' : 'center', justifyContent: actual ? 'flex-start' : 'center', p: 3, pt: 0 }}>
            <Box
              component="img"
              src={url}
              alt={alt ?? ''}
              onClick={(e: React.MouseEvent) => {
                e.stopPropagation()
                setActual((a) => !a)
              }}
              sx={{
                display: 'block',
                m: 'auto',
                cursor: actual ? 'zoom-out' : 'zoom-in',
                borderRadius: '8px',
                ...(actual ? {} : { maxWidth: '100%', maxHeight: '100%', objectFit: 'contain' }),
              }}
            />
          </Box>
        </RD.Content>
      </RD.Portal>
    </RD.Root>
  )
}

/** The clipboard takes PNG; anything else is redrawn as one. */
async function toPng(blob: Blob): Promise<Blob> {
  const bmp = await createImageBitmap(blob)
  const canvas = document.createElement('canvas')
  canvas.width = bmp.width
  canvas.height = bmp.height
  canvas.getContext('2d')!.drawImage(bmp, 0, 0)
  return new Promise((res, rej) => canvas.toBlob((b) => (b ? res(b) : rej(new Error('encode'))), 'image/png'))
}

/** One `image` content block (a pasted image, a screenshot a tool took). */
export function ImageBlock({ block }: { block: Extract<MessageContent, { type: 'image' }> }) {
  const src = block.base64 ? `data:${block.mediaType ?? 'image/png'};base64,${block.base64}` : block.url
  if (!src) return null
  return <ChatImage src={src} />
}
