import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Box, Button, ButtonBase, Chip, Collapse, IconButton, MenuItem, Stack, TextField, Tooltip, Typography, alpha, useTheme } from '../../../ui'
import { CloseLinear, MessageSquareLinear, TickCircleLinear, DangerLinear, ArrowDownLinear } from '../../../icons'
import { CircularProgress } from '../../../ui'
import { Markdown } from '../../ai/components/Markdown'
import { usePlanStore, type Plan, type PlanComment, type PlanStep, type StepState } from '../store/planStore'

const STATUS_LABEL: Record<string, string> = {
  review: 'Awaiting review',
  approved: 'Approved',
  executing: 'In progress',
  done: 'Done',
  failed: 'Needs attention',
  cancelled: 'Stopped',
}

/** What a part of the plan looked like in the previous version, to mark
 * the parts a revision changed. `undefined` = new in this version. */
function previousText(previous: Plan | null, id: string): string | undefined {
  if (!previous) return undefined
  const s = previous.sections.find((x) => x.id === id)
  if (s) return `${s.heading}\n${s.body}`
  const st = previous.steps.find((x) => x.id === id)
  if (st) return `${st.title}\n${st.detail}\n${st.owner ?? ''}\n${st.dependsOn.join(',')}`
  return undefined
}

/** A comment being written: which part, and the passage quoted, if any. */
interface Draft {
  targetId: string
  targetLabel: string
  quote: string | null
}

/**
 * One commentable part of the plan — a section or a step.
 *
 * Hovering shows a Comment button for the whole part; selecting text inside
 * it offers to comment on just that passage, quoted. Comments already made
 * sit under the part they're about, so review reads like margin notes.
 */
function Part({
  id,
  label,
  changed,
  isNew,
  canComment,
  onDraft,
  children,
}: {
  id: string
  label: string
  changed: boolean
  isNew: boolean
  canComment: boolean
  onDraft: (d: Draft) => void
  children: ReactNode
}) {
  const theme = useTheme()
  const ref = useRef<HTMLDivElement>(null)
  const [selection, setSelection] = useState<{ text: string; top: number } | null>(null)
  const comments = usePlanStore((s) => (s.current ? (s.comments[s.current.plan.id] ?? []) : []))
  const remove = usePlanStore((s) => s.removeComment)
  const mine = comments.map((c, i) => ({ c, i })).filter(({ c }) => c.targetId === id)

  const onMouseUp = () => {
    if (!canComment) return
    const sel = window.getSelection()
    const text = sel?.toString().trim() ?? ''
    if (!sel || !text || !ref.current || !sel.anchorNode || !ref.current.contains(sel.anchorNode)) {
      setSelection(null)
      return
    }
    const rect = sel.getRangeAt(0).getBoundingClientRect()
    const box = ref.current.getBoundingClientRect()
    setSelection({ text, top: rect.bottom - box.top + 4 })
  }

  return (
    <Box
      ref={ref}
      onMouseUp={onMouseUp}
      sx={{
        position: 'relative',
        pl: 1.5,
        pr: 1,
        py: 1,
        ml: -1.5,
        borderLeft: 2,
        borderColor: changed ? 'primary.main' : 'transparent',
        borderRadius: '0 8px 8px 0',
        transition: 'background-color 0.12s',
        '&:hover': canComment ? { bgcolor: alpha(theme.palette.text.primary, 0.025) } : undefined,
        '&:hover .plan-comment-btn, & .plan-comment-btn:focus-visible': { opacity: 1 },
      }}
    >
      {(changed || isNew) && (
        <Typography variant="caption" sx={{ color: 'primary.main', fontWeight: 600, display: 'block', mb: 0.25 }}>
          {isNew ? 'New in this version' : 'Changed in this version'}
        </Typography>
      )}
      {canComment && (
        <Tooltip title={`Comment on ${label}`}>
          <IconButton
            className="plan-comment-btn"
            aria-label={`Comment on ${label}`}
            size="small"
            onClick={() => onDraft({ targetId: id, targetLabel: label, quote: null })}
            sx={{ position: 'absolute', top: 6, right: 4, opacity: 0, transition: 'opacity 0.12s', color: 'text.secondary' }}
          >
            <MessageSquareLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
      )}
      {children}
      {selection && (
        <Button
          size="small"
          variant="contained"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => {
            onDraft({ targetId: id, targetLabel: label, quote: selection.text })
            setSelection(null)
            window.getSelection()?.removeAllRanges()
          }}
          sx={{ position: 'absolute', left: 12, top: selection.top, zIndex: 2, boxShadow: 3 }}
        >
          Comment on selection
        </Button>
      )}
      {mine.length > 0 && (
        <Stack spacing={0.75} sx={{ mt: 1 }}>
          {mine.map(({ c, i }) => (
            <Stack
              key={i}
              direction="row"
              spacing={1}
              sx={{ alignItems: 'flex-start', px: 1.25, py: 0.75, borderRadius: '8px', bgcolor: alpha(theme.palette.warning.main, 0.08), border: 1, borderColor: alpha(theme.palette.warning.main, 0.25) }}
            >
              <Box sx={{ flex: 1, minWidth: 0 }}>
                {c.quote && (
                  <Typography variant="caption" color="text.secondary" sx={{ display: 'block', fontStyle: 'italic', mb: 0.25 }} noWrap>
                    “{c.quote}”
                  </Typography>
                )}
                <Typography variant="caption" sx={{ display: 'block', whiteSpace: 'pre-wrap' }}>
                  {c.text}
                </Typography>
              </Box>
              <IconButton size="small" onClick={() => remove(i)} sx={{ color: 'text.disabled', p: 0.25 }} aria-label="Remove comment">
                <CloseLinear size={12} color="currentColor" />
              </IconButton>
            </Stack>
          ))}
        </Stack>
      )}
    </Box>
  )
}

/** The inline box a comment is written in. */
function CommentEditor({ draft, onDone }: { draft: Draft; onDone: () => void }) {
  const add = usePlanStore((s) => s.addComment)
  const [text, setText] = useState('')
  const save = () => {
    if (!text.trim()) return
    add({ ...draft, text: text.trim() } satisfies PlanComment)
    onDone()
  }
  return (
    <Stack spacing={1} sx={{ p: 1.25, borderRadius: '10px', border: 1, borderColor: 'primary.main', bgcolor: 'background.paper' }}>
      <Typography variant="caption" color="text.secondary">
        Comment on <b>{draft.targetLabel}</b>
        {draft.quote ? ` — “${draft.quote.slice(0, 120)}${draft.quote.length > 120 ? '…' : ''}”` : ''}
      </Typography>
      <TextField
        autoFocus
        multiline
        minRows={2}
        size="small"
        placeholder="What should change?"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) save()
          if (e.key === 'Escape') onDone()
        }}
      />
      <Stack direction="row" spacing={1} sx={{ justifyContent: 'flex-end' }}>
        <Button size="small" color="inherit" onClick={onDone}>
          Cancel
        </Button>
        <Button size="small" variant="contained" onClick={save} disabled={!text.trim()}>
          Add comment
        </Button>
      </Stack>
    </Stack>
  )
}

function StepStatusIcon({ state }: { state?: StepState }) {
  if (!state || state.status === 'pending') return <Box sx={{ width: 9, height: 9, m: '2.5px', borderRadius: '50%', border: 1.5, borderColor: 'text.disabled' }} />
  if (state.status === 'running') return <CircularProgress size={13} thickness={5} />
  if (state.status === 'done')
    return (
      <Box sx={{ display: 'flex', color: 'success.main' }}>
        <TickCircleLinear size={14} color="currentColor" />
      </Box>
    )
  return (
    <Box sx={{ display: 'flex', color: state.status === 'skipped' ? 'text.disabled' : 'error.main' }}>
      <DangerLinear size={14} color="currentColor" />
    </Box>
  )
}

function StepBody({ step, index, plan, state }: { step: PlanStep; index: number; plan: Plan; state?: StepState }) {
  const [open, setOpen] = useState(false)
  const deps = step.dependsOn.map((d) => plan.steps.findIndex((s) => s.id === d) + 1).filter((n) => n > 0)
  const owner = state?.owner ?? step.owner
  return (
    <Stack direction="row" spacing={1.25} sx={{ alignItems: 'flex-start' }}>
      <Box sx={{ pt: 0.25, width: 14, display: 'flex', justifyContent: 'center', flexShrink: 0 }}>
        <StepStatusIcon state={state} />
      </Box>
      <Box sx={{ flex: 1, minWidth: 0 }}>
        <Stack direction="row" spacing={1} sx={{ alignItems: 'baseline', flexWrap: 'wrap', pr: 4 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            {index + 1}. {step.title}
          </Typography>
          {owner && <Chip size="small" label={owner} sx={{ height: 18, fontSize: '0.6875rem' }} />}
          {deps.length > 0 && (
            <Typography variant="caption" color="text.disabled">
              after {deps.join(', ')}
            </Typography>
          )}
        </Stack>
        {step.detail && (
          <Box sx={{ mt: 0.25, color: 'text.secondary', fontSize: '0.8125rem' }}>
            <Markdown text={step.detail} />
          </Box>
        )}
        {state?.output && (
          <>
            <ButtonBase onClick={() => setOpen((v) => !v)} sx={{ mt: 0.5, gap: 0.5, color: 'text.secondary', borderRadius: '6px' }}>
              <Typography variant="caption">{open ? 'Hide report' : 'Show report'}</Typography>
              <Box sx={{ display: 'flex', transform: open ? 'rotate(180deg)' : 'none' }}>
                <ArrowDownLinear size={12} color="currentColor" />
              </Box>
            </ButtonBase>
            <Collapse in={open} unmountOnExit>
              <Box sx={{ mt: 0.5, p: 1.25, borderRadius: '8px', border: 1, borderColor: 'divider', fontSize: '0.8125rem' }}>
                <Markdown text={state.output} />
              </Box>
            </Collapse>
          </>
        )}
      </Box>
    </Stack>
  )
}

/**
 * The plan under review — or being carried out.
 *
 * Reads as a document. While it awaits review, any section or step takes a
 * comment (or a comment on a selected passage); comments collect in the
 * footer and go back to the agent together as one revision request. A
 * revised version marks what it changed. Once approved, the steps become a
 * live tracker: who has each one, whether it's running, done or failed, and
 * its report.
 */
export function PlanPanel() {
  const init = usePlanStore((s) => s.init)
  const current = usePlanStore((s) => s.current)
  const previous = usePlanStore((s) => s.previous)
  const plans = usePlanStore((s) => s.plans)
  const open = usePlanStore((s) => s.open)
  const comments = usePlanStore((s) => (s.current ? (s.comments[s.current.plan.id] ?? []) : []))
  const requestChanges = usePlanStore((s) => s.requestChanges)
  const approve = usePlanStore((s) => s.approve)
  const cancelExecution = usePlanStore((s) => s.cancelExecution)
  const busy = usePlanStore((s) => s.busy)
  const error = usePlanStore((s) => s.error)
  const [draft, setDraft] = useState<Draft | null>(null)

  useEffect(() => {
    void init()
  }, [init])
  // Nothing open yet: show the newest plan.
  useEffect(() => {
    if (!current && plans[0]) void open(plans[0].id)
  }, [current, plans, open])

  const changedIds = useMemo(() => {
    const out = { changed: new Set<string>(), added: new Set<string>() }
    if (!current || !previous) return out
    const { plan } = current
    for (const s of plan.sections) {
      const before = previousText(previous, s.id)
      if (before === undefined) out.added.add(s.id)
      else if (before !== `${s.heading}\n${s.body}`) out.changed.add(s.id)
    }
    for (const st of plan.steps) {
      const before = previousText(previous, st.id)
      if (before === undefined) out.added.add(st.id)
      else if (before !== `${st.title}\n${st.detail}\n${st.owner ?? ''}\n${st.dependsOn.join(',')}`) out.changed.add(st.id)
    }
    return out
  }, [current, previous])

  if (!current) {
    return (
      <Box sx={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', p: 4 }}>
        <Typography variant="body2" color="text.secondary" sx={{ textAlign: 'center', maxWidth: 320 }}>
          No plans yet. Turn on Plan in the composer and describe what you want built — the agent researches it and
          proposes a plan here for you to review.
        </Typography>
      </Box>
    )
  }

  const { plan, state, versions } = current
  const latest = plan.version === state.latest
  const reviewing = state.status === 'review' && latest
  const executing = state.status === 'executing'
  const stepsDone = Object.values(state.steps).filter((s) => s.status === 'done').length

  const part = (id: string, label: string, children: ReactNode) => (
    <Box key={id}>
      <Part
        id={id}
        label={label}
        // What a revision changed is review information; once the plan is
        // being carried out it's only noise.
        changed={state.status === 'review' && changedIds.changed.has(id)}
        isNew={state.status === 'review' && changedIds.added.has(id) && !!previous}
        canComment={reviewing}
        onDraft={setDraft}
      >
        {children}
      </Part>
      {draft?.targetId === id && (
        <Box sx={{ mt: 0.5, mb: 1 }}>
          <CommentEditor draft={draft} onDone={() => setDraft(null)} />
        </Box>
      )}
    </Box>
  )

  return (
    <Box sx={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
      {/* Header */}
      <Stack direction="row" spacing={1} sx={{ alignItems: 'center', px: 2, py: 1.25, borderBottom: 1, borderColor: 'divider' }}>
        {plans.length > 1 ? (
          <TextField
            select
            size="small"
            value={plan.id}
            onChange={(e) => void open(e.target.value)}
            sx={{ minWidth: 0, flex: 1, '& .ui-InputBase-root': { fontSize: '0.8125rem' } }}
          >
            {plans.map((p) => (
              <MenuItem key={p.id} value={p.id}>
                {p.title}
              </MenuItem>
            ))}
          </TextField>
        ) : (
          <Typography variant="body2" sx={{ fontWeight: 600, flex: 1, minWidth: 0 }} noWrap>
            {plan.title}
          </Typography>
        )}
        <Chip size="small" label={STATUS_LABEL[state.status] ?? state.status} sx={{ height: 20, fontSize: '0.6875rem' }} />
        {versions.length > 1 && (
          <Stack direction="row" spacing={0.25}>
            {versions.map((v) => (
              <ButtonBase
                key={v}
                onClick={() => void open(plan.id, v)}
                sx={{
                  px: 0.75,
                  py: 0.25,
                  borderRadius: '6px',
                  fontSize: '0.6875rem',
                  color: v === plan.version ? 'text.primary' : 'text.secondary',
                  bgcolor: v === plan.version ? 'action.selected' : 'transparent',
                }}
              >
                v{v}
              </ButtonBase>
            ))}
          </Stack>
        )}
      </Stack>

      {/* The plan */}
      <Box className="select-text" sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 3, py: 2 }}>
        <Stack spacing={1.5} sx={{ maxWidth: 760, mx: 'auto' }}>
          {plan.changes && (
            <Box sx={{ p: 1.25, borderRadius: '10px', border: 1, borderColor: 'divider', bgcolor: 'action.hover' }}>
              <Typography variant="caption" sx={{ fontWeight: 600, display: 'block' }}>
                What changed in v{plan.version}
              </Typography>
              <Box sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>
                <Markdown text={plan.changes} />
              </Box>
            </Box>
          )}
          <Box>
            <Typography variant="h5" sx={{ mb: 0.5 }}>
              {plan.title}
            </Typography>
            <Box sx={{ color: 'text.secondary', fontSize: '0.875rem' }}>
              <Markdown text={plan.summary} />
            </Box>
          </Box>

          {plan.sections.map((s) =>
            part(
              s.id,
              s.heading,
              <>
                <Typography variant="subtitle1" sx={{ fontWeight: 600, mb: 0.5, pr: 4 }}>
                  {s.heading}
                </Typography>
                <Box sx={{ fontSize: '0.875rem' }}>
                  <Markdown text={s.body} />
                </Box>
              </>,
            ),
          )}

          <Stack direction="row" sx={{ alignItems: 'baseline', gap: 1, pt: 1 }}>
            <Typography variant="subtitle1" sx={{ fontWeight: 600 }}>
              Steps
            </Typography>
            {state.approvedVersion !== null && (
              <Typography variant="caption" color="text.secondary">
                {stepsDone} of {plan.steps.length} done
              </Typography>
            )}
          </Stack>
          {plan.steps.map((st, i) =>
            part(st.id, `Step ${i + 1}: ${st.title}`, <StepBody step={st} index={i} plan={plan} state={state.steps[st.id]} />),
          )}
        </Stack>
      </Box>

      {/* Review / execution bar */}
      <Stack direction="row" spacing={1} sx={{ alignItems: 'center', px: 2, py: 1.25, borderTop: 1, borderColor: 'divider' }}>
        {!latest ? (
          <>
            <Typography variant="caption" color="text.secondary" sx={{ flex: 1 }}>
              Viewing v{plan.version} — the latest is v{state.latest}.
            </Typography>
            <Button size="small" onClick={() => void open(plan.id)}>
              Go to latest
            </Button>
          </>
        ) : reviewing ? (
          <>
            <Typography variant="caption" color="text.secondary" sx={{ flex: 1 }}>
              {comments.length
                ? `${comments.length} comment${comments.length === 1 ? '' : 's'} — the agent will revise the plan`
                : 'Hover a section or step to comment, or select text to quote it.'}
            </Typography>
            <Button size="small" color="inherit" disabled={busy || comments.length === 0} onClick={() => void requestChanges()}>
              Request changes
            </Button>
            <Button size="small" variant="contained" disabled={busy} onClick={() => void approve()}>
              Approve plan
            </Button>
          </>
        ) : executing ? (
          <>
            <CircularProgress size={13} thickness={5} />
            <Typography variant="caption" color="text.secondary" sx={{ flex: 1 }}>
              The team is carrying out the plan — {stepsDone} of {plan.steps.length} steps done.
            </Typography>
            <Button size="small" color="inherit" onClick={() => void cancelExecution()}>
              Stop
            </Button>
          </>
        ) : (
          <Typography variant="caption" color="text.secondary" sx={{ flex: 1 }}>
            {state.status === 'approved'
              ? 'Approved — the master agent is assigning the steps to the team.'
              : state.status === 'done'
                ? 'Carried out. The master agent reviews the results in the chat.'
                : state.status === 'failed'
                  ? 'Some steps did not finish. Ask the agent to retry them in the chat.'
                  : 'Stopped.'}
          </Typography>
        )}
      </Stack>
      {error && (
        <Typography variant="caption" color="error.main" sx={{ px: 2, pb: 1 }}>
          {error}
        </Typography>
      )}
    </Box>
  )
}
