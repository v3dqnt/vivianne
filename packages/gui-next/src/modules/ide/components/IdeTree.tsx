import { useEffect, useMemo, useState } from 'react'
import { Box, Stack, Typography, CircularProgress, Button, InputBase, useTheme, type Theme } from '../../../ui'
import { ChevronRightLinear, FolderLinear, FolderOpenLinear, DocumentLinear, SearchNormalLinear } from '../../../icons'
import { call } from '../../../ipc'
import { useActiveWorkspace } from '../../dock/useActiveWorkspace'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'
import { useIdeStore } from '../store/ideStore'

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

const TREE_MAX_DEPTH = 8
const TREE_MAX_ENTRIES = 5000

function basename(p: string): string {
  return p.split(/[\\/]/).filter(Boolean).pop() || p
}

/** Same category-tint scheme `WorkspaceTree` uses, kept in sync deliberately
 * — a file reads the same color whether you're browsing it or editing it.
 * Returns a resolved color, not a theme-path string, so it stays correct
 * regardless of how deep the matching palette token sits (`primary.main` is
 * one level, `text.disabled` is nested under `text`). */
function fileTint(theme: Theme, name: string): string {
  const ext = name.slice(name.lastIndexOf('.') + 1).toLowerCase()
  if (['ts', 'tsx', 'js', 'jsx', 'rs', 'py', 'go', 'java', 'rb', 'c', 'cpp', 'h', 'cs', 'swift', 'kt'].includes(ext)) return theme.palette.primary.main
  if (['json', 'toml', 'yaml', 'yml', 'lock', 'ini', 'env', 'conf', 'cfg'].includes(ext)) return theme.palette.warning.main
  if (['md', 'mdx', 'txt', 'rst', 'adoc'].includes(ext)) return theme.palette.success.main
  return theme.palette.text.disabled
}

function filterTree(nodes: TreeNode[], q: string): TreeNode[] {
  const out: TreeNode[] = []
  for (const n of nodes) {
    if (n.type === 'directory') {
      const kids = n.children ? filterTree(n.children, q) : []
      if (kids.length > 0 || n.name.toLowerCase().includes(q)) out.push({ ...n, children: kids })
    } else if (n.name.toLowerCase().includes(q)) {
      out.push(n)
    }
  }
  return out
}

/**
 * File tree for the IDE view.
 *
 * Deliberately its own component rather than reusing `WorkspaceTree`: that
 * one expands a file inline (a preview appears under the row you clicked),
 * which is the right browsing model for a read-only dock panel but wrong
 * here — clicking a file in an IDE should open it as a tab, full-height, next
 * to the tree, not push the rest of the tree down the page.
 */
export function IdeTree() {
  const theme = useTheme()
  const folders = useActiveWorkspace()
  const addExistingFolder = useWorkspaceStore((s) => s.addExistingFolder)
  const foldersKey = folders.join(' ')
  const [trees, setTrees] = useState<RootTree[] | null>(null)
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [query, setQuery] = useState('')
  const activePath = useIdeStore((s) => s.activePath)
  const openFile = useIdeStore((s) => s.openFile)

  useEffect(() => {
    setTrees(null)
    if (folders.length === 0) return
    let cancelled = false
    void Promise.all(
      folders.map((root) =>
        call<TreeNode[]>('cmd_list_dir_tree', { root, maxDepth: TREE_MAX_DEPTH, maxEntries: TREE_MAX_ENTRIES })
          .then((nodes): RootTree => ({ root, label: basename(root), nodes: nodes ?? [], error: null }))
          .catch((e): RootTree => ({ root, label: basename(root), nodes: null, error: String(e) })),
      ),
    ).then((results) => {
      if (!cancelled) setTrees(results)
    })
    return () => {
      cancelled = true
    }
  }, [foldersKey])

  const q = query.trim().toLowerCase()
  const visible = useMemo(() => {
    if (!trees) return null
    if (!q) return trees
    return trees.map((t) => ({ ...t, nodes: t.nodes ? filterTree(t.nodes, q) : null }))
  }, [trees, q])

  const toggleDir = (path: string) =>
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })

  const renderNodes = (nodes: TreeNode[], depth: number): React.ReactNode[] =>
    nodes.map((node) => {
      const isDir = node.type === 'directory'
      const isOpen = isDir ? q.length > 0 || expanded.has(node.path) : node.path === activePath
      return (
        <Box key={node.path}>
          <Stack
            direction="row"
            onClick={() => (isDir ? toggleDir(node.path) : void openFile(node.path))}
            sx={{
              alignItems: 'center',
              gap: 0.5,
              pl: 0.75 + depth * 1.25,
              pr: 0.75,
              py: 0.375,
              cursor: 'pointer',
              borderRadius: '4px',
              bgcolor: isOpen && !isDir ? 'action.selected' : 'transparent',
              '&:hover': { bgcolor: isOpen && !isDir ? 'action.selected' : 'action.hover' },
            }}
          >
            {isDir ? (
              <ChevronRightLinear
                size={11}
                color={theme.palette.text.disabled}
                style={{ transform: isOpen ? 'rotate(90deg)' : 'none', transition: 'transform 0.1s', flexShrink: 0 }}
              />
            ) : (
              <Box sx={{ width: 11, flexShrink: 0 }} />
            )}
            {isDir ? (
              isOpen ? (
                <FolderOpenLinear size={13} color={theme.palette.text.secondary} />
              ) : (
                <FolderLinear size={13} color={theme.palette.text.secondary} />
              )
            ) : (
              <DocumentLinear size={13} color={fileTint(theme, node.name)} />
            )}
            <Typography variant="caption" noWrap sx={{ color: isOpen && !isDir ? 'text.primary' : 'text.secondary' }}>
              {node.name}
            </Typography>
          </Stack>
          {isDir && isOpen && node.children && renderNodes(node.children, depth + 1)}
        </Box>
      )
    })

  if (folders.length === 0) {
    // A real, clickable action — not just a hint. With no folder open, IDE
    // layout is two empty placeholders (this and the editor's "select a
    // file") side by side, which reads as broken to anyone who hasn't
    // already opened a workspace some other way. This is the same picker
    // `WorkspaceSelector`'s "add a folder" row uses.
    return (
      <Stack sx={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 1, p: 2, textAlign: 'center' }}>
        <Typography variant="caption" color="text.secondary">
          No workspace folder is open yet.
        </Typography>
        <Button
          size="small"
          variant="outlined"
          onClick={() => void addExistingFolder()}
          sx={{ textTransform: 'none', fontSize: '0.75rem' }}
        >
          Open a folder
        </Button>
      </Stack>
    )
  }

  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, px: 1, py: 0.75, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <SearchNormalLinear size={12} color="currentColor" />
        <InputBase
          fullWidth
          placeholder="Filter files…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          sx={{ fontSize: '0.75rem' }}
        />
      </Stack>
      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', py: 0.5 }}>
        {visible === null ? (
          <Stack sx={{ alignItems: 'center', py: 3 }}>
            <CircularProgress size={16} />
          </Stack>
        ) : (
          visible.map((t) => (
            <Box key={t.root}>
              {visible.length > 1 && (
                <Typography
                  variant="caption"
                  sx={{ display: 'block', px: 1, py: 0.5, fontWeight: 700, letterSpacing: 0.3, color: 'text.disabled', fontSize: '0.625rem' }}
                >
                  {t.label.toUpperCase()}
                </Typography>
              )}
              {t.error ? (
                <Typography variant="caption" color="error.main" sx={{ display: 'block', px: 1 }}>
                  {t.error}
                </Typography>
              ) : (
                t.nodes && renderNodes(t.nodes, 0)
              )}
            </Box>
          ))
        )}
      </Box>
    </Stack>
  )
}
