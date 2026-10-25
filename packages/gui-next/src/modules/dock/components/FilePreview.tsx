import { lazy, Suspense, useEffect, useMemo, useState, type ReactNode } from 'react'
import { Box, ButtonBase, Stack, Typography } from '../../../ui'
import { call } from '../../../ipc'
import { CodeBlock } from '../../ai/components/CodeBlock'
import { Markdown } from '../../ai/components/Markdown'

const Model3DBlock = lazy(() => import('../../ai/components/Model3DBlock').then((m) => ({ default: m.Model3DBlock })))

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
    zsh: 'bash',
    ps1: 'powershell',
    go: 'go',
    java: 'java',
    kt: 'kotlin',
    swift: 'swift',
    c: 'c',
    h: 'c',
    cpp: 'cpp',
    cc: 'cpp',
    hpp: 'cpp',
    cs: 'csharp',
    rb: 'ruby',
    php: 'php',
    lua: 'lua',
    r: 'r',
    jl: 'julia',
    sql: 'sql',
    xml: 'xml',
    scss: 'scss',
    vue: 'vue',
    svelte: 'svelte',
    tex: 'latex',
  }
  return map[ext] ?? 'text'
}

/** Mirrors `file_preview::FilePreview` on the Rust side. */
export type FilePreviewData =
  | { kind: 'text'; content: string; truncated: boolean; lines: number }
  | { kind: 'image'; base64: string; mediaType: string; width: number; height: number; bytes: number; downscaled: boolean }
  | { kind: 'svg'; source: string }
  | { kind: 'pdf'; base64: string; bytes: number }
  | { kind: 'media'; media: 'video' | 'audio' | 'model'; base64: string; mediaType: string; bytes: number }
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
  full = false,
  fresh = false,
  dense = false,
}: {
  path: string
  /** Small, quiet code that folds when long: the tool-card look. */
  dense?: boolean
  wrap?: boolean
  bare?: boolean
  /** Filling a panel: no height caps, and Markdown, HTML and CSV are shown
   * rendered (with their source a click away) rather than as source. */
  full?: boolean
  /** Skip the cache: a viewer opened to see the file as it is now. */
  fresh?: boolean
}) {
  const [data, setData] = useState<FilePreviewData | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setData(null)
    setError(null)
    let stale = false
    const cached = fresh ? undefined : previewCache.get(path)
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
  }, [path, fresh])

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
            maxHeight: full ? 'none' : 320,
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
          // Drawn through <img>, never inlined: an SVG is a document that can
          // carry scripts and event handlers, and inlined into this window it
          // would run with the app's full backend access. As an image it
          // can't run anything — and it still stays sharp at any size.
          component="img"
          alt=""
          src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(data.source)}`}
          sx={{ maxHeight: full ? 'none' : 320, maxWidth: '100%', height: 'auto', display: 'block' }}
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
          sx={{ width: '100%', height: full ? 'calc(100vh - 180px)' : 400, border: 0, borderRadius: '6px', bgcolor: 'common.white' }}
        />
        {meta(`PDF · ${formatBytes(data.bytes)}`)}
      </Box>
    )
  }

  if (data.kind === 'media') {
    return <MediaView data={data} full={full} caption={meta(`${data.mediaType} · ${formatBytes(data.bytes)}`)} />
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
            fontFamily: '"Geist Mono", ui-monospace, monospace',
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

  if (full) {
    const rendered = renderedKind(path)
    if (rendered) return <RenderedText kind={rendered} path={path} content={data.content} />
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
          fontFamily: '"Geist Mono", ui-monospace, monospace',
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
      <CodeBlock code={data.content} lang={langFromPath(path)} wrap={wrap} bare={bare} dense={dense} />
      {data.truncated && meta(`Truncated — showing the first ${MAX_HIGHLIGHT_CHARS.toLocaleString()} characters`)}
    </Box>
  )
}

type MediaData = Extract<FilePreviewData, { kind: 'media' }>

/** Plays video and audio, and draws `.glb`/`.gltf` models, from a blob URL
 * made from the bytes the backend read, revoked when the view goes. */
function MediaView({ data, full, caption }: { data: MediaData; full: boolean; caption: ReactNode }) {
  const url = useMemo(() => {
    const bin = atob(data.base64)
    const bytes = new Uint8Array(bin.length)
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
    return URL.createObjectURL(new Blob([bytes], { type: data.mediaType }))
  }, [data])
  useEffect(() => () => URL.revokeObjectURL(url), [url])

  return (
    <Box sx={{ minWidth: 0 }}>
      {data.media === 'video' ? (
        <Box
          component="video"
          src={url}
          controls
          sx={{ width: '100%', maxHeight: full ? 'calc(100vh - 200px)' : 320, borderRadius: '8px', bgcolor: 'common.black', display: 'block' }}
        />
      ) : data.media === 'audio' ? (
        <Box component="audio" src={url} controls sx={{ width: '100%', display: 'block' }} />
      ) : (
        <Suspense
          fallback={
            <Typography variant="caption" color="text.secondary">
              Loading the 3D viewer…
            </Typography>
          }
        >
          <Model3DBlock code={JSON.stringify({ src: url })} />
        </Suspense>
      )}
      {caption}
    </Box>
  )
}

type Rendered = 'markdown' | 'html' | 'csv' | 'tsv'

function renderedKind(path: string): Rendered | null {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  if (ext === 'md' || ext === 'markdown' || ext === 'mdx') return 'markdown'
  if (ext === 'html' || ext === 'htm') return 'html'
  if (ext === 'csv') return 'csv'
  if (ext === 'tsv') return 'tsv'
  return null
}

/** Quote-aware CSV/TSV split: enough for the files people preview, with
 * quoted fields, doubled quotes, and newlines inside quotes. */
export function parseDelimited(text: string, sep: string, maxRows: number): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let field = ''
  let quoted = false
  for (let i = 0; i < text.length; i++) {
    const c = text[i]
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        field += '"'
        i++
      } else if (c === '"') quoted = false
      else field += c
    } else if (c === '"' && field === '') quoted = true
    else if (c === sep) {
      row.push(field)
      field = ''
    } else if (c === '\n' || c === '\r') {
      if (c === '\r' && text[i + 1] === '\n') i++
      row.push(field)
      rows.push(row)
      row = []
      field = ''
      if (rows.length >= maxRows) return rows
    } else field += c
  }
  if (field !== '' || row.length) {
    row.push(field)
    rows.push(row)
  }
  return rows
}

const MAX_TABLE_ROWS = 1000

/** Markdown as a document, HTML as a page, CSV as a table, each with a
 * toggle back to the source. */
function RenderedText({ kind, path, content }: { kind: Rendered; path: string; content: string }) {
  const [source, setSource] = useState(false)
  const rows = useMemo(
    () => (kind === 'csv' || kind === 'tsv' ? parseDelimited(content, kind === 'csv' ? ',' : '\t', MAX_TABLE_ROWS + 1) : []),
    [kind, content],
  )

  let body: ReactNode
  if (source) {
    body = <CodeBlock code={content} lang={langFromPath(path)} wrap />
  } else if (kind === 'markdown') {
    body = (
      <Box sx={{ fontSize: '0.875rem', maxWidth: 760 }}>
        <Markdown text={content} />
      </Box>
    )
  } else if (kind === 'html') {
    // A sandbox with nothing allowed: the page is drawn, but its scripts,
    // forms and links can't run with the app's access.
    body = (
      <Box
        component="iframe"
        title={path}
        sandbox=""
        srcDoc={content}
        sx={{ width: '100%', height: 'calc(100vh - 200px)', border: 1, borderColor: 'divider', borderRadius: '8px', bgcolor: 'common.white' }}
      />
    )
  } else {
    const [head, ...rest] = rows
    body = (
      <Box sx={{ overflow: 'auto', maxHeight: 'calc(100vh - 200px)', border: 1, borderColor: 'divider', borderRadius: '8px' }}>
        <Box
          component="table"
          sx={{
            borderCollapse: 'collapse',
            fontSize: '0.75rem',
            fontFamily: '"Geist Mono", ui-monospace, monospace',
            '& td, & th': { px: 1, py: 0.5, borderBottom: 1, borderRight: 1, borderColor: 'divider', whiteSpace: 'nowrap', textAlign: 'left' },
            '& th': { position: 'sticky', top: 0, bgcolor: 'background.paper', fontWeight: 600 },
          }}
        >
          {head && (
            <thead>
              <tr>
                {head.map((h, i) => (
                  <th key={i}>{h}</th>
                ))}
              </tr>
            </thead>
          )}
          <tbody>
            {rest.slice(0, MAX_TABLE_ROWS).map((r, i) => (
              <tr key={i}>
                {r.map((c, j) => (
                  <td key={j}>{c}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </Box>
      </Box>
    )
  }

  return (
    <Stack sx={{ minWidth: 0, gap: 1 }}>
      <Stack direction="row" sx={{ gap: 0.5, alignItems: 'center' }}>
        {(['Rendered', 'Source'] as const).map((label) => {
          const on = (label === 'Source') === source
          return (
            <ButtonBase
              key={label}
              onClick={() => setSource(label === 'Source')}
              sx={{
                px: 1,
                py: 0.25,
                borderRadius: '6px',
                fontSize: '0.6875rem',
                fontWeight: 600,
                color: on ? 'text.primary' : 'text.secondary',
                bgcolor: on ? 'action.selected' : 'transparent',
                '&:hover': { bgcolor: on ? 'action.selected' : 'action.hover' },
              }}
            >
              {label}
            </ButtonBase>
          )
        })}
        {rows.length > MAX_TABLE_ROWS && !source && (
          <Typography variant="caption" color="text.disabled" sx={{ ml: 1 }}>
            First {MAX_TABLE_ROWS.toLocaleString()} rows
          </Typography>
        )}
      </Stack>
      {body}
    </Stack>
  )
}

/** A whole panel given to one file of any kind: the Preview panel's view
 * for everything that isn't an office document. */
export function FileViewer({ path }: { path: string }) {
  return (
    <Box sx={{ flex: 1, minHeight: 0, overflow: 'auto', p: 1.5 }}>
      <FileContent path={path} wrap full fresh />
    </Box>
  )
}
