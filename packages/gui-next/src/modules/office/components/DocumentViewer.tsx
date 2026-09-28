import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react'
import { Box, Stack, Typography, Button, IconButton, InputBase, Tooltip, CircularProgress, ToggleButton, ToggleButtonGroup, alpha } from '../../../ui'
import { RefreshLinear, EditLinear, SparkleLinear } from '../../../icons'
import { useAgentStore } from '../../ai/store/agentStore'
// Each renderer is its own chunk: opening a PDF shouldn't download the
// PowerPoint engine (which brings a charting library with it).
const PdfView = lazy(() => import('./PdfView').then((m) => ({ default: m.PdfView })))
const DocxView = lazy(() => import('./DocxView').then((m) => ({ default: m.DocxView })))
const SheetView = lazy(() => import('./SheetView').then((m) => ({ default: m.SheetView })))
const SlidesView = lazy(() => import('./SlidesView').then((m) => ({ default: m.SlidesView })))
import { canEdit, editPart, extOf, fileName, inspect, kindOf, readBytes, restore, type DocPart } from '../lib/docs'

const MAX_UNDO = 20

/**
 * One document, open in a workspace panel: a faithful preview, and — for
 * Word, Excel and PowerPoint — editing in place.
 *
 * Every edit goes through the backend's part-level XML edit, the same one the
 * agent's `edit_document` tool uses, so formatting, formulas and images
 * survive a hand edit exactly as they survive an agent's. Before each edit the
 * current bytes are kept, which is what Undo restores.
 */
export function DocumentViewer({ path }: { path: string }) {
  const kind = kindOf(path)
  const editable = canEdit(path)
  const [data, setData] = useState<{ base64: string; bytes: Uint8Array } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [mode, setMode] = useState<'preview' | 'edit'>('preview')
  const [history, setHistory] = useState<string[]>([])
  const [busy, setBusy] = useState(false)
  const setDraft = useAgentStore((s) => s.setDraft)

  const load = useCallback(async () => {
    setError(null)
    try {
      setData(await readBytes(path))
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [path])

  useEffect(() => {
    setData(null)
    setHistory([])
    void load()
  }, [load])

  // The agent may have just edited this file; show what's on disk now.
  const agentBusy = useAgentStore((s) => s.isBusy)
  const wasBusy = useRef(agentBusy)
  useEffect(() => {
    if (wasBusy.current && !agentBusy) void load()
    wasBusy.current = agentBusy
  }, [agentBusy, load])

  /** Run one edit, remembering the bytes it replaces. */
  const applyEdit = useCallback(
    async (part: string, text: string) => {
      if (!data) return
      setBusy(true)
      setError(null)
      try {
        const before = data.base64
        await editPart(path, part, text)
        setHistory((h) => [...h.slice(-(MAX_UNDO - 1)), before])
        await load()
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e))
        throw e
      } finally {
        setBusy(false)
      }
    },
    [data, path, load],
  )

  const undo = async () => {
    const prev = history[history.length - 1]
    if (!prev) return
    setBusy(true)
    try {
      await restore(path, prev)
      setHistory((h) => h.slice(0, -1))
      await load()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const askAgent = () => setDraft(`In ${path}: `)

  const sheetEdit = editable && kind === 'sheet' ? (sheet: string, ref: string, value: string) => applyEdit(`${sheet}!${ref}`, value) : undefined

  return (
    <Stack className="select-text" sx={{ height: '100%', minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.25, py: 0.75, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <Stack sx={{ minWidth: 0, flex: 1 }}>
          <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem' }} noWrap title={path}>
            {fileName(path)}
          </Typography>
          <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }} noWrap>
            {extOf(path).toUpperCase()}
            {editable ? '' : ' · preview only'}
            {busy ? ' · saving…' : ''}
          </Typography>
        </Stack>
        {editable && kind !== 'sheet' && (
          <ToggleButtonGroup
            size="small"
            exclusive
            value={mode}
            onChange={(_, v) => v && setMode(v)}
            sx={{ '& .ui-ToggleButton': { py: 0.25, px: 1, fontSize: '0.75rem', textTransform: 'none', borderRadius: '6px' } }}
          >
            <ToggleButton value="preview">Preview</ToggleButton>
            <ToggleButton value="edit">Edit</ToggleButton>
          </ToggleButtonGroup>
        )}
        <Tooltip title={history.length ? `Undo your last edit (${history.length})` : 'Nothing to undo'}>
          <span>
            <Button size="small" color="inherit" disabled={!history.length || busy} onClick={() => void undo()} sx={{ minWidth: 0, fontSize: '0.75rem', borderRadius: '6px' }}>
              Undo
            </Button>
          </span>
        </Tooltip>
        <Tooltip title="Reload from disk">
          <IconButton size="small" onClick={() => void load()}>
            <RefreshLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
        <Tooltip title="Ask Vivianne to change this document">
          <IconButton size="small" onClick={askAgent}>
            <SparkleLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Stack>

      {error && (
        <Typography variant="caption" color="error.main" sx={{ px: 1.5, py: 0.75, borderBottom: 1, borderColor: 'divider' }}>
          {error}
        </Typography>
      )}

      <Suspense
        fallback={
          <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}>
            <CircularProgress size={18} />
          </Stack>
        }
      >
        {!data ? (
          <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}>{!error && <CircularProgress size={18} />}</Stack>
        ) : mode === 'edit' && editable && kind !== 'sheet' ? (
          <EditParts path={path} version={data.base64.length + ':' + data.base64.slice(-32)} onSave={applyEdit} />
        ) : kind === 'pdf' ? (
          <PdfView bytes={data.bytes} />
        ) : kind === 'word' ? (
          <DocxView bytes={data.bytes} />
        ) : kind === 'slides' ? (
          <SlidesView bytes={data.bytes} />
        ) : kind === 'sheet' ? (
          <SheetView bytes={data.bytes} onEditCell={sheetEdit} />
        ) : (
          <Typography variant="caption" color="text.secondary" sx={{ p: 2 }}>
            This file type can’t be previewed.
          </Typography>
        )}
      </Suspense>
    </Stack>
  )
}

/** Word paragraphs or slide text boxes, each editable where it stands. */
function EditParts({ path, version, onSave }: { path: string; version: string; onSave: (part: string, text: string) => Promise<void> }) {
  const [parts, setParts] = useState<DocPart[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let alive = true
    inspect(path)
      .then((p) => alive && setParts(p))
      .catch((e: unknown) => alive && setError(e instanceof Error ? e.message : String(e)))
    return () => {
      alive = false
    }
  }, [path, version])

  if (error) {
    return (
      <Typography variant="caption" color="error.main" sx={{ p: 2 }}>
        {error}
      </Typography>
    )
  }
  if (!parts) {
    return (
      <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center' }}>
        <CircularProgress size={18} />
      </Stack>
    )
  }

  let lastGroup = ''
  return (
    <Box sx={{ flex: 1, overflow: 'auto', px: 1.5, py: 1 }}>
      <Typography variant="caption" color="text.secondary" sx={{ display: 'block', pb: 1 }}>
        Edit text in place — formatting, styles and images are kept. Press Save on a block to write it to the file.
      </Typography>
      <Stack sx={{ gap: 0.75 }}>
        {parts.map((p) => {
          // Slides are grouped under a heading; paragraphs just flow.
          const group = p.id.startsWith('slide:') ? `Slide ${p.id.split(':')[1]}` : ''
          const heading = group && group !== lastGroup ? group : ''
          lastGroup = group || lastGroup
          return (
            <Box key={p.id}>
              {heading && (
                <Typography variant="caption" sx={{ display: 'block', fontWeight: 600, color: 'text.secondary', pt: 1, pb: 0.5 }}>
                  {heading}
                </Typography>
              )}
              <PartEditor part={p} onSave={onSave} />
            </Box>
          )
        })}
      </Stack>
    </Box>
  )
}

function PartEditor({ part, onSave }: { part: DocPart; onSave: (part: string, text: string) => Promise<void> }) {
  const [text, setText] = useState(part.text)
  const [saving, setSaving] = useState(false)
  useEffect(() => setText(part.text), [part.text])
  const dirty = text !== part.text
  const empty = !part.text.trim()

  return (
    <Stack
      direction="row"
      sx={(t) => ({
        gap: 0.75,
        alignItems: 'flex-start',
        p: 0.75,
        borderRadius: '8px',
        border: 1,
        borderColor: dirty ? 'primary.main' : 'transparent',
        bgcolor: dirty ? alpha(t.palette.primary.main, 0.05) : 'transparent',
        '&:hover': { borderColor: dirty ? 'primary.main' : 'divider' },
        opacity: empty ? 0.55 : 1,
      })}
    >
      <Tooltip title={part.label}>
        <Typography variant="caption" color="text.disabled" sx={{ fontFamily: '"Geist Mono", monospace', fontSize: '0.625rem', pt: 0.5, minWidth: 44 }}>
          {part.id}
        </Typography>
      </Tooltip>
      <InputBase
        multiline
        fullWidth
        disabled={!part.editable || saving}
        value={text}
        placeholder={empty ? '(empty)' : undefined}
        onChange={(e) => setText(e.target.value)}
        sx={{ fontSize: '0.8125rem', lineHeight: 1.55, p: 0 }}
      />
      {dirty && (
        <Stack direction="row" sx={{ gap: 0.25, flexShrink: 0 }}>
          <Button
            size="small"
            variant="contained"
            disableElevation
            disabled={saving}
            startIcon={saving ? <CircularProgress size={12} /> : <EditLinear size={12} color="currentColor" />}
            onClick={async () => {
              setSaving(true)
              try {
                await onSave(part.id, text)
              } catch {
                // the viewer shows the error
              } finally {
                setSaving(false)
              }
            }}
            sx={{ minWidth: 0, fontSize: '0.7rem', py: 0.125, borderRadius: '6px' }}
          >
            Save
          </Button>
          <Button size="small" color="inherit" disabled={saving} onClick={() => setText(part.text)} sx={{ minWidth: 0, fontSize: '0.7rem', py: 0.125, borderRadius: '6px' }}>
            Reset
          </Button>
        </Stack>
      )}
    </Stack>
  )
}
