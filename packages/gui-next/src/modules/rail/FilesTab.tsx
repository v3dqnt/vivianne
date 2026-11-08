import { useEffect, useMemo, useState } from 'react'
import { Box, ButtonBase, InputBase, Stack, Typography } from '../../ui'
import { ChevronRightLinear, SearchNormalLinear } from '../../icons'
import { FileTypeIcon } from '../../components/FileTypeIcon'
import { call } from '../../ipc'
import { useWorkspaceStore } from '../ai/store/workspaceStore'
import { useContextMenu } from '../../components/ContextMenu'
import { filterTree, isDir, visibleChildren, type TreeNode } from './fileTree'
import { useRail } from './railStore'

const MONO = '"Geist Mono", ui-monospace, monospace'

/** One row of the tree. Folders fold; files open in Preview; both have the file right-click menu. */
function Row({ node, depth, open, onToggle, active }: { node: TreeNode; depth: number; open: boolean; onToggle: () => void; active: boolean }) {
  const preview = useRail((s) => s.preview)
  const dir = isDir(node)
  const { onContextMenu, menu } = useContextMenu('file', { path: node.path })
  return (
    <>
      {menu}
      <ButtonBase
        onClick={() => (dir ? onToggle() : preview(node.path))}
        onContextMenu={onContextMenu}
        title={node.path}
        sx={{
          width: '100%',
          justifyContent: 'flex-start',
          gap: '6px',
          height: 28,
          pl: `${8 + depth * 14}px`,
          pr: '8px',
          borderRadius: '6px',
          fontSize: '0.8125rem',
          color: active ? 'text.primary' : 'text.secondary',
          bgcolor: active ? 'action.selected' : 'transparent',
          '&:hover': { bgcolor: active ? 'action.selected' : 'action.hover', color: 'text.primary' },
        }}
      >
        <Box sx={{ width: 12, display: 'flex', flexShrink: 0, opacity: dir ? 0.7 : 0, transform: open ? 'rotate(90deg)' : 'none', transition: 'transform 0.15s ease' }}>
          <ChevronRightLinear size={12} color="currentColor" />
        </Box>
        <FileTypeIcon name={node.name} dir={dir} open={open} size={16} />
        <Box component="span" sx={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {node.name}
        </Box>
      </ButtonBase>
    </>
  )
}

function Branch({ nodes, depth, expanded, toggle, forceOpen }: { nodes: TreeNode[]; depth: number; expanded: Set<string>; toggle: (p: string) => void; forceOpen: boolean }) {
  const current = useRail((s) => (s.tab === 'preview' ? s.previewPath : null))
  return (
    <>
      {visibleChildren(nodes).map((n) => {
        const open = isDir(n) && (forceOpen || expanded.has(n.path))
        return (
          <Box key={n.path}>
            <Row node={n} depth={depth} open={open} onToggle={() => toggle(n.path)} active={n.path === current} />
            {open && <Branch nodes={n.children ?? []} depth={depth + 1} expanded={expanded} toggle={toggle} forceOpen={forceOpen} />}
          </Box>
        )
      })}
    </>
  )
}

const EXPANDED_KEY = 'viv.rail.expanded'

/** The project's folder as a tree. Which folders are open is kept per device, so switching tabs or threads keeps your place. */
export function FilesTab() {
  const root = useWorkspaceStore((s) => s.folders[0] ?? null)
  const [tree, setTree] = useState<TreeNode[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [query, setQuery] = useState('')
  const [expanded, setExpanded] = useState<Set<string>>(() => {
    try {
      return new Set(JSON.parse(localStorage.getItem(EXPANDED_KEY) ?? '[]') as string[])
    } catch {
      return new Set()
    }
  })

  useEffect(() => {
    if (!root) return
    setTree(null)
    setError(null)
    call<TreeNode[]>('cmd_list_dir_tree', { root, maxDepth: 8, maxEntries: 5000 })
      .then((t) => setTree(t ?? []))
      .catch((e) => setError(String(e instanceof Error ? e.message : e)))
  }, [root])

  const toggle = (p: string) =>
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(p)) next.delete(p)
      else next.add(p)
      try {
        localStorage.setItem(EXPANDED_KEY, JSON.stringify([...next]))
      } catch {
        /* not remembered */
      }
      return next
    })

  const shown = useMemo(() => (tree ? filterTree(tree, query) : []), [tree, query])

  if (!root) return <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', px: '8px' }}>This thread has no folder.</Typography>
  return (
    <Stack sx={{ minHeight: 0, flex: 1, gap: '8px' }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', height: 36, px: '12px', borderRadius: '8px', bgcolor: 'action.hover', color: 'text.disabled', flexShrink: 0 }}>
        <SearchNormalLinear size={14} color="currentColor" />
        <InputBase value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Filter files" sx={{ flex: 1, fontSize: '0.8125rem', color: 'text.primary' }} />
      </Stack>
      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', mx: '-4px', px: '4px' }}>
        {error ? (
          <Typography sx={{ fontSize: '0.8125rem', color: 'error.main', px: '8px' }}>{error}</Typography>
        ) : tree === null ? (
          <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', px: '8px' }}>Reading the folder…</Typography>
        ) : shown.length === 0 ? (
          <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', px: '8px' }}>{query ? 'No files match.' : 'The folder is empty.'}</Typography>
        ) : (
          <Branch nodes={shown} depth={0} expanded={expanded} toggle={toggle} forceOpen={!!query.trim()} />
        )}
      </Box>
      <Typography noWrap title={root} sx={{ fontSize: '0.6875rem', color: 'text.disabled', fontFamily: MONO, px: '8px', flexShrink: 0 }}>
        {root}
      </Typography>
    </Stack>
  )
}
