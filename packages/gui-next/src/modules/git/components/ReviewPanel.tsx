import { useCallback, useEffect, useMemo, useState } from 'react'
import { Box, Stack, Typography, IconButton, ButtonBase, Button, Menu, MenuItem, Tooltip, CircularProgress } from '../../../ui'
import { CloseCircleLinear, ArrowDownLinear, ArrowRightLinear, RefreshLinear, DocumentTextLinear } from '../../../icons'
import { call } from '../../../ipc'
import { useActiveWorkspaceRoot } from '../../dock/useActiveWorkspace'

interface GitFileChange {
  path: string
  added: number
  removed: number
  status: string
  staged: boolean
}

type ViewMode = 'unified' | 'split'
type Scope = 'unstaged' | 'staged'

const SCOPE_LABELS: Record<Scope, string> = { unstaged: 'Unstaged', staged: 'Staged' }

function fileName(p: string): string {
  return p.split('/').filter(Boolean).pop() ?? p
}
function dirName(p: string): string {
  const parts = p.split('/').filter(Boolean)
  parts.pop()
  return parts.length ? `${parts.join('/')}/` : ''
}

interface DiffLine {
  kind: 'add' | 'del' | 'ctx' | 'hunk'
  text: string
  oldNo: number | null
  newNo: number | null
}

/** Parse a unified diff into numbered lines. Only what the view needs — no
 * rename/mode/binary handling, which `cmd_git_diff_stat` already reports
 * separately. */
function parseDiff(diff: string): DiffLine[] {
  const out: DiffLine[] = []
  let oldNo = 0
  let newNo = 0
  for (const line of diff.split('\n')) {
    if (line.startsWith('@@')) {
      const m = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/)
      if (m) {
        oldNo = parseInt(m[1], 10)
        newNo = parseInt(m[2], 10)
      }
      out.push({ kind: 'hunk', text: line, oldNo: null, newNo: null })
    } else if (line.startsWith('+++') || line.startsWith('---') || line.startsWith('diff ') || line.startsWith('index ')) {
      continue
    } else if (line.startsWith('+')) {
      out.push({ kind: 'add', text: line.slice(1), oldNo: null, newNo: newNo++ })
    } else if (line.startsWith('-')) {
      out.push({ kind: 'del', text: line.slice(1), oldNo: oldNo++, newNo: null })
    } else if (line.startsWith('\\')) {
      continue
    } else {
      out.push({ kind: 'ctx', text: line.slice(1), oldNo: oldNo++, newNo: newNo++ })
    }
  }
  // Trailing blank from the final newline.
  if (out.length && out[out.length - 1].kind === 'ctx' && out[out.length - 1].text === '') out.pop()
  return out
}

const MONO = { fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.6875rem', lineHeight: 1.6 } as const

function lineBg(kind: DiffLine['kind']): string {
  if (kind === 'add') return 'rgba(61, 214, 140, 0.13)'
  if (kind === 'del') return 'rgba(229, 72, 77, 0.13)'
  if (kind === 'hunk') return 'action.hover'
  return 'transparent'
}

function Gutter({ n }: { n: number | null }) {
  return (
    <Box component="span" sx={{ ...MONO, width: 34, flexShrink: 0, textAlign: 'right', pr: 1, color: 'text.disabled', userSelect: 'none' }}>
      {n ?? ''}
    </Box>
  )
}

function UnifiedDiff({ lines }: { lines: DiffLine[] }) {
  return (
    <Box sx={{ overflowX: 'auto' }}>
      {lines.map((l, i) => (
        <Stack key={i} direction="row" sx={{ bgcolor: lineBg(l.kind), minWidth: 'fit-content' }}>
          <Gutter n={l.oldNo} />
          <Gutter n={l.newNo} />
          <Box component="pre" sx={{ ...MONO, m: 0, px: 1, whiteSpace: 'pre', color: l.kind === 'hunk' ? 'text.secondary' : 'text.primary' }}>
            {l.kind === 'add' ? '+' : l.kind === 'del' ? '-' : ' '}
            {l.text}
          </Box>
        </Stack>
      ))}
    </Box>
  )
}

/** Side-by-side view. Adds and deletes inside one run are zipped into rows so
 * a modified line sits opposite the line it replaced, instead of both sides
 * scrolling independently out of sync. */
function SplitDiff({ lines }: { lines: DiffLine[] }) {
  const rows: { left: DiffLine | null; right: DiffLine | null }[] = []
  for (let i = 0; i < lines.length; ) {
    const l = lines[i]
    if (l.kind === 'ctx' || l.kind === 'hunk') {
      rows.push({ left: l, right: l.kind === 'hunk' ? null : l })
      i++
      continue
    }
    const dels: DiffLine[] = []
    const adds: DiffLine[] = []
    while (i < lines.length && lines[i].kind === 'del') dels.push(lines[i++])
    while (i < lines.length && lines[i].kind === 'add') adds.push(lines[i++])
    for (let k = 0; k < Math.max(dels.length, adds.length); k++) {
      rows.push({ left: dels[k] ?? null, right: adds[k] ?? null })
    }
  }

  const Side = ({ line, side }: { line: DiffLine | null; side: 'left' | 'right' }) => (
    <Stack direction="row" sx={{ flex: 1, minWidth: 0, bgcolor: line ? lineBg(line.kind) : 'action.disabledBackground' }}>
      <Gutter n={line ? (side === 'left' ? line.oldNo : line.newNo) : null} />
      <Box component="pre" sx={{ ...MONO, m: 0, px: 0.5, whiteSpace: 'pre', overflowX: 'auto', flex: 1, minWidth: 0 }}>
        {line?.kind === 'hunk' ? '' : (line?.text ?? '')}
      </Box>
    </Stack>
  )

  return (
    <Box>
      {rows.map((r, i) => (
        <Stack key={i} direction="row" sx={{ gap: '1px' }}>
          <Side line={r.left} side="left" />
          <Side line={r.right} side="right" />
        </Stack>
      ))}
    </Box>
  )
}

function FileRow({
  change,
  root,
  scope,
  view,
  expanded,
  onToggle,
}: {
  change: GitFileChange
  root: string
  scope: Scope
  view: ViewMode
  expanded: boolean
  onToggle: () => void
}) {
  const [diff, setDiff] = useState<DiffLine[] | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!expanded || diff) return
    setLoading(true)
    void call<string>('cmd_git_diff_file', { path: root, file: change.path, staged: scope === 'staged' })
      .then((text) => setDiff(parseDiff(text ?? '')))
      .finally(() => setLoading(false))
  }, [expanded, diff, root, change.path, scope])

  return (
    <Box sx={{ borderBottom: 1, borderColor: 'divider' }}>
      <ButtonBase
        onClick={onToggle}
        sx={{ display: 'flex', width: '100%', alignItems: 'center', gap: 1, px: 1.5, py: 0.875, '&:hover': { bgcolor: 'action.hover' } }}
      >
        <Box sx={{ display: 'flex', flexShrink: 0, color: 'text.secondary' }}>
          <DocumentTextLinear size={13} color="currentColor" />
        </Box>
        <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem', flexShrink: 0 }}>
          {fileName(change.path)}
        </Typography>
        <Typography variant="caption" color="text.disabled" noWrap sx={{ flex: 1, minWidth: 0, textAlign: 'left' }}>
          {dirName(change.path)}
        </Typography>
        <Typography variant="caption" sx={{ ...MONO, color: 'success.main', flexShrink: 0 }}>
          +{change.added}
        </Typography>
        <Typography variant="caption" sx={{ ...MONO, color: 'error.main', flexShrink: 0 }}>
          -{change.removed}
        </Typography>
        <Box sx={{ display: 'flex', flexShrink: 0, color: 'text.secondary' }}>
          {expanded ? <ArrowDownLinear size={12} color="currentColor" /> : <ArrowRightLinear size={12} color="currentColor" />}
        </Box>
      </ButtonBase>

      {expanded && (
        <Box sx={{ bgcolor: 'action.hover', borderTop: 1, borderColor: 'divider' }}>
          {loading || !diff ? (
            <Box sx={{ p: 1.5 }}>
              <CircularProgress size={13} />
            </Box>
          ) : diff.length === 0 ? (
            <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 1.5 }}>
              No textual diff (binary file, or no changes).
            </Typography>
          ) : view === 'split' ? (
            <SplitDiff lines={diff} />
          ) : (
            <UnifiedDiff lines={diff} />
          )}
        </Box>
      )}
    </Box>
  )
}

/** Review — every changed file in the workspace with its diff, matching the
 * Files/Live-preview panels' place in the right-hand dock. */
export function ReviewPanel({ onClose }: { onClose?: () => void }) {
  const root = useActiveWorkspaceRoot()
  const [changes, setChanges] = useState<GitFileChange[]>([])
  const [scope, setScope] = useState<Scope>('unstaged')
  const [view, setView] = useState<ViewMode>('unified')
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [scopeAnchor, setScopeAnchor] = useState<HTMLElement | null>(null)
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async () => {
    if (!root) return
    setLoading(true)
    try {
      const list = await call<GitFileChange[]>('cmd_git_diff_stat', { path: root, staged: scope === 'staged' })
      setChanges(list ?? [])
    } finally {
      setLoading(false)
    }
  }, [root, scope])

  useEffect(() => {
    void refresh()
  }, [refresh])

  // A file's cached diff belongs to one scope — collapse everything when the
  // scope changes so nothing shows a staged diff under an unstaged heading.
  useEffect(() => {
    setExpanded(new Set())
  }, [scope])

  const totals = useMemo(
    () => changes.reduce((acc, c) => ({ added: acc.added + c.added, removed: acc.removed + c.removed }), { added: 0, removed: 0 }),
    [changes],
  )

  const toggle = (path: string) =>
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })

  if (!root) {
    return (
      <Box sx={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', p: 2 }}>
        <Typography variant="caption" color="text.secondary">
          No workspace folder set.
        </Typography>
      </Box>
    )
  }

  return (
    <Stack sx={{ flex: 1, minWidth: 0, minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.5, py: 0.75, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <Typography variant="caption" sx={{ fontWeight: 700 }}>
          Review
        </Typography>
        <Box
          sx={{ px: 0.75, py: 0.125, borderRadius: 1, bgcolor: 'action.selected', fontSize: '0.6875rem', fontWeight: 600, color: 'text.secondary' }}
        >
          {changes.length}
        </Box>
        <Box sx={{ flex: 1 }} />
        <Typography variant="caption" sx={{ ...MONO, color: 'success.main' }}>
          +{totals.added}
        </Typography>
        <Typography variant="caption" sx={{ ...MONO, color: 'error.main' }}>
          -{totals.removed}
        </Typography>
        <Tooltip title="Refresh">
          <IconButton size="small" onClick={() => void refresh()} sx={{ width: 22, height: 22 }}>
            <RefreshLinear size={12} color="currentColor" />
          </IconButton>
        </Tooltip>
        {onClose && (
          <Tooltip title="Close panel">
            <IconButton size="small" onClick={onClose} sx={{ width: 22, height: 22 }}>
              <CloseCircleLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        )}
      </Stack>

      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, px: 1.5, py: 0.75, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <Button
          size="small"
          onClick={(e) => setScopeAnchor(e.currentTarget)}
          endIcon={<ArrowDownLinear size={10} color="currentColor" />}
          sx={{ textTransform: 'none', fontSize: '0.75rem', color: 'text.primary', px: 1, py: 0.25, border: 1, borderColor: 'divider', borderRadius: 1.5 }}
        >
          {SCOPE_LABELS[scope]}
        </Button>
        <Menu anchorEl={scopeAnchor} open={!!scopeAnchor} onClose={() => setScopeAnchor(null)}>
          {(Object.keys(SCOPE_LABELS) as Scope[]).map((s) => (
            <MenuItem
              key={s}
              selected={scope === s}
              onClick={() => {
                setScope(s)
                setScopeAnchor(null)
              }}
              sx={{ fontSize: '0.8125rem' }}
            >
              {SCOPE_LABELS[s]}
            </MenuItem>
          ))}
        </Menu>

        <Box sx={{ flex: 1 }} />

        <Stack direction="row" sx={{ border: 1, borderColor: 'divider', borderRadius: 1.5, overflow: 'hidden' }}>
          {(['unified', 'split'] as ViewMode[]).map((m) => (
            <ButtonBase
              key={m}
              onClick={() => setView(m)}
              sx={{
                px: 1.25,
                py: 0.375,
                fontSize: '0.6875rem',
                fontWeight: 600,
                textTransform: 'capitalize',
                bgcolor: view === m ? 'action.selected' : 'transparent',
                color: view === m ? 'text.primary' : 'text.secondary',
                '&:hover': { bgcolor: 'action.hover' },
              }}
            >
              {m}
            </ButtonBase>
          ))}
        </Stack>

        <Button
          size="small"
          onClick={() => setExpanded(expanded.size ? new Set() : new Set(changes.map((c) => c.path)))}
          sx={{ textTransform: 'none', fontSize: '0.6875rem', color: 'text.secondary', px: 0.75, minWidth: 0 }}
        >
          {expanded.size ? 'Collapse all' : 'Expand all'}
        </Button>
      </Stack>

      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto' }}>
        {loading && changes.length === 0 ? (
          <Box sx={{ p: 2 }}>
            <CircularProgress size={14} />
          </Box>
        ) : changes.length === 0 ? (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 2 }}>
            No {SCOPE_LABELS[scope].toLowerCase()} changes.
          </Typography>
        ) : (
          changes.map((c) => (
            <FileRow key={c.path} change={c} root={root} scope={scope} view={view} expanded={expanded.has(c.path)} onToggle={() => toggle(c.path)} />
          ))
        )}
      </Box>
    </Stack>
  )
}
