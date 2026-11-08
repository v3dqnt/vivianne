import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Box, Stack, Typography, IconButton, ButtonBase, Button, Menu, MenuItem, ListItemIcon, Divider, Tooltip, CircularProgress } from '../../../ui'
import {
  CloseCircleLinear,
  ArrowDownLinear,
  ArrowRightLinear,
  RefreshLinear,
  DocumentTextLinear,
  TextWrapLinear,
  ViewLinear,
  ViewOffLinear,
  LayoutTwoColumnLinear,
  FileViewLinear,
  UnfoldLessLinear,
  UnfoldMoreLinear,
  MoreHorizontalLinear,
  CopyLinear,
  ImportLinear,
  CheckLinear,
} from '../../../icons'
import { call } from '../../../ipc'
import { Markdown } from '../../ai/components/Markdown'
import { useActiveWorkspaceRoot } from '../../dock/useActiveWorkspace'
import { useGitStatus } from '../store/gitStatusStore'
import { useDiffOptions } from '../store/diffOptionsStore'
import { gitApplyCommand, hideImports, parseDiff, wordDiffs, type DiffLine, type DiffMode, type Seg } from '../lib/diffView'
import { BTN_SX, MONO_FONT, T, u } from './gitStyle'

interface GitFileChange {
  path: string
  added: number
  removed: number
  status: string
  staged: boolean
}

type Scope = 'unstaged' | 'staged'
const SCOPE_LABELS: Record<Scope, string> = { unstaged: 'Unstaged', staged: 'Staged' }
const MODE_LABELS: Record<DiffMode, string> = { auto: 'Auto', unified: 'Unified', split: 'Split' }
const NEXT_MODE: Record<DiffMode, DiffMode> = { auto: 'unified', unified: 'split', split: 'auto' }

/** Wide enough for two columns of code side by side. */
const SPLIT_FROM_PX = 980
/** Context large enough that git shows the whole file around a change. */
const WHOLE_FILE = 100_000
/** The changed-file list is re-read this often while the tab is open. */
const POLL_MS = 4000

function fileName(p: string): string {
  return p.split('/').filter(Boolean).pop() ?? p
}
function dirName(p: string): string {
  const parts = p.split('/').filter(Boolean)
  parts.pop()
  return parts.length ? `${parts.join('/')}/` : ''
}
const isMarkdown = (p: string) => /\.(md|markdown|mdx)$/i.test(p)

const MONO = { fontFamily: MONO_FONT, fontSize: '0.6875rem', lineHeight: '18px' } as const

function lineBg(kind: DiffLine['kind']): string {
  if (kind === 'add') return 'rgba(61, 214, 140, 0.13)'
  if (kind === 'del') return 'rgba(229, 72, 77, 0.13)'
  if (kind === 'hunk') return 'action.hover'
  return 'transparent'
}

function Gutter({ n }: { n: number | null }) {
  return (
    <Box component="span" sx={{ ...MONO, width: 40, flexShrink: 0, textAlign: 'right', pr: u(2), color: 'text.disabled', userSelect: 'none' }}>
      {n ?? ''}
    </Box>
  )
}

/** A line's text, with the words that changed marked when they are known. */
function LineText({ line, words }: { line: DiffLine; words: Map<DiffLine, Seg[]> }) {
  const segs = words.get(line)
  if (!segs) return <>{line.text}</>
  const mark = line.kind === 'add' ? 'rgba(61, 214, 140, 0.34)' : 'rgba(229, 72, 77, 0.34)'
  return (
    <>
      {segs.map((s, i) =>
        s.changed ? (
          <Box key={i} component="span" sx={{ bgcolor: mark, borderRadius: '2px' }}>
            {s.text}
          </Box>
        ) : (
          s.text
        ),
      )}
    </>
  )
}

const codeSx = (wrap: boolean) => ({ ...MONO, m: 0, px: u(2), whiteSpace: wrap ? 'pre-wrap' : 'pre', wordBreak: wrap ? 'break-word' : 'normal' }) as const

function UnifiedDiff({ lines, words, wrap }: { lines: DiffLine[]; words: Map<DiffLine, Seg[]>; wrap: boolean }) {
  return (
    <Box sx={{ overflowX: wrap ? 'hidden' : 'auto' }}>
      {lines.map((l, i) => (
        <Stack key={i} direction="row" sx={{ bgcolor: lineBg(l.kind), minWidth: wrap ? 0 : 'fit-content' }}>
          <Gutter n={l.oldNo} />
          <Gutter n={l.newNo} />
          <Box component="pre" sx={{ ...codeSx(wrap), flex: 1, minWidth: 0, color: l.kind === 'hunk' ? 'text.secondary' : 'text.primary' }}>
            {l.kind === 'add' ? '+' : l.kind === 'del' ? '-' : ' '}
            <LineText line={l} words={words} />
          </Box>
        </Stack>
      ))}
    </Box>
  )
}

/** Side-by-side view. Adds and deletes inside one run are zipped into rows so
 * a modified line sits opposite the line it replaced, instead of both sides
 * scrolling independently out of sync. */
function SplitDiff({ lines, words, wrap }: { lines: DiffLine[]; words: Map<DiffLine, Seg[]>; wrap: boolean }) {
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
    for (let k = 0; k < Math.max(dels.length, adds.length); k++) rows.push({ left: dels[k] ?? null, right: adds[k] ?? null })
  }

  const side = (line: DiffLine | null, which: 'left' | 'right') => (
    <Stack direction="row" sx={{ flex: 1, minWidth: 0, bgcolor: line ? lineBg(line.kind) : 'action.disabledBackground' }}>
      <Gutter n={line ? (which === 'left' ? line.oldNo : line.newNo) : null} />
      <Box component="pre" sx={{ ...codeSx(wrap), px: u(1), overflowX: wrap ? 'hidden' : 'auto', flex: 1, minWidth: 0 }}>
        {line && line.kind !== 'hunk' ? <LineText line={line} words={words} /> : ''}
      </Box>
    </Stack>
  )

  return (
    <Box>
      {rows.map((r, i) => (
        <Stack key={i} direction="row" sx={{ gap: '1px' }}>
          {side(r.left, 'left')}
          {side(r.right, 'right')}
        </Stack>
      ))}
    </Box>
  )
}

/** What a markdown file reads as: its new version, rendered. */
function RichPreview({ lines }: { lines: DiffLine[] }) {
  const text = lines.filter((l) => l.kind === 'ctx' || l.kind === 'add').map((l) => l.text).join('\n')
  return (
    <Box className="select-text" sx={{ px: u(5), py: u(4), maxWidth: 820 }}>
      <Markdown text={text} />
    </Box>
  )
}

function FileRow({
  change,
  root,
  scope,
  mode,
  expanded,
  onToggle,
}: {
  change: GitFileChange
  root: string
  scope: Scope
  mode: 'unified' | 'split'
  expanded: boolean
  onToggle: () => void
}) {
  const o = useDiffOptions()
  const rich = o.richPreview && isMarkdown(change.path)
  const whole = o.fullFiles || rich
  // A diff is read again when the file changed under it, when the root it
  // belongs to changed (another worktree), or when a setting that shapes it did.
  const key = `${root}|${scope}|${change.path}|${change.added}|${change.removed}|${whole}|${o.hideWhitespace}`
  const [loaded, setLoaded] = useState<{ key: string; raw: string } | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!expanded || loaded?.key === key) return
    let live = true
    setLoading(true)
    void call<string>('cmd_git_diff_file', {
      path: root,
      file: change.path,
      staged: scope === 'staged',
      context: whole ? WHOLE_FILE : null,
      ignoreWhitespace: o.hideWhitespace,
    })
      .then((raw) => live && setLoaded({ key, raw: raw ?? '' }))
      .finally(() => live && setLoading(false))
    return () => {
      live = false
    }
  }, [expanded, key, loaded?.key, root, change.path, scope, whole, o.hideWhitespace])

  const lines = useMemo(() => {
    if (!loaded) return null
    const parsed = parseDiff(loaded.raw)
    return o.hideImports ? hideImports(parsed, change.path) : parsed
  }, [loaded, o.hideImports, change.path])
  const words = useMemo(() => {
    const map = new Map<DiffLine, Seg[]>()
    if (!lines || !o.wordDiffs) return map
    wordDiffs(lines).forEach((segs, i) => map.set(lines[i], segs))
    return map
  }, [lines, o.wordDiffs])

  const shown = loaded?.key === key ? lines : lines && loading ? lines : null

  let body: ReactNode
  if (!shown) body = <Box sx={{ p: u(3) }}><CircularProgress size={12} /></Box>
  else if (shown.length === 0) body = <Typography sx={{ ...T.small, color: 'text.secondary', display: 'block', p: u(3) }}>{o.hideImports || o.hideWhitespace ? 'Nothing left to show with these options.' : 'No textual diff (binary file, or no changes).'}</Typography>
  else if (rich) body = <RichPreview lines={shown} />
  else if (mode === 'split') body = <SplitDiff lines={shown} words={words} wrap={o.wrap} />
  else body = <UnifiedDiff lines={shown} words={words} wrap={o.wrap} />

  return (
    <Box sx={{ borderBottom: 1, borderColor: 'divider' }}>
      <ButtonBase
        onClick={onToggle}
        sx={{ display: 'flex', width: '100%', height: 28, alignItems: 'center', gap: u(2), px: u(3), '&:hover': { bgcolor: 'action.hover' } }}
      >
        <Box sx={{ display: 'flex', flexShrink: 0, color: 'text.secondary' }}>
          {expanded ? <ArrowDownLinear size={12} color="currentColor" /> : <ArrowRightLinear size={12} color="currentColor" />}
        </Box>
        <Box sx={{ display: 'flex', flexShrink: 0, color: 'text.secondary' }}>
          <DocumentTextLinear size={13} color="currentColor" />
        </Box>
        <Typography sx={{ ...T.small, fontWeight: 600, flexShrink: 0 }}>{fileName(change.path)}</Typography>
        <Typography noWrap sx={{ ...T.meta, color: 'text.disabled', flex: 1, minWidth: 0, textAlign: 'left' }}>
          {dirName(change.path)}
        </Typography>
        <Typography variant="caption" sx={{ ...MONO, color: 'success.main', flexShrink: 0 }}>+{change.added}</Typography>
        <Typography variant="caption" sx={{ ...MONO, color: 'error.main', flexShrink: 0 }}>-{change.removed}</Typography>
      </ButtonBase>
      {expanded && <Box sx={{ bgcolor: 'action.hover', borderTop: 1, borderColor: 'divider' }}>{body}</Box>}
    </Box>
  )
}

function Check({ on }: { on: boolean }) {
  return <Box sx={{ ml: 'auto', pl: u(4), display: 'flex', color: 'text.secondary', visibility: on ? 'visible' : 'hidden' }}><CheckLinear size={14} color="currentColor" /></Box>
}

/** The menu beside the diff: how it is laid out and what it leaves out. */
function OptionsMenu({
  anchor,
  onClose,
  onRefresh,
  allExpanded,
  onToggleAll,
  onCopyApply,
}: {
  anchor: HTMLElement | null
  onClose: () => void
  onRefresh: () => void
  allExpanded: boolean
  onToggleAll: () => void
  onCopyApply: () => void
}) {
  const o = useDiffOptions()
  const row = (icon: ReactNode, label: string, onClick: () => void, check?: boolean) => (
    <MenuItem
      onClick={() => {
        onClick()
        onClose()
      }}
      sx={{ ...T.body, gap: u(1), minWidth: 232 }}
    >
      <ListItemIcon sx={{ minWidth: 28, color: 'text.secondary' }}>{icon}</ListItemIcon>
      {label}
      {check !== undefined && <Check on={check} />}
    </MenuItem>
  )
  const toggle = (icon: ReactNode, label: string, value: boolean, patch: Partial<typeof o>) => row(icon, label, () => o.set(patch), value)
  return (
    <Menu anchorEl={anchor} open={!!anchor} onClose={onClose} slotProps={{ paper: { sx: { borderRadius: '12px', py: u(1) } } }}>
      {row(<RefreshLinear size={15} color="currentColor" />, 'Refresh', onRefresh)}
      {toggle(<TextWrapLinear size={15} color="currentColor" />, 'Word wrap', o.wrap, { wrap: !o.wrap })}
      {row(<LayoutTwoColumnLinear size={15} color="currentColor" />, `Switch to ${MODE_LABELS[NEXT_MODE[o.mode]]} diff`, () => o.set({ mode: NEXT_MODE[o.mode] }))}
      {row(allExpanded ? <UnfoldLessLinear size={15} color="currentColor" /> : <UnfoldMoreLinear size={15} color="currentColor" />, allExpanded ? 'Collapse all diffs' : 'Expand all diffs', onToggleAll)}
      <Divider sx={{ my: u(1) }} />
      {toggle(<DocumentTextLinear size={15} color="currentColor" />, 'Load full files', o.fullFiles, { fullFiles: !o.fullFiles })}
      {toggle(<FileViewLinear size={15} color="currentColor" />, 'Rich preview', o.richPreview, { richPreview: !o.richPreview })}
      {toggle(<MoreHorizontalLinear size={15} color="currentColor" />, 'Word diffs', o.wordDiffs, { wordDiffs: !o.wordDiffs })}
      {toggle(o.hideWhitespace ? <ViewOffLinear size={15} color="currentColor" /> : <ViewLinear size={15} color="currentColor" />, 'Hide white space', o.hideWhitespace, { hideWhitespace: !o.hideWhitespace })}
      {toggle(<ImportLinear size={15} color="currentColor" />, 'Hide imports', o.hideImports, { hideImports: !o.hideImports })}
      {row(<CopyLinear size={15} color="currentColor" />, 'Copy git apply command', onCopyApply)}
    </Menu>
  )
}

/** Review: every changed file in the open worktree with its diff. It follows
 * the worktree: another thread's worktree is another root, and the list, the
 * open diffs and the counts all start over for it. */
export function ReviewPanel({ onClose }: { onClose?: () => void }) {
  const root = useActiveWorkspaceRoot()
  const branch = useGitStatus((s) => (s.root === root ? s.summary?.branch : null))
  const o = useDiffOptions()
  const [changes, setChanges] = useState<GitFileChange[]>([])
  const [scope, setScope] = useState<Scope>('unstaged')
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [scopeAnchor, setScopeAnchor] = useState<HTMLElement | null>(null)
  const [menuAnchor, setMenuAnchor] = useState<HTMLElement | null>(null)
  const [loading, setLoading] = useState(false)
  const [width, setWidth] = useState(0)
  const listRef = useRef<HTMLDivElement | null>(null)
  const shownRoot = useRef<string | null>(null)

  const refresh = useCallback(
    async (quiet = false) => {
      if (!root) return
      if (!quiet) setLoading(true)
      try {
        const list = (await call<GitFileChange[]>('cmd_git_diff_stat', { path: root, staged: scope === 'staged' })) ?? []
        // Only a change in the list is a state change: a poll that finds the
        // same files must not redraw the open diffs.
        setChanges((prev) => (JSON.stringify(prev) === JSON.stringify(list) ? prev : list))
      } finally {
        if (!quiet) setLoading(false)
      }
    },
    [root, scope],
  )

  // Another worktree, or another scope: nothing from the last one carries over.
  useEffect(() => {
    if (shownRoot.current !== root) setChanges([])
    shownRoot.current = root
    setExpanded(new Set())
    void refresh()
  }, [root, scope, refresh])

  // Kept current: on a timer while it is on screen, when the window comes
  // back, and the moment the repository's own status moves.
  useEffect(() => {
    const tick = () => document.visibilityState === 'visible' && void refresh(true)
    const t = window.setInterval(tick, POLL_MS)
    window.addEventListener('focus', tick)
    return () => {
      window.clearInterval(t)
      window.removeEventListener('focus', tick)
    }
  }, [refresh])
  const status = useGitStatus((s) => (s.root === root && s.summary ? `${s.summary.branch}|${s.summary.lastCommit}|${s.summary.staged}|${s.summary.unstaged}|${s.summary.untracked}` : ''))
  useEffect(() => {
    if (status) void refresh(true)
  }, [status, refresh])

  useEffect(() => {
    const el = listRef.current
    if (!el) return
    const ro = new ResizeObserver(([e]) => setWidth(e.contentRect.width))
    ro.observe(el)
    return () => ro.disconnect()
  }, [root])

  const mode: 'unified' | 'split' = o.mode === 'auto' ? (width >= SPLIT_FROM_PX ? 'split' : 'unified') : o.mode
  const totals = useMemo(() => changes.reduce((acc, c) => ({ added: acc.added + c.added, removed: acc.removed + c.removed }), { added: 0, removed: 0 }), [changes])
  const allExpanded = changes.length > 0 && expanded.size === changes.length
  const toggleAll = () => setExpanded(allExpanded ? new Set() : new Set(changes.map((c) => c.path)))
  const toggle = (path: string) =>
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })

  const copyApply = async () => {
    if (!root) return
    const parts = await Promise.all(changes.map((c) => call<string>('cmd_git_diff_file', { path: root, file: c.path, staged: scope === 'staged', context: null, ignoreWhitespace: false })))
    void navigator.clipboard.writeText(gitApplyCommand(parts.filter(Boolean).join('\n'))).catch(() => {
      // WebView2 can refuse clipboard access; nothing useful to show.
    })
  }

  if (!root) {
    return (
      <Box sx={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', p: u(4) }}>
        <Typography sx={{ ...T.small, color: 'text.secondary' }}>No workspace folder set.</Typography>
      </Box>
    )
  }

  return (
    <Stack sx={{ flex: 1, minWidth: 0, minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: u(2), px: u(3), height: 36, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <Button
          size="small"
          onClick={(e) => setScopeAnchor(e.currentTarget)}
          endIcon={<ArrowDownLinear size={10} color="currentColor" />}
          sx={{ ...BTN_SX, textTransform: 'none', color: 'text.primary', border: 1, borderColor: 'divider', '& .MuiButton-endIcon': { ml: u(1) } }}
        >
          {SCOPE_LABELS[scope]} · {changes.length}
        </Button>
        <Typography variant="caption" sx={{ ...MONO, color: 'success.main' }}>+{totals.added}</Typography>
        <Typography variant="caption" sx={{ ...MONO, color: 'error.main' }}>-{totals.removed}</Typography>
        {branch && (
          <Typography noWrap title={root} sx={{ ...T.meta, ...T.mono, color: 'text.disabled', minWidth: 0 }}>
            {branch}
          </Typography>
        )}
        <Menu anchorEl={scopeAnchor} open={!!scopeAnchor} onClose={() => setScopeAnchor(null)}>
          {(Object.keys(SCOPE_LABELS) as Scope[]).map((s) => (
            <MenuItem
              key={s}
              selected={scope === s}
              onClick={() => {
                setScope(s)
                setScopeAnchor(null)
              }}
              sx={{ ...T.small }}
            >
              {SCOPE_LABELS[s]}
            </MenuItem>
          ))}
        </Menu>

        <Box sx={{ flex: 1 }} />

        <Stack direction="row" sx={{ height: 24, border: 1, borderColor: 'divider', borderRadius: '6px', overflow: 'hidden', boxSizing: 'border-box' }}>
          {(['auto', 'unified', 'split'] as DiffMode[]).map((m) => (
            <ButtonBase
              key={m}
              onClick={() => o.set({ mode: m })}
              sx={{
                px: u(2),
                ...T.meta,
                fontWeight: 600,
                bgcolor: o.mode === m ? 'action.selected' : 'transparent',
                color: o.mode === m ? 'text.primary' : 'text.secondary',
                '&:hover': { bgcolor: 'action.hover' },
              }}
            >
              {MODE_LABELS[m]}
            </ButtonBase>
          ))}
        </Stack>

        <Tooltip title="Diff options">
          <IconButton size="small" aria-label="Diff options" onClick={(e) => setMenuAnchor(e.currentTarget)} sx={{ width: 24, height: 24 }}>
            <MoreHorizontalLinear size={14} color="currentColor" />
          </IconButton>
        </Tooltip>
        <OptionsMenu
          anchor={menuAnchor}
          onClose={() => setMenuAnchor(null)}
          onRefresh={() => void refresh()}
          allExpanded={allExpanded}
          onToggleAll={toggleAll}
          onCopyApply={() => void copyApply()}
        />
        {onClose && (
          <Tooltip title="Close panel">
            <IconButton size="small" onClick={onClose} sx={{ width: 24, height: 24 }}>
              <CloseCircleLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        )}
      </Stack>

      <Box ref={listRef} sx={{ flex: 1, minHeight: 0, overflowY: 'auto' }}>
        {loading && changes.length === 0 ? (
          <Box sx={{ p: u(4) }}>
            <CircularProgress size={14} />
          </Box>
        ) : changes.length === 0 ? (
          <Typography sx={{ ...T.small, color: 'text.secondary', display: 'block', p: u(4) }}>No {SCOPE_LABELS[scope].toLowerCase()} changes.</Typography>
        ) : (
          changes.map((c) => <FileRow key={`${root}|${scope}|${c.path}`} change={c} root={root} scope={scope} mode={mode} expanded={expanded.has(c.path)} onToggle={() => toggle(c.path)} />)
        )}
      </Box>
    </Stack>
  )
}
