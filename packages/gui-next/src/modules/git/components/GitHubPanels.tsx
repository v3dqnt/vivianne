import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Box, Button, ButtonBase, Chip, CircularProgress, IconButton, InputBase, Link, Stack, Tooltip, Typography, alpha, useTheme } from '../../../ui'
import { ArrowRightLinear, CloseCircleLinear, ClockLinear, ExportLinear, StopCircleLinear, TickCircleLinear } from '../../../icons'
import { call } from '../../../ipc'
import { Markdown } from '../../ai/components/Markdown'
import { resolvePath } from '../../ai/components/FileLink'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'
import { usePanelStore } from '../../dock/store/panelStore'
import { useAppView } from './GitView'

/* eslint-disable @typescript-eslint/no-explicit-any -- GitHub's REST shapes, read as they come. */
type J = any

/** One GitHub REST call for the folder's repository (`{repo}` is filled in). */
export function useGh(root: string | null) {
  return useCallback(
    async <T = J,>(path: string, opts: { method?: string; body?: unknown; raw?: boolean } = {}): Promise<T> => {
      if (!root) throw new Error('Open a project folder first.')
      return (await call<T>('cmd_github', { cwd: root, path, method: opts.method ?? null, body: opts.body ?? null, raw: opts.raw ?? null })) as T
    },
    [root],
  )
}

/** Load something from GitHub, with its error and a reload. */
function useLoad<T>(load: () => Promise<T>, deps: unknown[]) {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const reload = useCallback(async () => {
    setLoading(true)
    try {
      setData(await load())
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)
  useEffect(() => {
    void reload()
  }, [reload])
  return { data, error, loading, reload, setData }
}

export function ago(iso: string | null | undefined): string {
  if (!iso) return ''
  const s = (Date.now() - new Date(iso).getTime()) / 1000
  if (s < 60) return 'just now'
  if (s < 3600) return `${Math.floor(s / 60)}m ago`
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`
  if (s < 86400 * 30) return `${Math.floor(s / 86400)}d ago`
  return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
}

function duration(from: string | null, to: string | null): string {
  if (!from) return ''
  const s = Math.max(0, ((to ? new Date(to) : new Date()).getTime() - new Date(from).getTime()) / 1000)
  if (s < 60) return `${Math.round(s)}s`
  if (s < 3600) return `${Math.floor(s / 60)}m ${Math.round(s % 60)}s`
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`
}

/** A run, job, step or check, as one mark: done well, failed, running or waiting. */
export function StatusMark({ status, conclusion, size = 14 }: { status?: string | null; conclusion?: string | null; size?: number }) {
  if (status && status !== 'completed') {
    return status === 'in_progress' ? (
      <CircularProgress size={size - 2} thickness={5} sx={{ color: 'warning.main', flexShrink: 0 }} />
    ) : (
      <Box sx={{ color: 'text.disabled', display: 'flex', flexShrink: 0 }}>
        <ClockLinear size={size} color="currentColor" />
      </Box>
    )
  }
  const good = conclusion === 'success'
  const bad = conclusion === 'failure' || conclusion === 'timed_out' || conclusion === 'startup_failure'
  const color = good ? 'success.main' : bad ? 'error.main' : 'text.disabled'
  return (
    <Box sx={{ color, display: 'flex', flexShrink: 0 }} title={conclusion ?? status ?? ''}>
      {good ? <TickCircleLinear size={size} color="currentColor" /> : bad ? <CloseCircleLinear size={size} color="currentColor" /> : <StopCircleLinear size={size} color="currentColor" />}
    </Box>
  )
}

const LABEL_SX = { fontWeight: 600, letterSpacing: 0.4, textTransform: 'uppercase', fontSize: '0.6875rem' } as const

/** A list on the left, the picked item on the right. */
function Split({ list, detail, empty }: { list: ReactNode; detail: ReactNode | null; empty: string }) {
  return (
    <Stack direction="row" sx={{ flex: 1, minHeight: 0 }}>
      <Box sx={{ width: 360, flexShrink: 0, borderRight: 1, borderColor: 'divider', overflowY: 'auto' }}>{list}</Box>
      <Box sx={{ flex: 1, minWidth: 0, overflowY: 'auto' }}>
        {detail ?? (
          <Typography variant="body2" color="text.secondary" sx={{ p: 3 }}>
            {empty}
          </Typography>
        )}
      </Box>
    </Stack>
  )
}

function ListRow({ active, onClick, icon, title, meta, right }: { active: boolean; onClick: () => void; icon: ReactNode; title: string; meta: string; right?: ReactNode }) {
  const theme = useTheme()
  return (
    <ButtonBase
      onClick={onClick}
      aria-current={active ? 'true' : undefined}
      sx={{
        display: 'flex',
        width: '100%',
        justifyContent: 'flex-start',
        alignItems: 'flex-start',
        gap: 1,
        textAlign: 'left',
        px: 1.5,
        py: 0.875,
        borderBottom: 1,
        borderColor: 'divider',
        bgcolor: active ? alpha(theme.palette.primary.main, 0.1) : 'transparent',
        '&:hover': { bgcolor: active ? alpha(theme.palette.primary.main, 0.14) : 'action.hover' },
      }}
    >
      <Box sx={{ pt: '2px', display: 'flex' }}>{icon}</Box>
      <Stack sx={{ minWidth: 0, flex: 1 }}>
        <Typography variant="body2" sx={{ fontSize: '0.8125rem', fontWeight: 500, lineHeight: 1.35 }} noWrap title={title}>
          {title}
        </Typography>
        <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }} noWrap>
          {meta}
        </Typography>
      </Stack>
      {right}
    </ButtonBase>
  )
}

function Problem({ error, onRetry }: { error: string; onRetry: () => void }) {
  return (
    <Stack sx={{ p: 2, gap: 1, alignItems: 'flex-start' }}>
      <Typography variant="body2" color="error">
        {error}
      </Typography>
      <Button size="small" onClick={onRetry}>
        Try again
      </Button>
    </Stack>
  )
}

function Filter<T extends string>({ value, options, onChange }: { value: T; options: Array<[T, string]>; onChange: (v: T) => void }) {
  return (
    <Stack direction="row" sx={{ gap: 0.5, px: 1.5, py: 1, borderBottom: 1, borderColor: 'divider' }}>
      {options.map(([v, label]) => (
        <Chip key={v} size="small" label={label} onClick={() => onChange(v)} variant={value === v ? 'filled' : 'outlined'} sx={{ height: 22, fontSize: '0.71875rem' }} />
      ))}
    </Stack>
  )
}

function DetailHead({ title, sub, url, actions }: { title: string; sub: ReactNode; url?: string; actions?: ReactNode }) {
  return (
    <Stack sx={{ px: 2, py: 1.5, gap: 0.5, borderBottom: 1, borderColor: 'divider' }}>
      <Typography variant="subtitle2" sx={{ fontWeight: 700, lineHeight: 1.35 }}>
        {title}
      </Typography>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, flexWrap: 'wrap' }}>
        <Typography variant="caption" color="text.secondary" component="div" sx={{ display: 'flex', alignItems: 'center', gap: 0.75, flexWrap: 'wrap' }}>
          {sub}
        </Typography>
        <Box sx={{ flex: 1 }} />
        {actions}
        {url && (
          <Link href={url} target="_blank" rel="noreferrer" underline="hover" sx={{ fontSize: '0.75rem' }}>
            Open on GitHub
          </Link>
        )}
      </Stack>
    </Stack>
  )
}

/** An action button that shows its own progress and error. */
function Act({ label, run, color, confirm }: { label: string; run: () => Promise<unknown>; color?: 'error' | 'primary'; confirm?: string }) {
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75 }}>
      {err && (
        <Typography variant="caption" color="error" sx={{ maxWidth: 280 }} noWrap title={err}>
          {err}
        </Typography>
      )}
      <Button
        size="small"
        variant="outlined"
        color={color}
        disabled={busy}
        onClick={async () => {
          if (confirm && !window.confirm(confirm)) return
          setBusy(true)
          setErr(null)
          try {
            await run()
          } catch (e) {
            setErr(e instanceof Error ? e.message : String(e))
          } finally {
            setBusy(false)
          }
        }}
        sx={{ borderRadius: '8px', py: 0.125, fontSize: '0.75rem' }}
      >
        {busy ? '…' : label}
      </Button>
    </Stack>
  )
}

// ── Pull requests ─────────────────────────────────────────────────────────

export function PullRequests({ root, onCheckout }: { root: string | null; onCheckout: (branch: string, pr: number) => Promise<void> }) {
  const gh = useGh(root)
  const [state, setState] = useState<'open' | 'closed'>('open')
  const [picked, setPicked] = useState<number | null>(null)
  const list = useLoad(() => gh<J[]>(`/repos/{repo}/pulls?state=${state}&per_page=50&sort=updated&direction=desc`), [gh, state])

  return (
    <Split
      empty={list.data?.length === 0 ? `No ${state} pull requests.` : 'Pick a pull request.'}
      list={
        <>
          <Filter value={state} options={[['open', 'Open'], ['closed', 'Closed']]} onChange={(v) => { setState(v); setPicked(null) }} />
          {list.error && <Problem error={list.error} onRetry={list.reload} />}
          {list.loading && !list.data && <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 1.5 }}>Loading…</Typography>}
          {list.data?.map((p) => (
            <ListRow
              key={p.number}
              active={picked === p.number}
              onClick={() => setPicked(p.number)}
              icon={<PrMark pr={p} />}
              title={p.title}
              meta={`#${p.number} · ${p.user?.login} · ${p.head?.ref} → ${p.base?.ref} · ${ago(p.updated_at)}`}
              right={p.draft ? <Chip size="small" label="Draft" sx={{ height: 18, fontSize: '0.625rem' }} /> : undefined}
            />
          ))}
        </>
      }
      detail={picked !== null ? <PullRequest key={picked} gh={gh} number={picked} onCheckout={onCheckout} onChanged={list.reload} /> : null}
    />
  )
}

function PrMark({ pr }: { pr: J }) {
  const color = pr.merged_at ? 'secondary.main' : pr.state === 'closed' ? 'error.main' : pr.draft ? 'text.disabled' : 'success.main'
  return <Box sx={{ width: 10, height: 10, mt: '3px', borderRadius: '50%', bgcolor: color, flexShrink: 0 }} />
}

function PullRequest({ gh, number, onCheckout, onChanged }: { gh: ReturnType<typeof useGh>; number: number; onCheckout: (b: string, pr: number) => Promise<void>; onChanged: () => void }) {
  const pr = useLoad(() => gh(`/repos/{repo}/pulls/${number}`), [gh, number])
  const files = useLoad(() => gh<J[]>(`/repos/{repo}/pulls/${number}/files?per_page=100`), [gh, number])
  const sha = pr.data?.head?.sha as string | undefined
  const checks = useLoad(async () => (sha ? ((await gh(`/repos/{repo}/commits/${sha}/check-runs?per_page=100`)).check_runs as J[]) : []), [gh, sha])
  const reviews = useLoad(() => gh<J[]>(`/repos/{repo}/pulls/${number}/reviews?per_page=50`), [gh, number])
  const [comment, setComment] = useState('')
  const p = pr.data
  if (pr.error) return <Problem error={pr.error} onRetry={pr.reload} />
  if (!p) return <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 2 }}>Loading…</Typography>
  const open = p.state === 'open' && !p.merged_at
  const after = async () => {
    await pr.reload()
    onChanged()
  }
  return (
    <Stack>
      <DetailHead
        title={`${p.title} #${p.number}`}
        url={p.html_url}
        sub={
          <>
            <PrMark pr={p} />
            {p.merged_at ? 'Merged' : p.state === 'closed' ? 'Closed' : p.draft ? 'Draft' : 'Open'} · {p.user?.login} wants to merge {p.commits} commit{p.commits === 1 ? '' : 's'} from{' '}
            <code>{p.head?.ref}</code> into <code>{p.base?.ref}</code> · <Box component="span" sx={{ color: 'success.main' }}>+{p.additions}</Box>{' '}
            <Box component="span" sx={{ color: 'error.main' }}>−{p.deletions}</Box>
          </>
        }
        actions={
          <>
            <Act label="Check out" run={() => onCheckout(p.head?.ref, p.number)} />
            {open && (
              <Act
                label="Squash and merge"
                color="primary"
                confirm={`Squash and merge #${p.number} into ${p.base?.ref}?`}
                run={async () => {
                  await gh(`/repos/{repo}/pulls/${number}/merge`, { method: 'PUT', body: { merge_method: 'squash' } })
                  await after()
                }}
              />
            )}
            {open ? (
              <Act label="Close" color="error" run={async () => { await gh(`/repos/{repo}/pulls/${number}`, { method: 'PATCH', body: { state: 'closed' } }); await after() }} />
            ) : (
              !p.merged_at && <Act label="Reopen" run={async () => { await gh(`/repos/{repo}/pulls/${number}`, { method: 'PATCH', body: { state: 'open' } }); await after() }} />
            )}
          </>
        }
      />
      <Stack sx={{ p: 2, gap: 2 }}>
        <Section title={`Checks${checks.data ? ` · ${checks.data.length}` : ''}`}>
          {checks.data?.length === 0 && <Typography variant="caption" color="text.secondary">No checks on this commit.</Typography>}
          {checks.data?.map((c) => (
            <Stack key={c.id} direction="row" sx={{ alignItems: 'center', gap: 1, py: 0.375 }}>
              <StatusMark status={c.status} conclusion={c.conclusion} />
              <Typography variant="body2" sx={{ fontSize: '0.8125rem', flex: 1, minWidth: 0 }} noWrap>
                {c.name}
              </Typography>
              <Typography variant="caption" color="text.secondary">
                {duration(c.started_at, c.completed_at)}
              </Typography>
              {c.html_url && (
                <Link href={c.html_url} target="_blank" rel="noreferrer" underline="hover" sx={{ fontSize: '0.71875rem' }}>
                  Details
                </Link>
              )}
            </Stack>
          ))}
        </Section>
        {reviews.data && reviews.data.length > 0 && (
          <Section title="Reviews">
            {reviews.data.map((r) => (
              <Typography key={r.id} variant="body2" sx={{ fontSize: '0.8125rem' }}>
                <b>{r.user?.login}</b> {r.state === 'APPROVED' ? 'approved' : r.state === 'CHANGES_REQUESTED' ? 'requested changes' : 'commented'} · {ago(r.submitted_at)}
              </Typography>
            ))}
          </Section>
        )}
        {p.body && (
          <Section title="Description">
            <Box sx={{ fontSize: '0.875rem' }}>
              <Markdown text={p.body} />
            </Box>
          </Section>
        )}
        <Section title={`Files changed${files.data ? ` · ${files.data.length}` : ''}`}>
          {files.data?.map((f) => (
            <PrFile key={f.filename} file={f} />
          ))}
        </Section>
        <Section title="Comment">
          <InputBase multiline minRows={2} value={comment} onChange={(e) => setComment(e.target.value)} placeholder="Leave a comment" sx={{ fontSize: '0.8125rem', border: 1, borderColor: 'divider', borderRadius: '8px', px: 1, py: 0.75 }} />
          <Box sx={{ pt: 0.75 }}>
            <Act label="Comment" run={async () => { if (!comment.trim()) return; await gh(`/repos/{repo}/issues/${number}/comments`, { method: 'POST', body: { body: comment.trim() } }); setComment('') }} />
          </Box>
        </Section>
      </Stack>
    </Stack>
  )
}

/** A changed file of a pull request: click it for its diff; the arrow opens
 * it in Preview (as it is in this checkout). */
function PrFile({ file }: { file: J }) {
  const [open, setOpen] = useState(false)
  const root = useWorkspaceStore((s) => s.folders[0] ?? null)
  const show = () => {
    usePanelStore.getState().openDocument(resolvePath(file.filename, root))
    useAppView.setState({ view: 'chat' })
  }
  const lines: string[] = (file.patch ?? '').split('\n')
  return (
    <Box sx={{ borderBottom: 1, borderColor: 'divider' }}>
      <Stack direction="row" sx={{ gap: 1, alignItems: 'center', '&:hover .pr-open': { opacity: 1 } }}>
        <ButtonBase onClick={() => setOpen((v) => !v)} aria-expanded={open} sx={{ flex: 1, minWidth: 0, justifyContent: 'flex-start', gap: 1, py: 0.5, borderRadius: '6px', '&:hover': { color: 'primary.main' } }}>
          <Box sx={{ display: 'flex', transform: open ? 'rotate(90deg)' : 'none', transition: 'transform 150ms', color: 'text.secondary' }}>
            <ArrowRightLinear size={11} color="currentColor" />
          </Box>
          <Typography variant="body2" sx={{ fontSize: '0.78125rem', fontFamily: '"Geist Mono", ui-monospace, monospace', flex: 1, minWidth: 0, textAlign: 'left' }} noWrap title={file.filename}>
            {file.filename}
          </Typography>
          {file.status !== 'modified' && (
            <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.65625rem' }}>
              {file.status}
            </Typography>
          )}
          <Typography variant="caption" sx={{ color: 'success.main' }}>+{file.additions}</Typography>
          <Typography variant="caption" sx={{ color: 'error.main' }}>−{file.deletions}</Typography>
        </ButtonBase>
        <Tooltip title="Open in Preview">
          <IconButton size="small" className="pr-open" aria-label={`Open ${file.filename}`} onClick={show} sx={{ opacity: 0, transition: 'opacity 0.12s' }}>
            <ExportLinear size={12} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Stack>
      {open && (
        <Box
          component="pre"
          className="select-text"
          sx={{ m: 0, mb: 1, py: 0.75, overflowX: 'auto', borderRadius: '8px', border: 1, borderColor: 'divider', fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.71875rem', lineHeight: 1.5 }}
        >
          {file.patch
            ? lines.map((l, i) => (
                <Box
                  key={i}
                  component="div"
                  sx={{
                    px: 1,
                    whiteSpace: 'pre',
                    color: l.startsWith('@@') ? 'text.secondary' : undefined,
                    bgcolor: l.startsWith('+') ? 'rgba(46,160,67,0.15)' : l.startsWith('-') ? 'rgba(248,81,73,0.15)' : undefined,
                  }}
                >
                  {l || ' '}
                </Box>
              ))
            : <Box sx={{ px: 1, color: 'text.secondary' }}>No text diff (binary, or too large for GitHub to show).</Box>}
        </Box>
      )}
    </Box>
  )
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Stack sx={{ gap: 0.5 }}>
      <Typography variant="caption" color="text.secondary" sx={LABEL_SX}>
        {title}
      </Typography>
      {children}
    </Stack>
  )
}

// ── Actions ───────────────────────────────────────────────────────────────

export function Actions({ root }: { root: string | null }) {
  const gh = useGh(root)
  const [workflow, setWorkflow] = useState<number | 'all'>('all')
  const [picked, setPicked] = useState<number | null>(null)
  const flows = useLoad(async () => ((await gh('/repos/{repo}/actions/workflows?per_page=100')).workflows as J[]) ?? [], [gh])
  const runs = useLoad(
    async () => ((await gh(workflow === 'all' ? '/repos/{repo}/actions/runs?per_page=40' : `/repos/{repo}/actions/workflows/${workflow}/runs?per_page=40`)).workflow_runs as J[]) ?? [],
    [gh, workflow],
  )
  // Running ones are watched: the list refreshes while anything is going.
  const live = runs.data?.some((r) => r.status !== 'completed')
  useEffect(() => {
    if (!live) return
    const t = window.setInterval(() => void runs.reload(), 10_000)
    return () => window.clearInterval(t)
  }, [live, runs.reload])

  return (
    <Split
      empty={runs.data?.length === 0 ? 'No workflow runs yet.' : 'Pick a run to see its jobs and logs.'}
      list={
        <>
          <Stack direction="row" sx={{ gap: 0.5, px: 1.5, py: 1, borderBottom: 1, borderColor: 'divider', flexWrap: 'wrap' }}>
            <Chip size="small" label="All workflows" onClick={() => setWorkflow('all')} variant={workflow === 'all' ? 'filled' : 'outlined'} sx={{ height: 22, fontSize: '0.71875rem' }} />
            {flows.data?.map((w) => (
              <Chip key={w.id} size="small" label={w.name} onClick={() => setWorkflow(w.id)} variant={workflow === w.id ? 'filled' : 'outlined'} sx={{ height: 22, fontSize: '0.71875rem' }} />
            ))}
          </Stack>
          {runs.error && <Problem error={runs.error} onRetry={runs.reload} />}
          {runs.loading && !runs.data && <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 1.5 }}>Loading…</Typography>}
          {runs.data?.map((r) => (
            <ListRow
              key={r.id}
              active={picked === r.id}
              onClick={() => setPicked(r.id)}
              icon={<StatusMark status={r.status} conclusion={r.conclusion} />}
              title={r.display_title || r.name}
              meta={`${r.name} #${r.run_number} · ${r.head_branch} · ${r.event} · ${ago(r.created_at)}`}
            />
          ))}
        </>
      }
      detail={picked !== null ? <Run key={picked} gh={gh} id={picked} onChanged={runs.reload} /> : null}
    />
  )
}

function Run({ gh, id, onChanged }: { gh: ReturnType<typeof useGh>; id: number; onChanged: () => void }) {
  const run = useLoad(() => gh(`/repos/{repo}/actions/runs/${id}`), [gh, id])
  const jobs = useLoad(async () => ((await gh(`/repos/{repo}/actions/runs/${id}/jobs?per_page=100`)).jobs as J[]) ?? [], [gh, id])
  const [logOf, setLogOf] = useState<number | null>(null)
  const r = run.data
  const live = r && r.status !== 'completed'
  useEffect(() => {
    if (!live) return
    const t = window.setInterval(() => {
      void run.reload()
      void jobs.reload()
    }, 5000)
    return () => window.clearInterval(t)
  }, [live, run.reload, jobs.reload])
  // The first failed job opens on its own: that is what one looks for.
  useEffect(() => {
    if (logOf === null && jobs.data) {
      const failed = jobs.data.find((j) => j.conclusion === 'failure')
      if (failed) setLogOf(failed.id)
    }
  }, [jobs.data, logOf])
  if (run.error) return <Problem error={run.error} onRetry={run.reload} />
  if (!r) return <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 2 }}>Loading…</Typography>
  const after = async () => {
    await Promise.all([run.reload(), jobs.reload()])
    onChanged()
  }
  return (
    <Stack>
      <DetailHead
        title={`${r.display_title || r.name}`}
        url={r.html_url}
        sub={
          <>
            <StatusMark status={r.status} conclusion={r.conclusion} />
            {r.name} #{r.run_number} · {r.event} on <code>{r.head_branch}</code> · {r.head_sha?.slice(0, 7)} · {ago(r.created_at)} · {duration(r.run_started_at, r.status === 'completed' ? r.updated_at : null)}
          </>
        }
        actions={
          <>
            {live ? (
              <Act label="Cancel" color="error" run={async () => { await gh(`/repos/{repo}/actions/runs/${id}/cancel`, { method: 'POST' }); await after() }} />
            ) : (
              <>
                {r.conclusion === 'failure' && <Act label="Re-run failed jobs" run={async () => { await gh(`/repos/{repo}/actions/runs/${id}/rerun-failed-jobs`, { method: 'POST' }); await after() }} />}
                <Act label="Re-run all" run={async () => { await gh(`/repos/{repo}/actions/runs/${id}/rerun`, { method: 'POST' }); await after() }} />
              </>
            )}
          </>
        }
      />
      <Stack sx={{ p: 2, gap: 1 }}>
        {jobs.error && <Problem error={jobs.error} onRetry={jobs.reload} />}
        {jobs.data?.map((j) => (
          <Job key={j.id} gh={gh} job={j} open={logOf === j.id} onToggle={() => setLogOf(logOf === j.id ? null : j.id)} />
        ))}
      </Stack>
    </Stack>
  )
}

function Job({ gh, job, open, onToggle }: { gh: ReturnType<typeof useGh>; job: J; open: boolean; onToggle: () => void }) {
  const theme = useTheme()
  return (
    <Box sx={{ border: 1, borderColor: 'divider', borderRadius: '10px', overflow: 'hidden' }}>
      <ButtonBase onClick={onToggle} sx={{ display: 'flex', width: '100%', justifyContent: 'flex-start', gap: 1, px: 1.25, py: 0.875, '&:hover': { bgcolor: 'action.hover' } }}>
        <StatusMark status={job.status} conclusion={job.conclusion} />
        <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem', flex: 1, textAlign: 'left' }} noWrap>
          {job.name}
        </Typography>
        <Typography variant="caption" color="text.secondary">
          {duration(job.started_at, job.completed_at)}
        </Typography>
      </ButtonBase>
      {open && (
        <Box sx={{ borderTop: 1, borderColor: 'divider', bgcolor: alpha(theme.palette.text.primary, 0.025) }}>
          <Stack sx={{ px: 1.25, py: 0.75, gap: 0.25 }}>
            {(job.steps ?? []).map((s: J) => (
              <Stack key={s.number} direction="row" sx={{ alignItems: 'center', gap: 1 }}>
                <StatusMark status={s.status} conclusion={s.conclusion} size={12} />
                <Typography variant="caption" sx={{ fontSize: '0.75rem', flex: 1, minWidth: 0 }} noWrap>
                  {s.name}
                </Typography>
                <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem' }}>
                  {duration(s.started_at, s.completed_at)}
                </Typography>
              </Stack>
            ))}
          </Stack>
          {job.status === 'completed' && <JobLog gh={gh} id={job.id} />}
        </Box>
      )}
    </Box>
  )
}

/** A job's log, from its end: where it failed is usually the last screen. */
function JobLog({ gh, id }: { gh: ReturnType<typeof useGh>; id: number }) {
  const log = useLoad(() => gh<string>(`/repos/{repo}/actions/jobs/${id}/logs`, { raw: true }), [gh, id])
  const box = useRef<HTMLPreElement>(null)
  const lines = useMemo(() => {
    const all = (log.data ?? '').split('\n').map((l) => l.replace(/^\d{4}-\d\d-\d\dT[\d:.]+Z /, ''))
    return all.slice(-400).join('\n')
  }, [log.data])
  useEffect(() => {
    if (box.current) box.current.scrollTop = box.current.scrollHeight
  }, [lines])
  if (log.error) return <Typography variant="caption" color="error" sx={{ display: 'block', px: 1.25, pb: 1 }}>{log.error}</Typography>
  return (
    <Box
      ref={box}
      component="pre"
      className="select-text"
      sx={{ m: 0, px: 1.25, py: 1, maxHeight: 420, overflow: 'auto', borderTop: 1, borderColor: 'divider', fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.71875rem', lineHeight: 1.5, whiteSpace: 'pre-wrap', wordBreak: 'break-word', color: 'text.secondary' }}
    >
      {log.loading && !log.data ? 'Loading the log…' : lines || 'No log.'}
    </Box>
  )
}

// ── Issues ────────────────────────────────────────────────────────────────

export function Issues({ root }: { root: string | null }) {
  const gh = useGh(root)
  const [state, setState] = useState<'open' | 'closed'>('open')
  const [picked, setPicked] = useState<number | null>(null)
  const list = useLoad(async () => (await gh<J[]>(`/repos/{repo}/issues?state=${state}&per_page=50&sort=updated`)).filter((i) => !i.pull_request), [gh, state])
  const [title, setTitle] = useState('')

  return (
    <Split
      empty={list.data?.length === 0 ? `No ${state} issues.` : 'Pick an issue.'}
      list={
        <>
          <Filter value={state} options={[['open', 'Open'], ['closed', 'Closed']]} onChange={(v) => { setState(v); setPicked(null) }} />
          <Stack direction="row" sx={{ gap: 0.75, px: 1.5, py: 1, borderBottom: 1, borderColor: 'divider' }}>
            <InputBase value={title} onChange={(e) => setTitle(e.target.value)} placeholder="New issue title" sx={{ flex: 1, fontSize: '0.8125rem', border: 1, borderColor: 'divider', borderRadius: '8px', px: 1 }} />
            <Act
              label="Create"
              run={async () => {
                if (!title.trim()) return
                const made = await gh('/repos/{repo}/issues', { method: 'POST', body: { title: title.trim() } })
                setTitle('')
                await list.reload()
                setPicked(made.number)
              }}
            />
          </Stack>
          {list.error && <Problem error={list.error} onRetry={list.reload} />}
          {list.data?.map((i) => (
            <ListRow
              key={i.number}
              active={picked === i.number}
              onClick={() => setPicked(i.number)}
              icon={<Box sx={{ width: 10, height: 10, mt: '3px', borderRadius: '50%', bgcolor: i.state === 'open' ? 'success.main' : 'secondary.main' }} />}
              title={i.title}
              meta={`#${i.number} · ${i.user?.login} · ${i.comments} comment${i.comments === 1 ? '' : 's'} · ${ago(i.updated_at)}`}
              right={
                i.labels?.length ? (
                  <Stack direction="row" sx={{ gap: 0.25 }}>
                    {i.labels.slice(0, 2).map((l: J) => (
                      <Box key={l.id} sx={{ px: 0.5, borderRadius: '4px', fontSize: '0.625rem', bgcolor: `#${l.color}33`, color: 'text.secondary' }}>
                        {l.name}
                      </Box>
                    ))}
                  </Stack>
                ) : undefined
              }
            />
          ))}
        </>
      }
      detail={picked !== null ? <Issue key={picked} gh={gh} number={picked} onChanged={list.reload} /> : null}
    />
  )
}

function Issue({ gh, number, onChanged }: { gh: ReturnType<typeof useGh>; number: number; onChanged: () => void }) {
  const issue = useLoad(() => gh(`/repos/{repo}/issues/${number}`), [gh, number])
  const comments = useLoad(() => gh<J[]>(`/repos/{repo}/issues/${number}/comments?per_page=100`), [gh, number])
  const [text, setText] = useState('')
  const i = issue.data
  if (issue.error) return <Problem error={issue.error} onRetry={issue.reload} />
  if (!i) return <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 2 }}>Loading…</Typography>
  const setState = async (state: 'open' | 'closed') => {
    await gh(`/repos/{repo}/issues/${number}`, { method: 'PATCH', body: { state } })
    await issue.reload()
    onChanged()
  }
  return (
    <Stack>
      <DetailHead
        title={`${i.title} #${i.number}`}
        url={i.html_url}
        sub={<>{i.state === 'open' ? 'Open' : 'Closed'} · opened by {i.user?.login} {ago(i.created_at)}</>}
        actions={i.state === 'open' ? <Act label="Close" color="error" run={() => setState('closed')} /> : <Act label="Reopen" run={() => setState('open')} />}
      />
      <Stack sx={{ p: 2, gap: 2 }}>
        {i.body && (
          <Box sx={{ fontSize: '0.875rem' }}>
            <Markdown text={i.body} />
          </Box>
        )}
        {comments.data?.map((c) => (
          <Stack key={c.id} sx={{ gap: 0.5, borderTop: 1, borderColor: 'divider', pt: 1.5 }}>
            <Typography variant="caption" color="text.secondary">
              <b>{c.user?.login}</b> · {ago(c.created_at)}
            </Typography>
            <Box sx={{ fontSize: '0.875rem' }}>
              <Markdown text={c.body ?? ''} />
            </Box>
          </Stack>
        ))}
        <Stack sx={{ gap: 0.75 }}>
          <InputBase multiline minRows={2} value={text} onChange={(e) => setText(e.target.value)} placeholder="Leave a comment" sx={{ fontSize: '0.8125rem', border: 1, borderColor: 'divider', borderRadius: '8px', px: 1, py: 0.75 }} />
          <Box>
            <Act label="Comment" run={async () => { if (!text.trim()) return; await gh(`/repos/{repo}/issues/${number}/comments`, { method: 'POST', body: { body: text.trim() } }); setText(''); await comments.reload() }} />
          </Box>
        </Stack>
      </Stack>
    </Stack>
  )
}

// ── Releases ──────────────────────────────────────────────────────────────

export function Releases({ root }: { root: string | null }) {
  const gh = useGh(root)
  const list = useLoad(() => gh<J[]>('/repos/{repo}/releases?per_page=30'), [gh])
  return (
    <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto' }}>
      {list.error && <Problem error={list.error} onRetry={list.reload} />}
      {list.data?.length === 0 && <Typography variant="body2" color="text.secondary" sx={{ p: 3 }}>No releases yet.</Typography>}
      <Stack sx={{ maxWidth: 860, px: 2, py: 1.5, gap: 2 }}>
        {list.data?.map((r) => (
          <Stack key={r.id} sx={{ gap: 0.75, pb: 2, borderBottom: 1, borderColor: 'divider' }}>
            <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
              <Typography variant="subtitle2" sx={{ fontWeight: 700 }}>
                {r.name || r.tag_name}
              </Typography>
              <Chip size="small" label={r.tag_name} variant="outlined" sx={{ height: 18, fontSize: '0.625rem' }} />
              {r.prerelease && <Chip size="small" label="Pre-release" sx={{ height: 18, fontSize: '0.625rem' }} />}
              {r.draft && <Chip size="small" label="Draft" sx={{ height: 18, fontSize: '0.625rem' }} />}
              <Box sx={{ flex: 1 }} />
              <Typography variant="caption" color="text.secondary">
                {ago(r.published_at ?? r.created_at)}
              </Typography>
              <Link href={r.html_url} target="_blank" rel="noreferrer" underline="hover" sx={{ fontSize: '0.75rem' }}>
                Open
              </Link>
            </Stack>
            {r.body && (
              <Box sx={{ fontSize: '0.8125rem' }}>
                <Markdown text={r.body} />
              </Box>
            )}
            {r.assets?.length > 0 && (
              <Stack direction="row" sx={{ gap: 0.75, flexWrap: 'wrap' }}>
                {r.assets.map((a: J) => (
                  <Link key={a.id} href={a.browser_download_url} target="_blank" rel="noreferrer" underline="hover" sx={{ fontSize: '0.71875rem', fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
                    {a.name} ({Math.round(a.size / 1024 / 1024)} MB · {a.download_count}↓)
                  </Link>
                ))}
              </Stack>
            )}
          </Stack>
        ))}
      </Stack>
    </Box>
  )
}

