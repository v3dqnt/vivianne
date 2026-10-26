import { useState } from 'react'
import {
  Box,
  Button,
  ButtonBase,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Stack,
  TextField,
  Tooltip,
  Typography,
} from '../../../ui'
import { call } from '../../../ipc'
import { useAgentStore } from '../../ai/store/agentStore'
import { useGitStatus, changedCount, type BranchPr } from '../store/gitStatusStore'
import { useGitStore } from '../store/gitStore'
import { useAppView } from './GitView'

/** Hand the agent a git job in the chat, and show the chat. */
export function askAgent(text: string) {
  useAppView.setState({ view: 'chat' })
  void useAgentStore.getState().sendMessage(text)
}

/** A pull request's state and checks as one small chip. */
export function PrChip({ pr, onClick }: { pr: BranchPr; onClick?: () => void }) {
  const color = pr.state === 'merged' ? '#a371f7' : pr.state === 'closed' ? 'error.main' : pr.draft ? 'text.secondary' : 'success.main'
  const checks = pr.checks === 'failing' ? '✕' : pr.checks === 'running' ? '…' : pr.checks === 'passing' ? '✓' : ''
  const checksColor = pr.checks === 'failing' ? 'error.main' : pr.checks === 'running' ? 'warning.main' : 'success.main'
  return (
    <Tooltip title={`${pr.title} · ${pr.state === 'merged' ? 'Merged' : pr.state === 'closed' ? 'Closed' : pr.draft ? 'Draft' : 'Open'}${pr.checks !== 'none' ? ` · checks ${pr.checks}` : ''}`}>
      <ButtonBase
        onClick={onClick ?? (() => window.open(pr.url, '_blank'))}
        sx={{ display: 'flex', alignItems: 'center', gap: 0.5, px: 0.75, py: 0.125, borderRadius: '6px', border: 1, borderColor: 'divider', fontSize: '0.6875rem', '&:hover': { bgcolor: 'action.hover' } }}
      >
        <Box sx={{ width: 7, height: 7, borderRadius: '50%', bgcolor: color }} />
        <Box component="span" sx={{ fontWeight: 600 }}>#{pr.number}</Box>
        {checks && <Box component="span" sx={{ color: checksColor, fontWeight: 700 }}>{checks}</Box>}
      </ButtonBase>
    </Tooltip>
  )
}

/**
 * The repository at a glance, under the project's name: branch, what's
 * unpushed (↑) and unpulled (↓), what's uncommitted (●), and this branch's
 * pull request with its checks. Click to open the Git view.
 */
export function GitStatusChip() {
  const s = useGitStatus((x) => x.summary)
  const pr = useGitStatus((x) => x.pr)
  if (!s) return null
  const changed = changedCount(s)
  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, minWidth: 0 }}>
      <ButtonBase
        onClick={() => useAppView.setState({ view: 'git' })}
        title="Open the Git view"
        sx={{ display: 'flex', alignItems: 'center', gap: 0.75, minWidth: 0, px: 0.75, py: 0.25, borderRadius: '6px', fontSize: '0.71875rem', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}
      >
        <Box component="span" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 140 }}>
          {s.branch || 'detached'}
        </Box>
        {s.ahead > 0 && <Box component="span" title={`${s.ahead} to push`}>↑{s.ahead}</Box>}
        {s.behind > 0 && <Box component="span" title={`${s.behind} to pull`}>↓{s.behind}</Box>}
        {changed > 0 && (
          <Box component="span" title={`${changed} uncommitted`} sx={{ color: s.conflicted ? 'error.main' : 'warning.main' }}>
            ●{changed}
          </Box>
        )}
      </ButtonBase>
      {pr && <PrChip pr={pr} />}
    </Stack>
  )
}

/** The everyday git moves for this repository, with what each would do. */
export function GitQuickActions() {
  const root = useGitStatus((x) => x.root)
  const s = useGitStatus((x) => x.summary)
  const pr = useGitStatus((x) => x.pr)
  const repo = useGitStatus((x) => x.repo)
  const refresh = useGitStatus((x) => x.refresh)
  const refreshGithub = useGitStatus((x) => x.refreshGithub)
  const [dialog, setDialog] = useState<'commit' | 'pr' | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [note, setNote] = useState<{ text: string; error: boolean } | null>(null)
  if (!root || !s) return null
  const changed = changedCount(s)
  const onDefault = !!s.defaultBranch && s.branch === s.defaultBranch

  const run = async (label: string, f: () => Promise<string | null | undefined>) => {
    setBusy(label)
    setNote(null)
    try {
      const out = await f()
      setNote({ text: out || `${label}: done`, error: false })
      await refresh()
      void refreshGithub()
      void useGitStore.getState().refresh(root)
    } catch (e) {
      setNote({ text: e instanceof Error ? e.message : String(e), error: true })
    } finally {
      setBusy(null)
    }
  }

  const sync = s.upstream ? (s.behind && s.ahead ? 'Sync' : s.behind ? 'Pull' : s.ahead ? 'Push' : null) : 'Publish branch'

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, flexWrap: 'wrap' }}>
      {note && (
        <Typography variant="caption" color={note.error ? 'error' : 'text.secondary'} noWrap sx={{ maxWidth: 360 }} title={note.text}>
          {note.text}
        </Typography>
      )}
      {changed > 0 && (
        <Tooltip title={`Commit ${changed} change${changed === 1 ? '' : 's'}, with a message written from the diff`}>
          <Button size="small" variant="contained" disableElevation onClick={() => setDialog('commit')} sx={{ borderRadius: '8px', py: 0.25, fontSize: '0.75rem' }}>
            Commit {changed}
          </Button>
        </Tooltip>
      )}
      {sync && (
        <Tooltip title={s.upstream ? `${s.behind} to pull, ${s.ahead} to push` : 'Push this branch to GitHub so it can have a pull request'}>
          <span>
            <Button
              size="small"
              variant="outlined"
              disabled={!!busy}
              onClick={() => void run(sync, () => call<string>('cmd_git_sync', { path: root, action: s.upstream ? (s.behind && s.ahead ? 'sync' : s.behind ? 'pull' : 'push') : 'push' }))}
              sx={{ borderRadius: '8px', py: 0.25, fontSize: '0.75rem' }}
            >
              {busy === sync ? '…' : `${sync}${s.ahead ? ` ↑${s.ahead}` : ''}${s.behind ? ` ↓${s.behind}` : ''}`}
            </Button>
          </span>
        </Tooltip>
      )}
      {repo && !onDefault && (!pr || pr.state !== 'open') && s.branch && (
        <Tooltip title={s.upstream ? 'Open a pull request for this branch' : 'Pushes the branch, then opens a pull request'}>
          <Button size="small" variant="outlined" onClick={() => setDialog('pr')} sx={{ borderRadius: '8px', py: 0.25, fontSize: '0.75rem' }}>
            Create PR
          </Button>
        </Tooltip>
      )}
      {pr?.checks === 'failing' && (
        <Tooltip title={`Failing: ${pr.failing.join(', ')}. The agent reads the logs, fixes the cause on this branch and pushes.`}>
          <Button
            size="small"
            variant="outlined"
            color="error"
            onClick={() =>
              askAgent(
                `The checks on pull request #${pr.number} are failing: ${pr.failing.join(', ')}. Read the failing runs' logs with the github tool (actions run / checks), find the cause, fix it on this branch, commit, push, and tell me when the checks pass.`,
              )
            }
            sx={{ borderRadius: '8px', py: 0.25, fontSize: '0.75rem' }}
          >
            Fix checks with the agent
          </Button>
        </Tooltip>
      )}
      {changed > 0 && (
        <Tooltip title="The agent reads the uncommitted diff and points out bugs and risks before you commit">
          <Button
            size="small"
            variant="text"
            onClick={() => askAgent('Review my uncommitted changes (git diff HEAD, and new files) for bugs, risky changes and anything missed. List what you find by file and line; change nothing yet.')}
            sx={{ borderRadius: '8px', py: 0.25, fontSize: '0.75rem' }}
          >
            Review changes
          </Button>
        </Tooltip>
      )}
      {dialog === 'commit' && <CommitDialog root={root} onClose={() => setDialog(null)} onDone={(t) => void run('Commit', async () => t)} />}
      {dialog === 'pr' && (
        <PrDialog
          root={root}
          branch={s.branch}
          base={s.defaultBranch ?? 'main'}
          needsPush={!s.upstream || s.ahead > 0}
          onClose={() => setDialog(null)}
          onDone={(t) => void run('Pull request', async () => t)}
        />
      )}
    </Stack>
  )
}

function CommitDialog({ root, onClose, onDone }: { root: string; onClose: () => void; onDone: (summary: string) => void }) {
  const [title, setTitle] = useState('')
  const [body, setBody] = useState('')
  const [writing, setWriting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const write = async () => {
    setWriting(true)
    setError(null)
    try {
      const m = await call<{ title: string; body: string }>('cmd_git_suggest', { path: root, what: 'commit', base: null })
      if (m) {
        setTitle(m.title)
        setBody(m.body)
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setWriting(false)
    }
  }
  return (
    <Dialog open onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle sx={{ fontSize: '1rem', fontWeight: 700, pb: 0.5 }}>Commit all changes</DialogTitle>
      <DialogContent>
        <Stack sx={{ gap: 1.25, pt: 1 }}>
          <TextField size="small" autoFocus label="Summary" placeholder="Fix the login timeout" value={title} onChange={(e) => setTitle(e.target.value)} helperText={title.length > 72 ? `${title.length} characters: keep it under 72` : ' '} />
          <TextField size="small" label="Details (optional)" multiline minRows={3} value={body} onChange={(e) => setBody(e.target.value)} />
          <Box>
            <Button size="small" onClick={() => void write()} disabled={writing}>
              {writing ? 'Writing…' : 'Write it from the changes'}
            </Button>
          </Box>
          {error && <Typography variant="caption" color="error">{error}</Typography>}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button size="small" onClick={onClose}>
          Cancel
        </Button>
        <Button
          size="small"
          variant="contained"
          disableElevation
          disabled={!title.trim() || saving}
          onClick={async () => {
            setSaving(true)
            try {
              const made = await call<string>('cmd_git_commit', { path: root, message: body.trim() ? `${title.trim()}\n\n${body.trim()}` : title.trim(), files: null })
              onDone(`Committed ${made ?? ''}`)
              onClose()
            } catch (e) {
              setError(e instanceof Error ? e.message : String(e))
            } finally {
              setSaving(false)
            }
          }}
        >
          Commit
        </Button>
      </DialogActions>
    </Dialog>
  )
}

function PrDialog({ root, branch, base: defaultBase, needsPush, onClose, onDone }: { root: string; branch: string; base: string; needsPush: boolean; onClose: () => void; onDone: (summary: string) => void }) {
  const [title, setTitle] = useState('')
  const [body, setBody] = useState('')
  const [base, setBase] = useState(defaultBase)
  const [draft, setDraft] = useState(false)
  const [writing, setWriting] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const write = async () => {
    setWriting(true)
    setError(null)
    try {
      const m = await call<{ title: string; body: string }>('cmd_git_suggest', { path: root, what: 'pr', base })
      if (m) {
        setTitle(m.title)
        setBody(m.body)
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setWriting(false)
    }
  }
  return (
    <Dialog open onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle sx={{ fontSize: '1rem', fontWeight: 700, pb: 0.5 }}>Open a pull request</DialogTitle>
      <DialogContent>
        <Stack sx={{ gap: 1.25, pt: 1 }}>
          <Typography variant="caption" color="text.secondary">
            <code>{branch}</code> into{' '}
            <Box component="input" value={base} onChange={(e: React.ChangeEvent<HTMLInputElement>) => setBase(e.target.value)} sx={{ width: 120, font: 'inherit', fontFamily: '"Geist Mono", ui-monospace, monospace', border: 1, borderColor: 'divider', borderRadius: '4px', px: 0.5, bgcolor: 'transparent', color: 'text.primary' }} />
            {needsPush && ' · the branch is pushed first'}
          </Typography>
          <TextField size="small" autoFocus label="Title" value={title} onChange={(e) => setTitle(e.target.value)} />
          <TextField size="small" label="Description" multiline minRows={6} value={body} onChange={(e) => setBody(e.target.value)} placeholder={'## What\n- …\n\n## Why\n…\n\n## Testing\n…'} />
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
            <Button size="small" onClick={() => void write()} disabled={writing}>
              {writing ? 'Writing…' : 'Write it from the commits'}
            </Button>
            <Box component="label" sx={{ display: 'flex', alignItems: 'center', gap: 0.5, fontSize: '0.75rem', color: 'text.secondary' }}>
              <input type="checkbox" checked={draft} onChange={(e) => setDraft(e.target.checked)} /> Draft
            </Box>
          </Stack>
          {error && <Typography variant="caption" color="error">{error}</Typography>}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button size="small" onClick={onClose}>
          Cancel
        </Button>
        <Button
          size="small"
          variant="contained"
          disableElevation
          disabled={!title.trim() || saving}
          onClick={async () => {
            setSaving(true)
            setError(null)
            try {
              if (needsPush) await call('cmd_git_sync', { path: root, action: 'push' })
              const made = await call<{ number: number; html_url: string }>('cmd_github', {
                cwd: root,
                path: '/repos/{repo}/pulls',
                method: 'POST',
                body: { title: title.trim(), body, head: branch, base, draft },
                raw: null,
              })
              onDone(`Opened pull request #${made?.number}`)
              onClose()
            } catch (e) {
              setError(e instanceof Error ? e.message : String(e))
            } finally {
              setSaving(false)
            }
          }}
        >
          {needsPush ? 'Push and open' : 'Open pull request'}
        </Button>
      </DialogActions>
    </Dialog>
  )
}
