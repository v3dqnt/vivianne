import { useEffect, useState } from 'react'
import { Dialog, Box, Stack, Typography, Button, IconButton, Tooltip, ButtonBase, alpha, useTheme } from '../../../ui'
import { CloseLinear, MaximizeLinear, MinimizeLinear, UndoLinear } from '../../../icons'
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
        fontSize: '0.625rem',
        fontWeight: 700,
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

function DiffView({ file }: { file: FileChange }) {
  const theme = useTheme()

  if (file.binary || file.tooLarge || file.status === 'unchanged') {
    return (
      <Box sx={{ p: 3 }}>
        <Typography variant="body2" color="text.secondary">
          {file.status === 'unchanged'
            ? 'This file ends the assignment exactly as it started, so there is nothing to show.'
            : file.binary
              ? 'Binary file — its contents can’t be shown as text. Undo still restores it.'
              : 'Too large to show a diff for. Undo still restores it.'}
        </Typography>
      </Box>
    )
  }

  const addBg = alpha(theme.palette.success.main, theme.palette.mode === 'dark' ? 0.13 : 0.1)
  const delBg = alpha(theme.palette.error.main, theme.palette.mode === 'dark' ? 0.13 : 0.09)

  return (
    <Box sx={{ fontFamily: MONO, fontSize: '0.75rem', lineHeight: 1.6, minWidth: 'fit-content' }}>
      {file.hunks.map((h: Hunk, hi) => (
        <Box key={hi}>
          <Box
            sx={{
              px: 1.5,
              py: 0.25,
              color: 'text.secondary',
              bgcolor: alpha(theme.palette.info.main, 0.08),
              borderTop: hi > 0 ? 1 : 0,
              borderBottom: 1,
              borderColor: 'divider',
              position: 'sticky',
              left: 0,
            }}
          >
            {`@@ −${h.oldStart} +${h.newStart} @@`}
          </Box>
          {h.lines.map((l, li) => (
            <Box
              key={li}
              sx={{
                display: 'grid',
                gridTemplateColumns: '3.5em 3.5em 1.5em 1fr',
                bgcolor: l.kind === 'add' ? addBg : l.kind === 'del' ? delBg : 'transparent',
              }}
            >
              {/* Line numbers are not selectable, so copying a block from the
                  diff copies code rather than gutter numbers. */}
              <Box sx={{ textAlign: 'right', pr: 1, color: 'text.disabled', userSelect: 'none' }}>{l.oldNo ?? ''}</Box>
              <Box sx={{ textAlign: 'right', pr: 1, color: 'text.disabled', userSelect: 'none' }}>{l.newNo ?? ''}</Box>
              <Box
                sx={{
                  userSelect: 'none',
                  textAlign: 'center',
                  color: l.kind === 'add' ? 'success.main' : l.kind === 'del' ? 'error.main' : 'text.disabled',
                }}
              >
                {l.kind === 'add' ? '+' : l.kind === 'del' ? '−' : ''}
              </Box>
              <Box component="pre" sx={{ m: 0, pr: 2, whiteSpace: 'pre', font: 'inherit' }}>
                {l.text || ' '}
              </Box>
            </Box>
          ))}
        </Box>
      ))}
      {file.truncated && (
        <Box sx={{ p: 1.5, color: 'text.secondary', fontFamily: 'inherit' }}>
          Diff cut short here — it is too long to show in full. Undo still restores the whole file.
        </Box>
      )}
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
  const theme = useTheme()
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
          sx={{ alignItems: 'center', gap: 1.5, px: 2, py: 1.25, borderBottom: 1, borderColor: 'divider', flexWrap: 'wrap' }}
        >
          <Typography sx={{ fontWeight: 700, fontSize: '0.9375rem' }}>Review changes</Typography>
          <Typography variant="caption" color="text.secondary">
            {changed.length} file{changed.length === 1 ? '' : 's'}
          </Typography>
          <Counts additions={additions} deletions={deletions} />
          <Box sx={{ flex: 1 }} />
          {note}
          {!readOnly && (
            <Button
              size="small"
              color="error"
              variant="outlined"
              startIcon={<UndoLinear size={13} color="currentColor" />}
              disabled={busy || undoable.length === 0}
              onClick={() => onUndo(undoable)}
              sx={{ textTransform: 'none' }}
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
              width: { xs: '100%', md: 300 },
              maxHeight: { xs: 180, md: 'none' },
              flexShrink: 0,
              overflowY: 'auto',
              borderRight: { md: 1 },
              borderBottom: { xs: 1, md: 0 },
              borderColor: 'divider',
              py: 0.5,
            }}
          >
            {files.map((f) => {
              const rel = displayPath(f.path, root)
              const slash = rel.lastIndexOf('/')
              const active = f.path === selected
              return (
                <ButtonBase
                  key={f.path}
                  onClick={() => setSelected(f.path)}
                  aria-current={active ? 'true' : undefined}
                  sx={{
                    display: 'flex',
                    width: '100%',
                    alignItems: 'center',
                    gap: 1,
                    px: 1.5,
                    py: 0.625,
                    justifyContent: 'flex-start',
                    textAlign: 'left',
                    bgcolor: active ? alpha(theme.palette.primary.main, 0.12) : 'transparent',
                    opacity: f.status === 'unchanged' ? 0.6 : 1,
                    '&:hover': { bgcolor: active ? alpha(theme.palette.primary.main, 0.16) : 'action.hover' },
                  }}
                >
                  <StatusMark status={f.status} />
                  <Stack sx={{ minWidth: 0, flex: 1 }}>
                    <Typography sx={{ fontFamily: MONO, fontSize: '0.75rem', fontWeight: 600 }} noWrap title={rel}>
                      {rel.slice(slash + 1)}
                    </Typography>
                    {slash > 0 && (
                      <Typography sx={{ fontFamily: MONO, fontSize: '0.6875rem', color: 'text.secondary' }} noWrap title={rel}>
                        {rel.slice(0, slash)}
                      </Typography>
                    )}
                  </Stack>
                  <Counts additions={f.additions} deletions={f.deletions} />
                </ButtonBase>
              )
            })}
          </Box>

          <Stack sx={{ flex: 1, minWidth: 0, minHeight: 0 }}>
            {file && (
              <Stack
                direction="row"
                sx={{ alignItems: 'center', gap: 1, px: 2, py: 0.75, borderBottom: 1, borderColor: 'divider' }}
              >
                <StatusMark status={file.status} />
                <Typography sx={{ fontFamily: MONO, fontSize: '0.75rem', minWidth: 0, flex: 1 }} noWrap title={file.path}>
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
