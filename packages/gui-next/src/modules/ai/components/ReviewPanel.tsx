import { useEffect, useMemo, useState } from 'react'
import { codeToTokens } from 'shiki'
import { Dialog, Box, Stack, Typography, Button, IconButton, Tooltip, ButtonBase, alpha, useTheme } from '../../../ui'
import { ChevronRightLinear, CloseLinear, FolderLinear, MaximizeLinear, MinimizeLinear, UndoLinear } from '../../../icons'
import { displayPath, totals, type FileChange, type Hunk } from '../lib/review'

const MONO = '"Geist Mono", ui-monospace, Consolas, monospace'

export function StatusMark({ status }: { status: FileChange['status'] }) {
  const theme = useTheme()
  const spec = {
    added: { letter: 'A', color: theme.palette.success.main, label: 'Added' },
    modified: { letter: 'M', color: theme.palette.warning.main, label: 'Modified' },
    deleted: { letter: 'D', color: theme.palette.error.main, label: 'Deleted' },
    unchanged: { letter: '=', color: theme.palette.text.disabled, label: 'No net change' },
  }[status]
  return (
    <Box
      component="span"
      title={spec.label}
      aria-label={spec.label}
      sx={{
        width: 16,
        height: 16,
        flexShrink: 0,
        borderRadius: '4px',
        display: 'inline-grid',
        placeItems: 'center',
        fontFamily: MONO,
        fontSize: '0.6875rem',
        fontWeight: 500,
        color: spec.color,
        bgcolor: alpha(spec.color, 0.14),
      }}
    >
      {spec.letter}
    </Box>
  )
}

export function Counts({ additions, deletions }: { additions: number; deletions: number }) {
  return (
    <Box component="span" sx={{ fontFamily: MONO, fontSize: '0.6875rem', fontVariantNumeric: 'tabular-nums', whiteSpace: 'nowrap' }}>
      {additions > 0 && <Box component="span" sx={{ color: 'success.main' }}>+{additions}</Box>}
      {additions > 0 && deletions > 0 && ' '}
      {deletions > 0 && <Box component="span" sx={{ color: 'error.main' }}>−{deletions}</Box>}
    </Box>
  )
}

/** Why a file can't be undone, or null when it can. */
export function undoBlocker(f: FileChange, readOnly: boolean): string | null {
  if (readOnly) return 'History is read-only — undo from today’s conversation.'
  if (f.status === 'unchanged') return 'Nothing to undo — it ends as it started.'
  if (f.modifiedSince) return 'Changed again since — undoing this would erase those later changes too.'
  return null
}

/** A file's language for the highlighter, from its extension. */
function langOf(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = { mjs: 'js', cjs: 'js', yml: 'yaml', htm: 'html', h: 'c', hpp: 'cpp', rs: 'rust', py: 'python', rb: 'ruby', sh: 'bash', ps1: 'powershell', md: 'markdown' }
  return map[ext] ?? (ext || 'text')
}

type Tok = { content: string; color?: string; fontStyle?: number }

/** Each hunk's lines, tokenised by the same highlighter the chat's code
 * blocks use, so a diff reads as code rather than as plain text. A hunk is
 * highlighted as one block, so a string or comment that spans lines keeps
 * its colour. */
function useHunkTokens(file: FileChange, dark: boolean): Tok[][][] | null {
  const [tokens, setTokens] = useState<Tok[][][] | null>(null)
  useEffect(() => {
    let live = true
    setTokens(null)
    const lang = langOf(file.path)
    const theme = dark ? 'github-dark-default' : 'github-light'
    void Promise.all(
      file.hunks.map((h) =>
        codeToTokens(h.lines.map((l) => l.text).join('\n'), { lang: lang as never, theme })
          .then((r) => r.tokens as Tok[][])
          .catch(() => h.lines.map((l) => [{ content: l.text }])),
      ),
    ).then((t) => live && setTokens(t))
    return () => {
      live = false
    }
  }, [file, dark])
  return tokens
}

function DiffView({ file }: { file: FileChange }) {
  const theme = useTheme()
  const dark = theme.palette.mode === 'dark'
  const tokens = useHunkTokens(file, dark)

  if (file.binary || file.tooLarge || file.status === 'unchanged') {
    return (
      <Box sx={{ p: 3 }}>
        <Typography variant="body2" color="text.secondary">
          {file.status === 'unchanged'
            ? 'This file ends the assignment exactly as it started, so there is nothing to show.'
            : file.binary
              ? 'Binary file: its contents can’t be shown as text. Undo still restores it.'
              : 'Too large to show a diff for. Undo still restores it.'}
        </Typography>
      </Box>
    )
  }

  const green = theme.palette.success.main
  const red = theme.palette.error.main
  const addBg = alpha(green, dark ? 0.1 : 0.08)
  const delBg = alpha(red, dark ? 0.1 : 0.07)

  return (
    <Box sx={{ fontFamily: MONO, fontSize: '0.8125rem', lineHeight: '20px', minWidth: 'fit-content', pb: '16px' }}>
      {file.hunks.map((h: Hunk, hi) => {
        const last = h.lines.reduce((n, l) => Math.max(n, l.newNo ?? 0), h.newStart)
        return (
          <Box key={hi} sx={{ mt: hi ? '12px' : 0 }}>
            {/* Where this part of the file is, as a quiet band. */}
            <Stack
              direction="row"
              sx={{ alignItems: 'center', gap: '10px', px: '16px', height: 32, position: 'sticky', left: 0, color: 'text.secondary', bgcolor: alpha(theme.palette.text.primary, 0.035), borderTop: hi ? 1 : 0, borderBottom: 1, borderColor: 'divider' }}
            >
              <Typography component="span" sx={{ fontSize: '0.75rem', fontWeight: 500, color: 'text.secondary', fontFamily: 'inherit' }}>
                Lines {h.newStart}–{last}
              </Typography>
              <Typography component="span" sx={{ fontSize: '0.6875rem', color: 'text.disabled', fontFamily: 'inherit' }}>
                {`@@ -${h.oldStart} +${h.newStart} @@`}
              </Typography>
            </Stack>
            {h.lines.map((l, li) => {
              const kind = l.kind
              const bar = kind === 'add' ? green : kind === 'del' ? red : 'transparent'
              const line = tokens?.[hi]?.[li]
              return (
                <Box
                  key={li}
                  sx={{
                    display: 'grid',
                    gridTemplateColumns: '3px 48px 48px 24px 1fr',
                    bgcolor: kind === 'add' ? addBg : kind === 'del' ? delBg : 'transparent',
                    '&:hover': { bgcolor: kind === 'add' ? alpha(green, dark ? 0.16 : 0.12) : kind === 'del' ? alpha(red, dark ? 0.16 : 0.11) : alpha(theme.palette.text.primary, 0.03) },
                  }}
                >
                  <Box sx={{ bgcolor: bar }} />
                  {/* Line numbers are not selectable, so copying a block
                      copies code rather than gutter numbers. */}
                  <Box sx={{ textAlign: 'right', pr: '10px', color: kind === 'del' ? alpha(red, 0.8) : 'text.disabled', userSelect: 'none', fontSize: '0.6875rem' }}>{l.oldNo ?? ''}</Box>
                  <Box sx={{ textAlign: 'right', pr: '10px', color: kind === 'add' ? alpha(green, 0.85) : 'text.disabled', userSelect: 'none', fontSize: '0.6875rem' }}>{l.newNo ?? ''}</Box>
                  <Box sx={{ userSelect: 'none', textAlign: 'center', fontWeight: 600, color: kind === 'add' ? green : kind === 'del' ? red : 'transparent' }}>{kind === 'add' ? '+' : kind === 'del' ? '−' : ''}</Box>
                  <Box component="pre" sx={{ m: 0, pr: '24px', whiteSpace: 'pre', font: 'inherit', color: 'text.primary', opacity: kind === 'ctx' ? 0.78 : 1 }}>
                    {line ? line.map((t, ti) => (
                      <span key={ti} style={{ color: t.color, fontStyle: t.fontStyle && t.fontStyle & 1 ? 'italic' : undefined }}>
                        {t.content}
                      </span>
                    )) : l.text || ' '}
                  </Box>
                </Box>
              )
            })}
          </Box>
        )
      })}
      {file.truncated && (
        <Box sx={{ p: '16px', color: 'text.secondary', fontFamily: 'inherit' }}>
          Diff cut short here: it is too long to show in full. Undo still restores the whole file.
        </Box>
      )}
    </Box>
  )
}

/** The changed files grouped by folder: a folder line with its counts, its
 * files under it. */
function FileTree({ files, root, selected, onSelect }: { files: FileChange[]; root: string | null; selected: string | null; onSelect: (path: string) => void }) {
  const theme = useTheme()
  const [closed, setClosed] = useState<Set<string>>(new Set())
  const groups = useMemo(() => {
    const by = new Map<string, FileChange[]>()
    for (const f of files) {
      const rel = displayPath(f.path, root)
      const slash = rel.lastIndexOf('/')
      const dir = slash > 0 ? rel.slice(0, slash) : ''
      by.set(dir, [...(by.get(dir) ?? []), f])
    }
    return [...by.entries()].sort(([a], [b]) => a.localeCompare(b))
  }, [files, root])

  return (
    <Box sx={{ py: '8px' }}>
      {groups.map(([dir, list]) => {
        const open = !closed.has(dir)
        const t = totals(list)
        return (
          <Box key={dir || '.'}>
            {dir && (
              <ButtonBase
                onClick={() => setClosed((c) => { const n = new Set(c); if (n.has(dir)) n.delete(dir); else n.add(dir); return n })}
                title={dir}
                sx={{ display: 'flex', width: '100%', justifyContent: 'flex-start', gap: '6px', px: '12px', height: 32, color: 'text.secondary', '&:hover': { color: 'text.primary', bgcolor: 'action.hover' } }}
              >
                <Box sx={{ display: 'flex', transform: open ? 'rotate(90deg)' : 'none', transition: 'transform 0.12s', opacity: 0.7 }}>
                  <ChevronRightLinear size={11} color="currentColor" />
                </Box>
                <FolderLinear size={14} color="currentColor" />
                <Typography sx={{ flex: 1, minWidth: 0, textAlign: 'left', fontSize: '0.75rem', fontWeight: 500, direction: 'rtl' }} noWrap>
                  {dir}
                </Typography>
                <Counts additions={t.additions} deletions={t.deletions} />
              </ButtonBase>
            )}
            {open &&
              list.map((f) => {
                const rel = displayPath(f.path, root)
                const name = rel.slice(rel.lastIndexOf('/') + 1)
                const active = f.path === selected
                return (
                  <ButtonBase
                    key={f.path}
                    onClick={() => onSelect(f.path)}
                    aria-current={active ? 'true' : undefined}
                    title={rel}
                    sx={{
                      display: 'flex',
                      width: '100%',
                      justifyContent: 'flex-start',
                      gap: '8px',
                      pl: dir ? '36px' : '12px',
                      pr: '12px',
                      height: 32,
                      position: 'relative',
                      bgcolor: active ? alpha(theme.palette.text.primary, 0.08) : 'transparent',
                      opacity: f.status === 'unchanged' ? 0.55 : 1,
                      '&::before': active ? { content: '""', position: 'absolute', left: 0, top: 6, bottom: 6, width: 2, borderRadius: 1.6, bgcolor: 'text.primary' } : {},
                      '&:hover': { bgcolor: active ? alpha(theme.palette.text.primary, 0.1) : 'action.hover' },
                    }}
                  >
                    <StatusMark status={f.status} />
                    <Typography sx={{ flex: 1, minWidth: 0, textAlign: 'left', fontFamily: MONO, fontSize: '0.75rem', fontWeight: active ? 600 : 400 }} noWrap>
                      {name}
                    </Typography>
                    <Counts additions={f.additions} deletions={f.deletions} />
                  </ButtonBase>
                )
              })}
          </Box>
        )
      })}
    </Box>
  )
}

/**
 * The enlarged review of one assignment's changes.
 *
 * Opens as a large panel and maximizes to the whole window — a diff of a
 * rewritten component needs the width, and the chat column is the wrong place
 * to read one. Undo lives here as well as on the summary card, per file and
 * for everything, because this is where the decision gets made.
 */
export function ReviewPanel({
  open,
  onClose,
  files,
  root,
  readOnly,
  busy,
  onUndo,
  note,
}: {
  open: boolean
  onClose: () => void
  files: FileChange[]
  root: string | null
  readOnly: boolean
  busy: boolean
  onUndo: (paths: string[]) => void
  /** Status after an undo, rendered in the toolbar. */
  note: React.ReactNode
}) {
  const [maximized, setMaximized] = useState(false)
  const [selected, setSelected] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    // Keep the selection across refreshes (an undo re-fetches), but land on
    // the first real change when opening fresh.
    setSelected((cur) =>
      cur && files.some((f) => f.path === cur) ? cur : (files.find((f) => f.status !== 'unchanged') ?? files[0])?.path ?? null,
    )
  }, [open, files])

  const file = files.find((f) => f.path === selected) ?? null
  const changed = files.filter((f) => f.status !== 'unchanged')
  const { additions, deletions } = totals(changed)
  const undoable = files.filter((f) => !undoBlocker(f, readOnly)).map((f) => f.path)
  const fileBlocker = file ? undoBlocker(file, readOnly) : null

  return (
    <Dialog
      open={open}
      onClose={onClose}
      fullScreen={maximized}
      maxWidth={false}
      slotProps={{
        paper: {
          sx: maximized
            ? {}
            : { width: 'min(1200px, 94vw)', height: 'min(820px, 88vh)', maxWidth: 'none', maxHeight: 'none' },
        },
      }}
    >
      <Stack sx={{ height: '100%', minHeight: 0 }}>
        <Stack
          direction="row"
          sx={{ alignItems: 'center', gap: '12px', px: '20px', py: '14px', borderBottom: 1, borderColor: 'divider', flexWrap: 'wrap' }}
        >
          <Stack sx={{ gap: '2px' }}>
            <Typography sx={{ fontWeight: 600, fontSize: '1rem', lineHeight: 1.3 }}>Review changes</Typography>
            <Stack direction="row" sx={{ alignItems: 'center', gap: '8px' }}>
              <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>
                {changed.length} file{changed.length === 1 ? '' : 's'} changed
              </Typography>
              <Counts additions={additions} deletions={deletions} />
            </Stack>
          </Stack>
          <Box sx={{ flex: 1 }} />
          {note}
          {!readOnly && (
            <Button
              size="small"
              color="error"
              startIcon={<UndoLinear size={13} color="currentColor" />}
              disabled={busy || undoable.length === 0}
              onClick={() => onUndo(undoable)}
              sx={{ textTransform: 'none', borderRadius: '8px', bgcolor: (t) => alpha(t.palette.error.main, 0.1), '&:hover': { bgcolor: (t) => alpha(t.palette.error.main, 0.18) } }}
            >
              Undo all
            </Button>
          )}
          <Tooltip title={maximized ? 'Restore size' : 'Maximize'}>
            <IconButton size="small" aria-label={maximized ? 'Restore size' : 'Maximize'} onClick={() => setMaximized((v) => !v)}>
              {maximized ? <MinimizeLinear size={15} color="currentColor" /> : <MaximizeLinear size={15} color="currentColor" />}
            </IconButton>
          </Tooltip>
          <Tooltip title="Close">
            <IconButton size="small" aria-label="Close review" onClick={onClose}>
              <CloseLinear size={15} color="currentColor" />
            </IconButton>
          </Tooltip>
        </Stack>

        <Stack direction={{ xs: 'column', md: 'row' }} sx={{ flex: 1, minHeight: 0 }}>
          <Box
            sx={{
              width: { xs: '100%', md: 280 },
              maxHeight: { xs: 180, md: 'none' },
              flexShrink: 0,
              overflowY: 'auto',
              borderRight: { md: 1 },
              borderBottom: { xs: 1, md: 0 },
              borderColor: 'divider',
            }}
          >
            <FileTree files={files} root={root} selected={selected} onSelect={setSelected} />
          </Box>

          <Stack sx={{ flex: 1, minWidth: 0, minHeight: 0 }}>
            {file && (
              <Stack
                direction="row"
                sx={{ alignItems: 'center', gap: '10px', px: '16px', height: 44, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}
              >
                <StatusMark status={file.status} />
                <Typography sx={{ fontFamily: MONO, fontSize: '0.8125rem', fontWeight: 500, minWidth: 0, flex: 1 }} noWrap title={file.path}>
                  {displayPath(file.path, root)}
                </Typography>
                {file.modifiedSince && (
                  <Typography variant="caption" sx={{ color: 'warning.main', flexShrink: 0 }}>
                    Changed again since
                  </Typography>
                )}
                {!readOnly && (
                  <Tooltip title={fileBlocker ?? 'Restore this file to how it was before this assignment'}>
                    <span>
                      <Button
                        size="small"
                        startIcon={<UndoLinear size={13} color="currentColor" />}
                        disabled={busy || !!fileBlocker}
                        onClick={() => onUndo([file.path])}
                        sx={{ textTransform: 'none' }}
                      >
                        Undo file
                      </Button>
                    </span>
                  </Tooltip>
                )}
              </Stack>
            )}
            <Box sx={{ flex: 1, minHeight: 0, overflow: 'auto' }}>{file && <DiffView file={file} />}</Box>
          </Stack>
        </Stack>
      </Stack>
    </Dialog>
  )
}
