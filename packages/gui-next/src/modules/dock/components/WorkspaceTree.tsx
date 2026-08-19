import { useEffect, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import Dialog from '@mui/material/Dialog'
import Tooltip from '@mui/material/Tooltip'
import { FolderLinear, DocumentLinear, ArrowRightLinear, ArrowDownLinear, MonitorLinear, CloseCircleLinear } from '../../../icons'
import { call } from '../../../ipc'
import { useActiveWorkspaceRoot } from '../useActiveWorkspace'
import { FileContent } from './FilePreview'

interface TreeNode {
  name: string
  path: string
  type: string
  children?: TreeNode[]
}

/** Workspace file tree — walks the primary workspace folder via
 * `cmd_list_dir_tree` (depth-limited server-side, so no manual recursion cap
 * here), with lazy per-directory expansion.
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
  const root = useActiveWorkspaceRoot()
  const [tree, setTree] = useState<TreeNode[] | null>(null)
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [openFile, setOpenFile] = useState<string | null>(null)
  const [fullscreen, setFullscreen] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    setTree(null)
    setExpanded(new Set())
    setOpenFile(null)
    if (!root) return
    setLoading(true)
    void call<TreeNode[]>('cmd_list_dir_tree', { root, maxDepth: 6, maxEntries: 1200 }).then((t) => {
      setTree(t ?? [])
      setLoading(false)
    })
  }, [root])

  const toggleDir = (path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(path)) next.delete(path)
      else next.add(path)
      return next
    })
  }

  const renderNodes = (nodes: TreeNode[], depth: number): React.ReactNode[] =>
    nodes.map((node) => {
      const isDir = node.type === 'directory'
      const isOpen = isDir ? expanded.has(node.path) : openFile === node.path
      return (
        <Box key={node.path}>
          <Stack
            direction="row"
            onClick={() => (isDir ? toggleDir(node.path) : setOpenFile(isOpen ? null : node.path))}
            sx={{
              alignItems: 'center',
              gap: 0.5,
              py: 0.375,
              pl: 1 + depth * 1.25,
              pr: 0.5,
              cursor: 'pointer',
              borderRadius: 1,
              transition: 'background-color 0.12s',
              bgcolor: !isDir && isOpen ? 'action.selected' : 'transparent',
              '&:hover': { bgcolor: !isDir && isOpen ? 'action.selected' : 'action.hover' },
              '&:hover .file-expand': { opacity: 1 },
            }}
            title={node.path}
          >
            {isDir ? (
              isOpen ? <ArrowDownLinear size={12} color="currentColor" /> : <ArrowRightLinear size={12} color="currentColor" />
            ) : (
              <Box sx={{ width: 12, flexShrink: 0 }} />
            )}
            {isDir ? <FolderLinear size={13} color="currentColor" /> : <DocumentLinear size={13} color="currentColor" />}
            <Typography variant="caption" noWrap sx={{ flex: 1, minWidth: 0, fontSize: '0.6875rem', color: isDir ? 'text.primary' : 'text.secondary' }}>
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

          {!isDir && isOpen && (
            <Box sx={{ ml: 1 + depth * 1.25, mr: 0.5, mb: 0.75, maxHeight: 320, overflow: 'auto', borderLeft: 2, borderColor: 'primary.main', pl: 1 }}>
              <FileContent path={node.path} />
            </Box>
          )}
          {isDir && isOpen && node.children && renderNodes(node.children, depth + 1)}
        </Box>
      )
    })

  return (
    <>
      <Box sx={{ px: 1, pt: 1, overflowY: 'auto', flex: 1, minHeight: 0, minWidth: 0 }}>
        {!root ? (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 1.5 }}>
            No workspace folder set — add one from the composer's folder picker.
          </Typography>
        ) : loading && !tree ? (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 1.5 }}>
            Loading…
          </Typography>
        ) : tree && tree.length === 0 ? (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', p: 1.5 }}>
            Empty folder.
          </Typography>
        ) : (
          tree && renderNodes(tree, 0)
        )}
      </Box>

      <Dialog open={!!fullscreen} onClose={() => setFullscreen(null)} fullScreen>
        <Stack sx={{ height: '100%', minHeight: 0 }}>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 2, py: 1.25, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
            <DocumentLinear size={14} color="currentColor" />
            <Typography variant="body2" sx={{ flex: 1, minWidth: 0, fontFamily: '"JetBrains Mono", ui-monospace, monospace' }} noWrap title={fullscreen ?? ''}>
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
