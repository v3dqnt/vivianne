import { useEffect, useMemo, useRef, useState } from 'react'
import { Box, ButtonBase, Stack, Typography, IconButton, Tooltip, CircularProgress, InputBase, useTheme } from '../../../ui'
import {
  FolderLinear, DocumentLinear, ChevronRightLinear, FolderOpenLinear,
  CloseCircleLinear, SearchNormalLinear, RefreshLinear, CopyLinear,
} from '../../../icons'
import { call } from '../../../ipc'
import { useActiveWorkspace } from '../../dock/useActiveWorkspace'
import { FileContent } from '../../dock/components/FilePreview'
import { basename, changeFor, filterTree, isVisualFile, type TreeNode } from './fileTree'
import { useFileChanges } from './useChanges'
import { ChangedFileView } from './ChangedFileView'
import { usePanelStore } from '../../dock/store/panelStore'
import { useFilesStore } from './store'

interface RootTree {
  root: string
  label: string
  nodes: TreeNode[] | null
  error: string | null
}

/** Matches `cmd_list_dir_tree`'s own clamps. */
const TREE_MAX_DEPTH = 8
const TREE_MAX_ENTRIES = 5000

function fileTint(name: string): string {
  const ext = name.slice(name.lastIndexOf('.') + 1).toLowerCase()
  if (['ts', 'tsx', 'js', 'jsx', 'rs', 'py', 'go', 'java', 'rb', 'c', 'cpp', 'h', 'cs', 'swift', 'kt'].includes(ext)) return 'primary.main'
  if (['json', 'toml', 'yaml', 'yml', 'lock', 'ini', 'env', 'conf', 'cfg'].includes(ext)) return 'warning.main'
  if (['md', 'mdx', 'txt', 'rst', 'adoc'].includes(ext)) return 'success.main'
  if (['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'ico', 'mp4', 'mp3', 'wav', 'woff', 'woff2', 'ttf'].includes(ext)) return 'secondary.main'
  return 'text.disabled'
}

/**
 * Every file of every folder in the active workspace. Folders expand in place,
 * a file expands under its own row, and a changed file opens on its diff.
 */
export function FilesPanel() {
  const theme = useTheme()
  const folders = useActiveWorkspace()
  const [trees, setTrees] = useState<RootTree[] | null>(null)
  const [loading, setLoading] = useState(false)
  const [reload, setReload] = useState(0)
  const [query, setQuery] = useState('')
  const [collapsedRoots, setCollapsedRoots] = useState<Set<string>>(new Set())

  const expanded = useFilesStore((s) => s.expanded)
  const openFile = useFilesStore((s) => s.openFile)
  const flash = useFilesStore((s) => s.flash)
  const toggleFolder = useFilesStore((s) => s.toggleFolder)
  const setOpenFile = useFilesStore((s) => s.setOpenFile)
  const flashFile = useFilesStore((s) => s.flashFile)
  const changes = useFileChanges(folders)

  const foldersKey = folders.join(' ')

  useEffect(() => {
    setTrees(null)
    if (folders.length === 0) return
    let cancelled = false
    setLoading(true)
    void Promise.all(
      folders.map((root) =>
        call<TreeNode[]>('cmd_list_dir_tree', { root, maxDepth: TREE_MAX_DEPTH, maxEntries: TREE_MAX_ENTRIES })
          .then((nodes): RootTree => ({ root, label: basename(root), nodes: nodes ?? [], error: null }))
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
  }, [foldersKey, reload])

  // After `revealFile`: bring the row into view and let the highlight fade.
  const bodyRef = useRef<HTMLDivElement | null>(null)
  useEffect(() => {
    if (!flash) return
    const el = bodyRef.current?.querySelector(`[data-path="${CSS.escape(flash)}"]`)
    el?.scrollIntoView({ block: 'center' })
    const t = window.setTimeout(() => flashFile(null), 1200)
    return () => window.clearTimeout(t)
  }, [flash, flashFile])

  const q = query.trim().toLowerCase()
  const visible = useMemo(() => {
    if (!trees) return null
    if (!q) return trees
    return trees.map((t) => ({ ...t, nodes: t.nodes ? filterTree(t.nodes, q) : null }))
  }, [trees, q])

  const renderNodes = (nodes: TreeNode[], depth: number, root: string): React.ReactNode[] =>
    nodes.map((node) => {
      const isDir = node.type === 'directory'
      // While filtering, folders default to open so matches deep in the tree
      // are visible without hand-expanding every level.
      const isOpen = isDir ? (q ? true : expanded.has(node.path)) : openFile === node.path
      return (
        <Box key={node.path}>
          <Stack
            data-path={node.path}
            direction="row"
            onClick={() => (isDir ? toggleFolder(node.path) : setOpenFile(isOpen ? null : node.path))}
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
              bgcolor: !isDir && isOpen ? 'action.selected' : flash === node.path ? 'action.selected' : 'transparent',
              '&:hover': { bgcolor: 'action.hover' },
              ...(depth > 0 && {
                backgroundImage: `repeating-linear-gradient(to right, ${theme.palette.divider} 0 1px, transparent 1px 14px)`,
                backgroundSize: `${depth * 14}px 100%`,
                backgroundRepeat: 'no-repeat',
                backgroundPositionX: '8px',
              }),
            }}
            title={node.path}
          >
            <Box sx={{ width: 14, flexShrink: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'text.disabled', transform: isDir && isOpen ? 'rotate(90deg)' : 'none', transition: 'transform 0.15s ease' }}>
              {isDir && <ChevronRightLinear size={12} color="currentColor" />}
            </Box>
            <Box sx={{ flexShrink: 0, display: 'flex', color: isDir ? 'primary.main' : fileTint(node.name), opacity: isDir ? 0.75 : 0.85 }}>
              {isDir ? (isOpen ? <FolderOpenLinear size={14} color="currentColor" /> : <FolderLinear size={14} color="currentColor" />) : <DocumentLinear size={13} color="currentColor" />}
            </Box>
            <Typography
              variant="caption"
              noWrap
              sx={{ flex: 1, minWidth: 0, fontSize: '0.75rem', lineHeight: 1, fontWeight: isDir ? 500 : 400, letterSpacing: 0, color: isDir ? 'text.primary' : 'text.secondary' }}
            >
              {node.name}
            </Typography>
            {!isDir && (() => {
              const c = changeFor(changes, node.path)
              if (!c) return null
              return (
                <Stack direction="row" sx={{ gap: 0.5, flexShrink: 0, fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.6875rem', lineHeight: 1 }}>
                  <Box component="span" sx={{ color: 'success.main' }}>+{c.additions}</Box>
                  {c.status !== '??' && <Box component="span" sx={{ color: 'error.main' }}>−{c.deletions}</Box>}
                </Stack>
              )
            })()}
          </Stack>
          {!isDir && isOpen && (
            <Box sx={{ ml: `${8 + depth * 14 + 7}px`, mr: 0.75, mt: 0.25, mb: 0.75 }}>
              <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, px: 1, py: 0.25, mb: 0.25 }}>
                <Typography variant="caption" color="text.secondary" noWrap sx={{ flex: 1, minWidth: 0, fontSize: '0.625rem', fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
                  {node.path.slice(root.length + 1)}
                </Typography>
                {isVisualFile(node.path) && (
                  <ButtonBase
                    onClick={(e) => {
                      e.stopPropagation()
                      usePanelStore.getState().openDocument(node.path)
                    }}
                    sx={{ px: 0.75, height: 20, borderRadius: '6px', fontSize: '0.625rem', fontWeight: 600, color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}
                  >
                    Open in Browser
                  </ButtonBase>
                )}
                <Tooltip title="Copy path">
                  <IconButton
                    size="small"
                    aria-label="Copy path"
                    onClick={(e) => {
                      e.stopPropagation()
                      void navigator.clipboard.writeText(node.path)
                    }}
                    sx={{ width: 18, height: 18 }}
                  >
                    <CopyLinear size={11} color="currentColor" />
                  </IconButton>
                </Tooltip>
              </Stack>
              {changeFor(changes, node.path)
                ? <ChangedFileView path={node.path} root={root} />
                : (
                  <Box sx={{ maxHeight: 420, overflow: 'auto' }}>
                    <FileContent path={node.path} full fresh />
                  </Box>
                )}
            </Box>
          )}
          {isDir && isOpen && node.children && renderNodes(node.children, depth + 1, root)}
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
    <Stack sx={{ flex: 1, minHeight: 0, minWidth: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, mx: 1, mt: 1, mb: 0.5, flexShrink: 0 }}>
        <Stack
          direction="row"
          sx={{
            alignItems: 'center', gap: 0.75, flex: 1, minWidth: 0, px: 1, height: 28, borderRadius: '7px',
            bgcolor: 'action.hover', color: 'text.disabled', transition: 'background-color 0.12s',
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
        <Tooltip title="Refresh">
          <IconButton size="small" aria-label="Refresh files" onClick={() => setReload((n) => n + 1)} sx={{ width: 24, height: 24 }}>
            <RefreshLinear size={13} color="currentColor" />
          </IconButton>
        </Tooltip>
        {changes.size > 0 && (
          <Typography variant="caption" color="text.secondary" sx={{ flexShrink: 0, fontSize: '0.6875rem' }}>
            {changes.size} changed
          </Typography>
        )}
      </Stack>

      <Box ref={bodyRef} sx={{ px: 1, pb: 1, overflowY: 'auto', flex: 1, minHeight: 0, minWidth: 0 }}>
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
                          onClick={() =>
                            setCollapsedRoots((prev) => {
                              const next = new Set(prev)
                              if (next.has(t.root)) next.delete(t.root)
                              else next.add(t.root)
                              return next
                            })
                          }
                          sx={{ alignItems: 'center', gap: 0.5, px: 0.75, py: 0.5, mt: 0.5, position: 'sticky', top: 0, zIndex: 1, cursor: 'pointer', borderRadius: 1.5, bgcolor: 'background.chrome', '&:hover': { bgcolor: 'action.hover' } }}
                          title={t.root}
                        >
                          <Box sx={{ display: 'flex', color: 'text.disabled', transform: collapsed ? 'none' : 'rotate(90deg)', transition: 'transform 0.15s ease' }}>
                            <ChevronRightLinear size={11} color="currentColor" />
                          </Box>
                          <Typography variant="caption" noWrap sx={{ flex: 1, minWidth: 0, fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.4, fontSize: '0.6875rem', color: 'text.secondary' }}>
                            {t.label}
                          </Typography>
                        </Stack>
                      )}
                      {!collapsed &&
                        (t.error
                          ? hint(`Couldn’t read this folder — ${t.error}`)
                          : t.nodes && t.nodes.length === 0
                            ? hint('Empty folder.')
                            : t.nodes && renderNodes(t.nodes, 0, t.root))}
                    </Box>
                  )
                })}
        {loading && visible && (
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1, p: 1.5 }}>
            <CircularProgress size={12} />
            <Typography variant="caption" color="text.secondary">Refreshing…</Typography>
          </Stack>
        )}
      </Box>
    </Stack>
  )
}
