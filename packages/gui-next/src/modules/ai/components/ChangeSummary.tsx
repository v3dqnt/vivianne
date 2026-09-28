import { useEffect, useState } from 'react'
import { create } from 'zustand'
import { Box, Stack, Typography, Button, IconButton, Tooltip, ButtonBase, alpha, useTheme } from '../../../ui'
import { MaximizeLinear, UndoLinear, RedoLinear } from '../../../icons'
import { useWorkspaceStore } from '../store/workspaceStore'
import { useAgentStore } from '../store/agentStore'
import {
  displayPath,
  loadChanges,
  redo,
  totals,
  undoFiles,
  type Assignment,
  type AssignmentChanges,
  type UndoReport,
} from '../lib/review'
import { Counts, ReviewPanel, StatusMark, undoBlocker } from './ReviewPanel'

/** How many files the card lists before "and N more" sends you to the panel. */
const INLINE_FILES = 6

/**
 * Bumped whenever files may have changed under an existing card — after any
 * undo or redo, and whenever a turn finishes. Every card re-fetches on a bump:
 * an undo in one assignment can change whether an older assignment's files
 * count as "changed since", and a card that kept its stale answer would offer
 * an undo the backend would then refuse.
 */
const useReviewVersion = create<{ v: number; bump: () => void }>((set) => ({
  v: 0,
  bump: () => set((s) => ({ v: s.v + 1 })),
}))

let wasBusy = false
useAgentStore.subscribe((s) => {
  if (wasBusy && !s.isBusy) useReviewVersion.getState().bump()
  wasBusy = s.isBusy
})

/**
 * What one assignment changed, shown under its last message.
 *
 * Only rendered for assignments that called a file-editing tool, so an
 * ordinary question-and-answer turn gets no card and costs no query.
 */
export function ChangeSummary({
  assignment,
  sessionId,
  readOnly,
}: {
  assignment: Assignment
  /** Set when viewing a past day, whose checkpoints live under its own id. */
  sessionId?: string
  readOnly: boolean
}) {
  const theme = useTheme()
  const root = useWorkspaceStore((s) => s.folders[0] ?? null)
  const version = useReviewVersion((s) => s.v)
  const bump = useReviewVersion((s) => s.bump)

  const [changes, setChanges] = useState<AssignmentChanges | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [panelOpen, setPanelOpen] = useState(false)
  const [lastUndo, setLastUndo] = useState<UndoReport | null>(null)

  const { fromMs, toMs } = assignment

  useEffect(() => {
    let cancelled = false
    loadChanges({ fromMs, toMs }, sessionId)
      .then((c) => {
        if (!cancelled) {
          setChanges(c)
          setError(null)
        }
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e))
      })
    return () => {
      cancelled = true
    }
  }, [fromMs, toMs, sessionId, version])

  const undo = async (paths: string[]) => {
    setBusy(true)
    try {
      const report = await undoFiles({ fromMs, toMs }, paths)
      setLastUndo(report)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
      bump()
    }
  }

  const redoLast = async () => {
    if (!lastUndo?.redoCheckpointId) return
    setBusy(true)
    try {
      await redo(lastUndo.redoCheckpointId, [...lastUndo.restored, ...lastUndo.deleted])
      setLastUndo(null)
    } finally {
      setBusy(false)
      bump()
    }
  }

  if (error) {
    return (
      <Typography variant="caption" color="text.disabled" sx={{ display: 'block', pl: 0.5 }}>
        Couldn’t load this assignment’s changes: {error}
      </Typography>
    )
  }
  // An assignment that called an edit tool can still end with nothing on
  // record — every call failed, or it only ever wrote a file back to how it
  // was. Nothing to review, so nothing shown.
  if (!changes || changes.files.length === 0) return null

  const changed = changes.files.filter((f) => f.status !== 'unchanged')
  const { additions, deletions } = totals(changed)
  const undoable = changes.files.filter((f) => !undoBlocker(f, readOnly)).map((f) => f.path)
  const undoneCount = lastUndo ? lastUndo.restored.length + lastUndo.deleted.length : 0

  const undoNote = lastUndo && (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, flexWrap: 'wrap' }}>
      <Typography variant="caption" color="text.secondary">
        {undoneCount > 0 ? `Undid ${undoneCount} file${undoneCount === 1 ? '' : 's'}.` : 'Nothing was undone.'}
        {lastUndo.skipped.length > 0 && ` Skipped ${lastUndo.skipped.length}: ${lastUndo.skipped[0][1]}.`}
        {lastUndo.failed.length > 0 && ` ${lastUndo.failed.length} failed: ${lastUndo.failed[0][1]}.`}
      </Typography>
      {lastUndo.redoCheckpointId && undoneCount > 0 && (
        <Button
          size="small"
          startIcon={<RedoLinear size={12} color="currentColor" />}
          onClick={() => void redoLast()}
          disabled={busy}
          sx={{ textTransform: 'none', py: 0, minHeight: 0, fontSize: '0.75rem' }}
        >
          Redo
        </Button>
      )}
    </Stack>
  )

  return (
    <Box
      sx={{
        border: 1,
        borderColor: 'divider',
        borderRadius: '12px',
        bgcolor: alpha(theme.palette.text.primary, 0.02),
        overflow: 'hidden',
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.5, py: 0.875 }}>
        <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem' }}>
          {changed.length === 0
            ? 'No net changes'
            : `Changed ${changed.length} file${changed.length === 1 ? '' : 's'}`}
        </Typography>
        <Counts additions={additions} deletions={deletions} />
        <Box sx={{ flex: 1 }} />
        <Button
          size="small"
          onClick={() => setPanelOpen(true)}
          startIcon={<MaximizeLinear size={12} color="currentColor" />}
          sx={{ textTransform: 'none', fontSize: '0.75rem', py: 0.25 }}
        >
          Review
        </Button>
      </Stack>

      <Stack sx={{ borderTop: 1, borderColor: 'divider', py: 0.5 }}>
        {changes.files.slice(0, INLINE_FILES).map((f) => {
          const blocker = undoBlocker(f, readOnly)
          return (
            <Stack
              key={f.path}
              direction="row"
              sx={{ alignItems: 'center', gap: 1, px: 1.5, py: 0.25, opacity: f.status === 'unchanged' ? 0.55 : 1 }}
            >
              <StatusMark status={f.status} />
              <ButtonBase
                onClick={() => setPanelOpen(true)}
                sx={{ minWidth: 0, flex: 1, justifyContent: 'flex-start', borderRadius: '4px' }}
              >
                <Typography
                  sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.75rem' }}
                  noWrap
                  title={f.path}
                >
                  {displayPath(f.path, root)}
                </Typography>
              </ButtonBase>
              <Counts additions={f.additions} deletions={f.deletions} />
              {!readOnly && (
                <Tooltip title={blocker ?? 'Undo this file'}>
                  <span>
                    <IconButton
                      size="small"
                      aria-label={`Undo ${displayPath(f.path, root)}`}
                      disabled={busy || !!blocker}
                      onClick={() => void undo([f.path])}
                      sx={{ p: 0.375 }}
                    >
                      <UndoLinear size={12} color="currentColor" />
                    </IconButton>
                  </span>
                </Tooltip>
              )}
            </Stack>
          )
        })}
        {changes.files.length > INLINE_FILES && (
          <ButtonBase onClick={() => setPanelOpen(true)} sx={{ justifyContent: 'flex-start', px: 1.5, py: 0.5 }}>
            <Typography variant="caption" color="primary.main">
              and {changes.files.length - INLINE_FILES} more — open review
            </Typography>
          </ButtonBase>
        )}
      </Stack>

      {(!readOnly && undoable.length > 1) || lastUndo || assignment.ranShell || changes.possiblyIncomplete ? (
        <Stack sx={{ borderTop: 1, borderColor: 'divider', px: 1.5, py: 0.75, gap: 0.5 }}>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1, flexWrap: 'wrap' }}>
            {!readOnly && undoable.length > 1 && (
              <Button
                size="small"
                color="error"
                startIcon={<UndoLinear size={12} color="currentColor" />}
                disabled={busy}
                onClick={() => void undo(undoable)}
                sx={{ textTransform: 'none', fontSize: '0.75rem', py: 0 }}
              >
                Undo all
              </Button>
            )}
            {undoNote}
          </Stack>
          {/* Said plainly because it is the review's one real blind spot: a
              list that silently omits files a shell command rewrote would look
              complete and not be. */}
          {assignment.ranShell && (
            <Typography variant="caption" color="text.disabled" sx={{ lineHeight: 1.4 }}>
              Shell commands also ran in this assignment. Files they changed aren’t listed here and can’t be undone
              from here.
            </Typography>
          )}
          {changes.possiblyIncomplete && (
            <Typography variant="caption" sx={{ color: 'warning.main', lineHeight: 1.4 }}>
              This was a long day of edits and the oldest snapshots have been cleared, so “before” may not be the true
              starting point for every file.
            </Typography>
          )}
        </Stack>
      ) : null}

      <ReviewPanel
        open={panelOpen}
        onClose={() => setPanelOpen(false)}
        files={changes.files}
        root={root}
        readOnly={readOnly}
        busy={busy}
        onUndo={(paths) => void undo(paths)}
        note={undoNote}
      />
    </Box>
  )
}
