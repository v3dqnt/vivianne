import { useEffect, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import { call } from '../../../ipc'
import { CodeBlock } from '../../ai/components/CodeBlock'

function langFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = {
    ts: 'typescript',
    tsx: 'tsx',
    js: 'javascript',
    jsx: 'jsx',
    rs: 'rust',
    py: 'python',
    json: 'json',
    md: 'markdown',
    css: 'css',
    html: 'html',
    toml: 'ini',
    yml: 'yaml',
    yaml: 'yaml',
    sh: 'bash',
    bash: 'bash',
  }
  return map[ext] ?? 'text'
}

/** Mirrors `file_preview::FilePreview` on the Rust side. */
export type FilePreviewData =
  | { kind: 'text'; content: string; truncated: boolean; lines: number }
  | { kind: 'image'; base64: string; mediaType: string; width: number; height: number; bytes: number; downscaled: boolean }
  | { kind: 'svg'; source: string }
  | { kind: 'pdf'; base64: string; bytes: number }
  | { kind: 'binary'; bytes: number; hex: string; mediaType: string | null; note: string }

/** Settled-content cache: expanding a tree row (or reopening the fullscreen
 * viewer) remounts `FileContent`, which used to re-read and re-decode the
 * file every time. Keyed by path, FIFO-capped — files edited on disk go stale
 * here, which matches the preview's snapshot nature (reopening after a change
 * re-fetches only once this is cleared on app reload). */
const previewCache = new Map<string, FilePreviewData>()
const CACHE_MAX_ENTRIES = 40

function cachePut(path: string, data: FilePreviewData): void {
  previewCache.delete(path)
  previewCache.set(path, data)
  if (previewCache.size > CACHE_MAX_ENTRIES) {
    const oldest = previewCache.keys().next().value
    if (oldest !== undefined) previewCache.delete(oldest)
  }
}

/** Shiki tokenizing a multi-hundred-KB file blocks the main thread far longer
 * than the highlighting is worth — past this cap, render plain monospace. */
const MAX_HIGHLIGHT_CHARS = 250_000

function formatBytes(n: number): string {
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`
  if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${n} B`
}

/** Reads a file via `cmd_read_file_preview` and renders it by what it
 * actually is — images as images, SVG as vector source, PDFs in the
 * browser's own viewer, text with shiki highlighting, and anything else as a
 * labelled hex dump.
 *
 * The previous version called a bare `read_to_string`, so every binary in the
 * tree rendered as the same "stream did not contain valid UTF-8" error.
 *
 * @param wrap reflow long lines instead of scrolling horizontally — right
 *   for the narrow inline preview, wrong for the fullscreen view where the
 *   original line structure is what you came to read.
 * @param bare strip the code block's card chrome, for the inline preview
 *   where the code should read as part of the tree.
 */
export function FileContent({
  path,
  wrap = true,
  bare = false,
}: {
  path: string
  wrap?: boolean
  bare?: boolean
}) {
  const [data, setData] = useState<FilePreviewData | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setData(null)
    setError(null)
    let stale = false
    const cached = previewCache.get(path)
    if (cached !== undefined) {
      setData(cached)
      return
    }
    void call<FilePreviewData>('cmd_read_file_preview', { path })
      .then((d) => {
        if (stale) return
        if (d) {
          cachePut(path, d)
          setData(d)
        } else setError('Could not read file')
      })
      .catch((e) => {
        if (!stale) setError(e instanceof Error ? e.message : String(e))
      })
    return () => {
      stale = true
    }
  }, [path])

  if (error) {
    return (
      <Typography variant="caption" color="error.main">
        {error}
      </Typography>
    )
  }
  if (data === null) {
    return (
      <Typography variant="caption" color="text.secondary">
        Loading…
      </Typography>
    )
  }

  const meta = (text: string) => (
    <Typography variant="caption" color="text.disabled" sx={{ display: 'block', mt: 0.5, fontSize: '0.6875rem' }}>
      {text}
    </Typography>
  )

  if (data.kind === 'image') {
    return (
      <Box sx={{ minWidth: 0 }}>
        <Box
          component="img"
          src={`data:${data.mediaType};base64,${data.base64}`}
          alt={path}
          sx={{
            maxWidth: '100%',
            maxHeight: 320,
            objectFit: 'contain',
            borderRadius: '6px',
            display: 'block',
            // A checker ground so transparent PNGs (icons, logos — most of
            // what's in an assets folder) don't vanish into the panel.
            backgroundImage:
              'linear-gradient(45deg, rgba(128,128,128,0.15) 25%, transparent 25%, transparent 75%, rgba(128,128,128,0.15) 75%), linear-gradient(45deg, rgba(128,128,128,0.15) 25%, transparent 25%, transparent 75%, rgba(128,128,128,0.15) 75%)',
            backgroundSize: '16px 16px',
            backgroundPosition: '0 0, 8px 8px',
          }}
        />
        {meta(
          `${data.width}×${data.height} · ${formatBytes(data.bytes)}${data.downscaled ? ' · downscaled for preview' : ''}`,
        )}
      </Box>
    )
  }

  if (data.kind === 'svg') {
    return (
      <Box sx={{ minWidth: 0 }}>
        <Box
          // Rendered from source rather than rasterised: an SVG stays sharp
          // at any size, and this is a preview panel that can be resized.
          dangerouslySetInnerHTML={{ __html: data.source }}
          sx={{
            maxHeight: 320,
            overflow: 'auto',
            '& svg': { maxWidth: '100%', height: 'auto', display: 'block' },
          }}
        />
        {meta('SVG · rendered from source')}
      </Box>
    )
  }

  if (data.kind === 'pdf') {
    return (
      <Box sx={{ minWidth: 0 }}>
        <Box
          component="iframe"
          src={`data:application/pdf;base64,${data.base64}`}
          title={path}
          sx={{ width: '100%', height: 400, border: 0, borderRadius: '6px', bgcolor: 'common.white' }}
        />
        {meta(`PDF · ${formatBytes(data.bytes)}`)}
      </Box>
    )
  }

  if (data.kind === 'binary') {
    return (
      <Stack sx={{ minWidth: 0, gap: 0.5 }}>
        <Typography variant="caption" sx={{ fontWeight: 600 }}>
          {data.note}
        </Typography>
        <Box
          component="pre"
          sx={{
            m: 0,
            p: 0,
            fontFamily: '"JetBrains Mono", ui-monospace, monospace',
            fontSize: '0.6875rem',
            lineHeight: 1.5,
            color: 'text.disabled',
            overflowX: 'auto',
            maxHeight: 240,
          }}
        >
          {data.hex}
        </Box>
        {meta(`${formatBytes(data.bytes)} · first ${Math.min(512, data.bytes)} bytes shown`)}
      </Stack>
    )
  }

  // Text. minWidth: 0 at every level is what lets the code area shrink below
  // its content's natural width inside a flex column — without it a long line
  // forces the whole dock wider instead of scrolling/wrapping internally.
  if (data.content.length > MAX_HIGHLIGHT_CHARS) {
    return (
      <Box
        component="pre"
        sx={{
          minWidth: 0,
          m: 0,
          p: bare ? 0 : 1.5,
          ...(bare ? { color: 'text.secondary' } : { borderRadius: 1, bgcolor: '#0d1117', color: '#c9d1d9' }),
          fontFamily: '"JetBrains Mono", ui-monospace, monospace',
          fontSize: '0.75rem',
          lineHeight: 1.5,
          whiteSpace: wrap ? 'pre-wrap' : 'pre',
          overflowX: 'auto',
        }}
      >
        {data.content}
      </Box>
    )
  }
  return (
    <Box sx={{ minWidth: 0, '& > *': { minWidth: 0 } }}>
      <CodeBlock code={data.content} lang={langFromPath(path)} wrap={wrap} bare={bare} />
      {data.truncated && meta(`Truncated — showing the first ${MAX_HIGHLIGHT_CHARS.toLocaleString()} characters`)}
    </Box>
  )
}
