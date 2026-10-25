import { useEffect, useMemo, useState } from 'react'
import { Box, Stack, Typography, IconButton, Dialog, Tooltip, CircularProgress, InputBase, useTheme } from '../../../ui'
import {
  FolderLinear,
  DocumentLinear,
  ChevronRightLinear,
  FolderOpenLinear,
  MonitorLinear,
  CloseCircleLinear,
  SearchNormalLinear,
} from '../../../icons'
import { call } from '../../../ipc'
import { useActiveWorkspace } from '../useActiveWorkspace'
import { FileContent } from './FilePreview'
import { FileCard } from './FileCard'
import { useWorkspace } from '../../workspace/layout'
import { usePanelStore } from '../store/panelStore'
import { isDocument } from '../../office/lib/docs'
import { useIdeStore } from '../../ide/store/ideStore'

interface TreeNode {
  name: string
  path: string
  type: string
  children?: TreeNode[]
}

interface RootTree {
  root: string
  label: string
  nodes: TreeNode[] | null
  error: string | null
}

/** Matches `cmd_list_dir_tree`'s own clamps, so this asks for everything the
 * backend is willing to walk rather than the old 6/1200, which silently cut
 * off most of a real repo. */
const TREE_MAX_DEPTH = 8
const TREE_MAX_ENTRIES = 5000

/** A muted tint per broad file category. Scanning a long tree is mostly
 * pattern-matching on shape and colour rather than reading names, so grouping
 * by what a file IS (source / config / docs / assets) does more work than a
 * unique colour per extension would — and stays legible in both themes
 * because these are palette roles, not fixed hex. */
function fileTint(name: string): string {
  const ext = name.slice(name.lastIndexOf('.') + 1).toLowerCase()
  if (['ts', 'tsx', 'js', 'jsx', 'rs', 'py', 'go', 'java', 'rb', 'c', 'cpp', 'h', 'cs', 'swift', 'kt'].includes(ext)) {
    return 'primary.main'
  }
  if (['json', 'toml', 'yaml', 'yml', 'lock', 'ini', 'env', 'conf', 'cfg'].includes(ext)) return 'warning.main'
  if (['md', 'mdx', 'txt', 'rst', 'adoc'].includes(ext)) return 'success.main'
  if (['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'ico', 'mp4', 'mp3', 'wav', 'woff', 'woff2', 'ttf'].includes(ext)) {
    return 'secondary.main'
  }
  return 'text.disabled'
}

function basename(p: string): string {
  return p.split(/[\\/]/).filter(Boolean).pop() || p
}

/** Keeps a node when it (or anything under it) matches — so filtering shows
 * matching files still sitting in their real folder structure instead of a
 * flat list that loses all context. */
function filterTree(nodes: TreeNode[], q: string): TreeNode[] {
  const out: TreeNode[] = []
  for (const n of nodes) {
    if (n.type === 'directory') {
      const kids = n.children ? filterTree(n.children, q) : []
      if (kids.length > 0 || n.name.toLowerCase().includes(q)) {
        out.push({ ...n, children: kids })
      }
    } else if (n.name.toLowerCase().includes(q)) {
      out.push(n)
    }
  }
  return out
}

/** Workspace file tree — walks EVERY open workspace folder via
 * `cmd_list_dir_tree`, not just the primary one: a second folder added to the
 * workspace used to be invisible here even though the agent could read it.
 * With more than one folder open each root gets its own collapsible section
 * header; with one, the tree renders flat as before.
 *
 * Files preview in place: clicking one expands its contents inline under the
 * row, and the expand button opens the same file full-screen. There's no
 * separate Preview tab — browsing a tree and reading the file you clicked are
 * one task, and bouncing between two tabs to do it meant losing your place in
 * the tree on every file.
 *
 * Root folder resolution: while a secondary thread pane is focused, the tree
 * follows THAT session's own `workspace_folders` (read straight off the
 * already-fetched session list — no backend call needed) instead of the
 * primary thread's workspace. Workspace commands themselves aren't
 * thread-scoped yet, so this is a read-only override, not real per-thread
 * workspace state. */
export function WorkspaceTree() {
  const theme = useTheme()
  // With an Editor panel open, this tree is the editor's file tree, so a
  // click opens the file there instead of expanding a read-only preview under
  // the row. Two behaviours rather than two components: the tree, its filter
  // and its multi-root handling are identical either way, and a second copy
  // would drift.
  const ideLayout = useWorkspace((s) => s.open.includes('ide'))
  const openInEditor = useIdeStore((s) => s.openFile)
  const editingPath = useIdeStore((s) => s.activePath)
  const folders = useActiveWorkspace()
  const [trees, setTrees] = useState<RootTree[] | null>(null)
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [collapsedRoots, setCollapsedRoots] = useState<Set<string>>(new Set())
  const [openFile, setOpenFile] = useState<string | null>(null)
  const [fullscreen, setFullscreen] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [query, setQuery] = useState('')

  // Joined rather than the array itself: `useActiveWorkspace` returns a fresh
  // array each render, so depending on it directly would refetch every tree
  // on every render.
  const foldersKey = folders.join(' ')

  useEffect(() => {
    setTrees(null)
    setExpanded(new Set())
    setOpenFile(null)
    if (folders.length === 0) return
    let cancelled = false
    setLoading(true)
    void Promise.all(
      folders.map((root) =>
        call<TreeNode[]>('cmd_list_dir_tree', { root, maxDepth: TREE_MAX_DEPTH, maxEntries: TREE_MAX_ENTRIES })
          .then((nodes): RootTree => ({ root, label: basename(root), nodes: nodes ?? [], error: null }))
          // One unreadable folder shouldn't blank the whole panel — it gets
          // its own inline error row and the others still render.
          .catch((e): RootTree => ({ root, label: basename(root), nodes: null, error: String(e) })),
      ),
    ).then((results) => {
      if (cancelled) return
      setTrees(results)
      setLoading(false)
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [foldersKey])

  const q = query.trim().toLowerCase()
  const visible = useMemo(() => {
    if (!trees) return null
    if (!q) return trees
    return trees.map((t) => ({ ...t, nodes: t.nodes ? filterTree(t.nodes, q) : null }))
  }, [trees, q])

  const toggleDir = (path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  }

  const toggleRoot = (root: string) => {
    setCollapsedRoots((prev) => {
      const next = new Set(prev)
      if (next.has(root)) next.delete(root)
      else next.add(root)
      return next
    })
  }

  const renderNodes = (nodes: TreeNode[], depth: number): React.ReactNode[] =>
    nodes.map((node) => {
      const isDir = node.type === 'directory'
      // While filtering, directories default to open so matches deep in the
      // tree are actually visible without hand-expanding every level.
      const isOpen = isDir
        ? q
          ? true
          : expanded.has(node.path)
        : ideLayout
          ? editingPath === node.path
          : openFile === node.path
      return (
        <Box key={node.path}>
          <Stack
            direction="row"
            onClick={() =>
              isDir
                ? toggleDir(node.path)
                : isDocument(node.path)
                  ? usePanelStore.getState().openDocument(node.path)
                  : ideLayout
                    ? void openInEditor(node.path)
                    : setOpenFile(isOpen ? null : node.path)
            }
            sx={{
              alignItems: 'center',
              gap: 0.625,
              height: 26,
              pl: `${8 + depth * 14}px`,
              pr: 0.75,
              cursor: 'pointer',
              borderRadius: '6px',
              position: 'relative',
              transition: 'background-color 0.12s',
              bgcolor: !isDir && isOpen ? 'action.selected' : 'transparent',
              '&:hover': { bgcolor: !isDir && isOpen ? 'action.selected' : 'action.hover' },
              '&:hover .file-expand': { opacity: 1 },
              // Indent guides: one hairline per ancestor level, so deep nesting
              // stays readable instead of relying on padding alone to convey
              // depth. Drawn as a repeating gradient rather than N elements —
              // same result, no extra nodes per row.
              ...(depth > 0 && {
                backgroundImage: `repeating-linear-gradient(to right, ${theme.palette.divider} 0 1px, transparent 1px 14px)`,
                backgroundSize: `${depth * 14}px 100%`,
                backgroundRepeat: 'no-repeat',
                backgroundPositionX: '8px',
              }),
            }}
            title={node.path}
          >
            <Box
              sx={{
                width: 14,
                flexShrink: 0,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                color: 'text.disabled',
                // Rotated rather than swapped for a second glyph: the chevron
                // animates between states instead of popping.
                transform: isDir && isOpen ? 'rotate(90deg)' : 'none',
                transition: 'transform 0.15s ease',
              }}
            >
              {isDir && <ChevronRightLinear size={12} color="currentColor" />}
            </Box>
            <Box
              sx={{
                flexShrink: 0,
                display: 'flex',
                // Folders take a single muted accent; files are tinted by what
                // they are, which is what makes a long listing scannable
                // without reading every name.
                color: isDir ? 'primary.main' : fileTint(node.name),
                opacity: isDir ? 0.75 : 0.85,
              }}
            >
              {isDir ? (
                isOpen ? (
                  <FolderOpenLinear size={14} color="currentColor" />
                ) : (
                  <FolderLinear size={14} color="currentColor" />
                )
              ) : (
                <DocumentLinear size={13} color="currentColor" />
              )}
            </Box>
            <Typography
              variant="caption"
              noWrap
              sx={{
                flex: 1,
                minWidth: 0,
                fontSize: '0.75rem',
                lineHeight: 1,
                fontWeight: isDir ? 500 : 400,
                letterSpacing: 0,
                color: isDir ? 'text.primary' : 'text.secondary',
              }}
            >
              {node.name}
            </Typography>
            {!isDir && (
              <Tooltip title="Open fullscreen">
                <IconButton
                  size="small"
                  className="file-expand"
                  onClick={(e) => {
                    e.stopPropagation()
                    setFullscreen(node.path)
                  }}
                  sx={{ width: 18, height: 18, flexShrink: 0, opacity: isOpen ? 1 : 0, transition: 'opacity 0.12s' }}
                >
                  <MonitorLinear size={11} color="currentColor" />
                </IconButton>
              </Tooltip>
            )}
          </Stack>

          {/* Not in the IDE layout — there the file opens in the center
              editor, and an inline copy under the row would be a second,
              read-only view of the same file fighting the one you are
              actually editing. */}
          {!isDir && isOpen && !ideLayout && (
            // The read tool's card: the same look for a file wherever it's previewed.
            <Box sx={{ ml: `${8 + depth * 14 + 7}px`, mr: 0.75, mt: 0.25, mb: 0.75 }}>
              <FileCard path={node.path} onExpand={() => setFullscreen(node.path)} />
            </Box>
          )}
          {isDir && isOpen && node.children && renderNodes(node.children, depth + 1)}
        </Box>
      )
    })

  const hint = (text: string) => (
    <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 1.5 }}>
      {text}
    </Typography>
  )

  const multiRoot = folders.length > 1
  const totalVisible = visible?.reduce((n, t) => n + (t.nodes?.length ?? 0), 0) ?? 0

  return (
    <>
      <Stack sx={{ flex: 1, minHeight: 0, minWidth: 0 }}>
        {folders.length > 0 && (
          <Stack
            direction="row"
            sx={{
              alignItems: 'center',
              gap: 0.75,
              mx: 1,
              mt: 1,
              mb: 0.5,
              px: 1,
              height: 28,
              borderRadius: '7px',
              bgcolor: 'action.hover',
              color: 'text.disabled',
              flexShrink: 0,
              transition: 'background-color 0.12s',
              '&:focus-within': { bgcolor: 'action.selected', color: 'text.secondary' },
            }}
          >
            <SearchNormalLinear size={12} color="currentColor" />
            <InputBase
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Filter files…"
              sx={{ flex: 1, fontSize: '0.75rem', color: 'text.primary', '& input': { p: 0 } }}
            />
            {query && (
              <IconButton size="small" onClick={() => setQuery('')} sx={{ width: 16, height: 16 }}>
                <CloseCircleLinear size={11} color="currentColor" />
              </IconButton>
            )}
          </Stack>
        )}

        <Box sx={{ px: 1, pb: 1, overflowY: 'auto', flex: 1, minHeight: 0, minWidth: 0 }}>
          {folders.length === 0
            ? hint('No workspace folder set — add one from the composer’s folder picker.')
            : loading && !visible
              ? hint('Loading…')
              : q && totalVisible === 0
                ? hint(`Nothing matching “${query}”.`)
                : visible?.map((t) => {
                    const collapsed = collapsedRoots.has(t.root)
                    return (
                      <Box key={t.root} sx={{ mb: multiRoot ? 0.5 : 0 }}>
                        {multiRoot && (
                          <Stack
                            direction="row"
                            onClick={() => toggleRoot(t.root)}
                            sx={{
                              alignItems: 'center',
                              gap: 0.5,
                              px: 0.75,
                              py: 0.5,
                              mt: 0.5,
                              position: 'sticky',
                              top: 0,
                              zIndex: 1,
                              cursor: 'pointer',
                              borderRadius: 1.5,
                              bgcolor: 'background.chrome',
                              '&:hover': { bgcolor: 'action.hover' },
                            }}
                            title={t.root}
                          >
                            <Box
                              sx={{
                                display: 'flex',
                                color: 'text.disabled',
                                transform: collapsed ? 'none' : 'rotate(90deg)',
                                transition: 'transform 0.15s ease',
                              }}
                            >
                              <ChevronRightLinear size={11} color="currentColor" />
                            </Box>
                            <Typography
                              variant="caption"
                              noWrap
                              sx={{
                                flex: 1,
                                minWidth: 0,
                                fontWeight: 700,
                                textTransform: 'uppercase',
                                letterSpacing: 0.4,
                                fontSize: '0.6875rem',
                                color: 'text.secondary',
                              }}
                            >
                              {t.label}
                            </Typography>
                          </Stack>
                        )}
                        {!collapsed &&
                          (t.error
                            ? hint(`Couldn’t read this folder — ${t.error}`)
                            : t.nodes && t.nodes.length === 0
                              ? hint('Empty folder.')
                              : t.nodes && renderNodes(t.nodes, 0))}
                      </Box>
                    )
                  })}
          {loading && visible && (
            <Stack direction="row" sx={{ alignItems: 'center', gap: 1, p: 1.5 }}>
              <CircularProgress size={12} />
              <Typography variant="caption" color="text.secondary">
                Refreshing…
              </Typography>
            </Stack>
          )}
        </Box>
      </Stack>

      <Dialog open={!!fullscreen} onClose={() => setFullscreen(null)} fullScreen>
        <Stack sx={{ height: '100%', minHeight: 0 }}>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 2, py: 1.25, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
            <DocumentLinear size={14} color="currentColor" />
            <Typography variant="body2" sx={{ flex: 1, minWidth: 0, fontFamily: '"Geist Mono", ui-monospace, monospace' }} noWrap title={fullscreen ?? ''}>
              {fullscreen}
            </Typography>
            <IconButton size="small" onClick={() => setFullscreen(null)}>
              <CloseCircleLinear size={16} color="currentColor" />
            </IconButton>
          </Stack>
          <Box sx={{ flex: 1, minHeight: 0, overflow: 'auto', px: 2, py: 1.5 }}>
            {fullscreen && <FileContent path={fullscreen} wrap={false} />}
          </Box>
        </Stack>
      </Dialog>
    </>
  )
}
