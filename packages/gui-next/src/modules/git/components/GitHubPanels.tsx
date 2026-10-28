import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Box, Button, ButtonBase, CircularProgress, IconButton, InputBase, Link, Stack, Tooltip, Typography, alpha, useTheme } from '../../../ui'
import { ArrowRightLinear, CloseCircleLinear, ClockLinear, ExportLinear, StopCircleLinear, TickCircleLinear } from '../../../icons'
import { call } from '../../../ipc'
import { Markdown } from '../../ai/components/Markdown'
import { resolvePath } from '../../ai/components/FileLink'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'
import { usePanelStore } from '../../dock/store/panelStore'
import { useAppView } from './GitView'
import { BTN_SX, MONO_FONT, Pill, T, u } from './gitStyle'

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


/** A list on the left, the picked item on the right. */
function Split({ list, detail, empty }: { list: ReactNode; detail: ReactNode | null; empty: string }) {
  return (
    <Stack direction="row" sx={{ flex: 1, minHeight: 0 }}>
      <Box sx={{ width: 320, flexShrink: 0, borderRight: 1, borderColor: 'divider', overflowY: 'auto' }}>{list}</Box>
      <Box sx={{ flex: 1, minWidth: 0, overflowY: 'auto' }}>
        {detail ?? (
          <Typography sx={{ ...T.small, color: 'text.secondary', p: u(4) }}>
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
        gap: u(2),
        textAlign: 'left',
        px: u(3),
        py: u(1),
        borderBottom: 1,
        borderColor: 'divider',
        bgcolor: active ? alpha(theme.palette.primary.main, 0.1) : 'transparent',
        '&:hover': { bgcolor: active ? alpha(theme.palette.primary.main, 0.14) : 'action.hover' },
      }}
    >
      <Box sx={{ height: 20, display: 'flex', alignItems: 'center', flexShrink: 0 }}>{icon}</Box>
      <Stack sx={{ minWidth: 0, flex: 1 }}>
        <Typography sx={{ ...T.body, fontWeight: 500 }} noWrap title={title}>
          {title}
        </Typography>
        <Typography sx={{ ...T.meta, color: 'text.secondary' }} noWrap>
          {meta}
        </Typography>
      </Stack>
      {right && <Box sx={{ height: 20, display: 'flex', alignItems: 'center', flexShrink: 0 }}>{right}</Box>}
    </ButtonBase>
  )
}

function Problem({ error, onRetry }: { error: string; onRetry: () => void }) {
  return (
    <Stack sx={{ p: u(3), gap: u(2), alignItems: 'flex-start' }}>
      <Typography sx={{ ...T.small, color: 'error.main' }}>
        {error}
      </Typography>
      <Button size="small" onClick={onRetry} sx={BTN_SX}>
        Try again
      </Button>
    </Stack>
  )
}

function Filter<T extends string>({ value, options, onChange }: { value: T; options: Array<[T, string]>; onChange: (v: T) => void }) {
  return (
    <Stack direction="row" sx={{ gap: u(1), px: u(3), py: u(2), borderBottom: 1, borderColor: 'divider' }}>
      {options.map(([v, label]) => (
        <Pill key={v} active={value === v} onClick={() => onChange(v)}>
          {label}
        </Pill>
      ))}
    </Stack>
  )
}

function DetailHead({ title, sub, url, actions }: { title: string; sub: ReactNode; url?: string; actions?: ReactNode }) {
  return (
    <Stack sx={{ px: u(4), py: u(3), gap: u(2), borderBottom: 1, borderColor: 'divider' }}>
      <Typography sx={{ ...T.title }}>
        {title}
      </Typography>
      <Stack direction="row" sx={{ alignItems: 'center', gap: u(2), flexWrap: 'wrap' }}>
        <Typography component="div" sx={{ ...T.small, color: 'text.secondary', display: 'flex', alignItems: 'center', gap: u(1), flexWrap: 'wrap', '& code': { ...T.mono, fontSize: '11px' } }}>
          {sub}
        </Typography>
        <Box sx={{ flex: 1 }} />
        {actions}
        {url && (
          <Link href={url} target="_blank" rel="noreferrer" underline="hover" sx={{ ...T.small, whiteSpace: 'nowrap' }}>
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
    <Stack direction="row" sx={{ alignItems: 'center', gap: u(2) }}>
      {err && (
        <Typography sx={{ ...T.meta, color: 'error.main', maxWidth: 280 }} noWrap title={err}>
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
        sx={BTN_SX}
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
          {list.loading && !list.data && <Typography sx={{ ...T.small, color: 'text.secondary', display: 'block', p: u(3) }}>Loading…</Typography>}
          {list.data?.map((p) => (
            <ListRow
              key={p.number}
              active={picked === p.number}
              onClick={() => setPicked(p.number)}
              icon={<PrMark pr={p} />}
              title={p.title}
              meta={`#${p.number} · ${p.user?.login} · ${p.head?.ref} → ${p.base?.ref} · ${ago(p.updated_at)}`}
              right={p.draft ? <Box component="span" sx={{ ...T.meta, fontWeight: 600, px: u(1), borderRadius: '4px', bgcolor: 'action.selected', color: 'text.secondary' }}>Draft</Box> : undefined}
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
  return <Box sx={{ width: 8, height: 8, borderRadius: '50%', bgcolor: color, flexShrink: 0 }} />
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
  if (!p) return <Typography sx={{ ...T.small, color: 'text.secondary', display: 'block', p: u(4) }}>Loading…</Typography>
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
      <Stack sx={{ px: u(4), py: u(3), gap: u(4) }}>
        <Section title={`Checks${checks.data ? ` · ${checks.data.length}` : ''}`}>
          {checks.data?.length === 0 && <Typography sx={{ ...T.small, color: 'text.secondary' }}>No checks on this commit.</Typography>}
          {checks.data?.map((c) => (
            <Stack key={c.id} direction="row" sx={{ alignItems: 'center', gap: u(2), height: 24 }}>
              <StatusMark status={c.status} conclusion={c.conclusion} />
              <Typography sx={{ ...T.small, flex: 1, minWidth: 0 }} noWrap>
                {c.name}
              </Typography>
              <Typography sx={{ ...T.meta, color: 'text.secondary' }}>
                {duration(c.started_at, c.completed_at)}
              </Typography>
              {c.html_url && (
                <Link href={c.html_url} target="_blank" rel="noreferrer" underline="hover" sx={{ ...T.meta }}>
                  Details
                </Link>
              )}
            </Stack>
          ))}
        </Section>
        {reviews.data && reviews.data.length > 0 && (
          <Section title="Reviews">
            {reviews.data.map((r) => (
              <Typography key={r.id} sx={{ ...T.small }}>
                <b>{r.user?.login}</b> {r.state === 'APPROVED' ? 'approved' : r.state === 'CHANGES_REQUESTED' ? 'requested changes' : 'commented'} · {ago(r.submitted_at)}
              </Typography>
            ))}
          </Section>
        )}
        {p.body && (
          <Section title="Description">
            <Box sx={{ ...T.body }}>
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
          <InputBase multiline minRows={2} value={comment} onChange={(e) => setComment(e.target.value)} placeholder="Leave a comment" sx={{ ...T.body, border: 1, borderColor: 'divider', borderRadius: '6px', px: u(2), py: u(1) }} />
          <Box sx={{ pt: u(2) }}>
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
      <Stack direction="row" sx={{ gap: u(1), alignItems: 'center', height: 28, '&:hover .pr-open': { opacity: 1 } }}>
        <ButtonBase onClick={() => setOpen((v) => !v)} aria-expanded={open} sx={{ flex: 1, minWidth: 0, height: '100%', justifyContent: 'flex-start', gap: u(2), borderRadius: '4px', '&:hover': { color: 'primary.main' } }}>
          <Box sx={{ display: 'flex', transform: open ? 'rotate(90deg)' : 'none', transition: 'transform 150ms', color: 'text.secondary' }}>
            <ArrowRightLinear size={11} color="currentColor" />
          </Box>
          <Typography sx={{ ...T.small, ...T.mono, flex: 1, minWidth: 0, textAlign: 'left' }} noWrap title={file.filename}>
            {file.filename}
          </Typography>
          {file.status !== 'modified' && (
            <Typography sx={{ ...T.meta, color: 'text.secondary' }}>
              {file.status}
            </Typography>
          )}
          <Typography sx={{ ...T.meta, ...T.mono, color: 'success.main' }}>+{file.additions}</Typography>
          <Typography sx={{ ...T.meta, ...T.mono, color: 'error.main' }}>−{file.deletions}</Typography>
        </ButtonBase>
        <Tooltip title="Open in Browser">
          <IconButton size="small" className="pr-open" aria-label={`Open ${file.filename}`} onClick={show} sx={{ width: 24, height: 24, opacity: 0, transition: 'opacity 0.12s' }}>
            <ExportLinear size={12} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Stack>
      {open && (
        <Box
          component="pre"
          className="select-text"
          sx={{ m: 0, mb: u(2), py: u(1), overflowX: 'auto', borderRadius: '6px', border: 1, borderColor: 'divider', fontFamily: MONO_FONT, fontSize: '11px', lineHeight: '16px' }}
        >
          {file.patch
            ? lines.map((l, i) => (
                <Box
                  key={i}
                  component="div"
                  sx={{
                    px: u(2),
                    whiteSpace: 'pre',
                    color: l.startsWith('@@') ? 'text.secondary' : undefined,
                    bgcolor: l.startsWith('+') ? 'rgba(46,160,67,0.15)' : l.startsWith('-') ? 'rgba(248,81,73,0.15)' : undefined,
                  }}
                >
                  {l || ' '}
                </Box>
              ))
            : <Box sx={{ px: u(2), color: 'text.secondary' }}>No text diff (binary, or too large for GitHub to show).</Box>}
        </Box>
      )}
    </Box>
  )
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Stack sx={{ gap: u(1) }}>
      <Typography sx={{ ...T.label, color: 'text.secondary', pb: u(1) }}>
        {title}
      </Typography>
      {children}
    </Stack>
  )
}

// ── Actions ───────────────────────────────────────────────────────────────

/** Repositories whose Actions belong with this project's: its own, a
 * `<repo>-releases` beside it when there is one (where releases often build),
 * and any the user adds. Kept per folder. */
function useActionRepos(root: string | null, gh: ReturnType<typeof useGh>) {
  const key = `viv.git.actionRepos.${root ?? ''}`
  const [own, setOwn] = useState<string | null>(null)
  const [extra, setExtra] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem(key) ?? '[]') as string[]
    } catch {
      return []
    }
  })
  const save = (list: string[]) => {
    setExtra(list)
    try {
      localStorage.setItem(key, JSON.stringify(list))
    } catch {
      // storage unavailable: kept for this session only
    }
  }
  useEffect(() => {
    if (!root) return
    let live = true
    void call<string>('cmd_github_repo', { cwd: root })
      .then(async (repo) => {
        if (!live || !repo) return
        setOwn(repo)
        const releases = `${repo}-releases`
        if (extra.includes(releases)) return
        try {
          await gh(`/repos/${releases}`)
          if (live) save([...extra, releases])
        } catch {
          // no such repository
        }
      })
      .catch(() => {})
    return () => {
      live = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [root])
  return { own, extra, add: (r: string) => !extra.includes(r) && save([...extra, r]), remove: (r: string) => save(extra.filter((x) => x !== r)) }
}

export function Actions({ root }: { root: string | null }) {
  const base = useGh(root)
  const repos = useActionRepos(root, base)
  const [target, setTarget] = useState<string | null>(null)
  const [adding, setAdding] = useState('')
  // The same calls, against the repository picked (`{repo}` is the folder's own).
  const gh = useCallback(
    <T = J,>(path: string, opts?: { method?: string; body?: unknown; raw?: boolean }) => base<T>(target ? path.replace('{repo}', target) : path, opts),
    [base, target],
  ) as ReturnType<typeof useGh>
  const [workflow, setWorkflow] = useState<string | 'all'>('all')
  const [picked, setPicked] = useState<number | null>(null)
  const runs = useLoad(async () => ((await gh('/repos/{repo}/actions/runs?per_page=60')).workflow_runs as J[]) ?? [], [gh])
  // Every workflow that has runs, including ones since deleted.
  const names = useMemo(() => [...new Set((runs.data ?? []).map((r) => r.name as string))], [runs.data])
  const shown = (runs.data ?? []).filter((r) => workflow === 'all' || r.name === workflow)
  // Running ones are watched: the list refreshes while anything is going.
  const live = runs.data?.some((r) => r.status !== 'completed')
  useEffect(() => {
    if (!live) return
    const t = window.setInterval(() => void runs.reload(), 10_000)
    return () => window.clearInterval(t)
  }, [live, runs.reload])
  const pick = (r: string | null) => {
    setTarget(r)
    setWorkflow('all')
    setPicked(null)
  }

  return (
    <Split
      empty={shown.length === 0 && runs.data ? 'No workflow runs here yet.' : 'Pick a run to see its jobs and logs.'}
      list={
        <>
          <Stack direction="row" sx={{ gap: u(1), px: u(3), pt: u(2), flexWrap: 'wrap', alignItems: 'center' }}>
            <Pill active={target === null} onClick={() => pick(null)}>
              {repos.own ?? 'This repository'}
            </Pill>
            {repos.extra.map((r) => (
              <Stack key={r} direction="row" sx={{ alignItems: 'center' }}>
                <Pill active={target === r} onClick={() => pick(r)} title={r}>
                  {r}
                </Pill>
                <ButtonBase
                  aria-label={`Stop showing ${r}`}
                  title="Stop showing"
                  onClick={() => {
                    repos.remove(r)
                    if (target === r) pick(null)
                  }}
                  sx={{ width: 16, height: 20, ...T.small, color: 'text.disabled', '&:hover': { color: 'text.primary' } }}
                >
                  ×
                </ButtonBase>
              </Stack>
            ))}
            <InputBase
              value={adding}
              onChange={(e) => setAdding(e.target.value)}
              onKeyDown={(e) => {
                const r = adding.trim().replace(/^https?:\/\/github\.com\//, '').replace(/\.git$/, '')
                if (e.key === 'Enter' && /^[\w.-]+\/[\w.-]+$/.test(r)) {
                  repos.add(r)
                  setAdding('')
                  pick(r)
                }
              }}
              placeholder="+ owner/repo"
              sx={{ width: 112, ...T.meta, px: u(2), border: 1, borderColor: 'divider', borderRadius: '4px', height: 20 }}
            />
          </Stack>
          <Stack direction="row" sx={{ gap: u(1), px: u(3), py: u(2), borderBottom: 1, borderColor: 'divider', flexWrap: 'wrap' }}>
            <Pill active={workflow === 'all'} onClick={() => setWorkflow('all')}>
              All workflows
            </Pill>
            {names.map((n) => (
              <Pill key={n} active={workflow === n} onClick={() => setWorkflow(n)} title={n}>
                {n}
              </Pill>
            ))}
          </Stack>
          {runs.error && <Problem error={runs.error} onRetry={runs.reload} />}
          {runs.loading && !runs.data && <Typography sx={{ ...T.small, color: 'text.secondary', display: 'block', p: u(3) }}>Loading…</Typography>}
          {shown.map((r) => (
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
      detail={picked !== null ? <Run key={`${target}-${picked}`} gh={gh} id={picked} onChanged={runs.reload} /> : null}
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
  if (!r) return <Typography sx={{ ...T.small, color: 'text.secondary', display: 'block', p: u(4) }}>Loading…</Typography>
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
      <Stack sx={{ px: u(4), py: u(3), gap: u(2) }}>
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
    <Box sx={{ border: 1, borderColor: 'divider', borderRadius: '8px', overflow: 'hidden' }}>
      <ButtonBase onClick={onToggle} sx={{ display: 'flex', width: '100%', height: 32, justifyContent: 'flex-start', gap: u(2), px: u(3), '&:hover': { bgcolor: 'action.hover' } }}>
        <StatusMark status={job.status} conclusion={job.conclusion} />
        <Typography sx={{ ...T.small, fontWeight: 600, flex: 1, textAlign: 'left' }} noWrap>
          {job.name}
        </Typography>
        <Typography sx={{ ...T.meta, color: 'text.secondary' }}>
          {duration(job.started_at, job.completed_at)}
        </Typography>
      </ButtonBase>
      {open && (
        <Box sx={{ borderTop: 1, borderColor: 'divider', bgcolor: alpha(theme.palette.text.primary, 0.025) }}>
          <Stack sx={{ px: u(3), py: u(1) }}>
            {(job.steps ?? []).map((s: J) => (
              <Stack key={s.number} direction="row" sx={{ alignItems: 'center', gap: u(2), height: 20 }}>
                <StatusMark status={s.status} conclusion={s.conclusion} size={12} />
                <Typography sx={{ ...T.small, flex: 1, minWidth: 0 }} noWrap>
                  {s.name}
                </Typography>
                <Typography sx={{ ...T.meta, color: 'text.disabled' }}>
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
  if (log.error) return <Typography sx={{ ...T.meta, color: 'error.main', display: 'block', px: u(3), pb: u(2) }}>{log.error}</Typography>
  return (
    <Box
      ref={box}
      component="pre"
      className="select-text"
      sx={{ m: 0, px: u(3), py: u(2), maxHeight: 420, overflow: 'auto', borderTop: 1, borderColor: 'divider', fontFamily: MONO_FONT, fontSize: '11px', lineHeight: '16px', whiteSpace: 'pre-wrap', wordBreak: 'break-word', color: 'text.secondary' }}
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
          <Stack direction="row" sx={{ gap: u(2), px: u(3), py: u(2), borderBottom: 1, borderColor: 'divider' }}>
            <InputBase value={title} onChange={(e) => setTitle(e.target.value)} placeholder="New issue title" sx={{ flex: 1, ...T.small, height: 24, border: 1, borderColor: 'divider', borderRadius: '6px', px: u(2) }} />
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
              icon={<Box sx={{ width: 8, height: 8, borderRadius: '50%', bgcolor: i.state === 'open' ? 'success.main' : 'secondary.main' }} />}
              title={i.title}
              meta={`#${i.number} · ${i.user?.login} · ${i.comments} comment${i.comments === 1 ? '' : 's'} · ${ago(i.updated_at)}`}
              right={
                i.labels?.length ? (
                  <Stack direction="row" sx={{ gap: u(1) }}>
                    {i.labels.slice(0, 2).map((l: J) => (
                      <Box key={l.id} sx={{ px: u(1), borderRadius: '4px', fontSize: '10px', lineHeight: '16px', bgcolor: `#${l.color}33`, color: 'text.secondary' }}>
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
  if (!i) return <Typography sx={{ ...T.small, color: 'text.secondary', display: 'block', p: u(4) }}>Loading…</Typography>
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
      <Stack sx={{ px: u(4), py: u(3), gap: u(4) }}>
        {i.body && (
          <Box sx={{ ...T.body }}>
            <Markdown text={i.body} />
          </Box>
        )}
        {comments.data?.map((c) => (
          <Stack key={c.id} sx={{ gap: u(1), borderTop: 1, borderColor: 'divider', pt: u(3) }}>
            <Typography sx={{ ...T.meta, color: 'text.secondary' }}>
              <b>{c.user?.login}</b> · {ago(c.created_at)}
            </Typography>
            <Box sx={{ ...T.body }}>
              <Markdown text={c.body ?? ''} />
            </Box>
          </Stack>
        ))}
        <Stack sx={{ gap: u(2) }}>
          <InputBase multiline minRows={2} value={text} onChange={(e) => setText(e.target.value)} placeholder="Leave a comment" sx={{ ...T.body, border: 1, borderColor: 'divider', borderRadius: '6px', px: u(2), py: u(1) }} />
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
      {list.data?.length === 0 && <Typography sx={{ ...T.small, color: 'text.secondary', p: u(4) }}>No releases yet.</Typography>}
      <Stack sx={{ maxWidth: 880, px: u(4), py: u(3), gap: u(4) }}>
        {list.data?.map((r) => (
          <Stack key={r.id} sx={{ gap: u(2), pb: u(4), borderBottom: 1, borderColor: 'divider' }}>
            <Stack direction="row" sx={{ alignItems: 'center', gap: u(2) }}>
              <Typography sx={{ ...T.title }}>
                {r.name || r.tag_name}
              </Typography>
              <Box component="span" sx={{ ...T.meta, ...T.mono, px: u(1), borderRadius: '4px', border: 1, borderColor: 'divider', color: 'text.secondary' }}>{r.tag_name}</Box>
              {r.prerelease && <Box component="span" sx={{ ...T.meta, fontWeight: 600, px: u(1), borderRadius: '4px', bgcolor: 'action.selected', color: 'warning.main' }}>Pre-release</Box>}
              {r.draft && <Box component="span" sx={{ ...T.meta, fontWeight: 600, px: u(1), borderRadius: '4px', bgcolor: 'action.selected', color: 'text.secondary' }}>Draft</Box>}
              <Box sx={{ flex: 1 }} />
              <Typography sx={{ ...T.meta, color: 'text.secondary' }}>
                {ago(r.published_at ?? r.created_at)}
              </Typography>
              <Link href={r.html_url} target="_blank" rel="noreferrer" underline="hover" sx={{ ...T.small }}>
                Open
              </Link>
            </Stack>
            {r.body && (
              <Box sx={{ ...T.body }}>
                <Markdown text={r.body} />
              </Box>
            )}
            {r.assets?.length > 0 && (
              <Stack direction="row" sx={{ gap: u(3), flexWrap: 'wrap' }}>
                {r.assets.map((a: J) => (
                  <Link key={a.id} href={a.browser_download_url} target="_blank" rel="noreferrer" underline="hover" sx={{ ...T.meta, ...T.mono }}>
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

