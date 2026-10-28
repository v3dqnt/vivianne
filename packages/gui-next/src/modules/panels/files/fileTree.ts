/** Mirrors the shape `cmd_list_dir_tree` returns. */
export interface TreeNode {
  name: string
  path: string
  type: string
  children?: TreeNode[]
}

/** Mirrors `GitFileChange` from the Rust side. */
export interface GitFileChange {
  path: string
  added: number
  removed: number
  status: string
  staged: boolean
}

/** What a row's badge shows. */
export interface FileChange {
  additions: number
  deletions: number
  status: string
}

export function basename(p: string): string {
  return p.split(/[\\/]/).filter(Boolean).pop() || p
}

/** One spelling for comparing paths that came from different places. */
export function normalizePath(p: string): string {
  return p.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
}

/** Keeps a node when it (or anything under it) matches — so filtering shows
 * matching files still sitting in their real folder structure. */
export function filterTree(nodes: TreeNode[], q: string): TreeNode[] {
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

/** Git reports paths relative to the repository root, which is usually the
 * workspace root but may be above it, so a change is matched by suffix rather
 * than by joining the two. */
export function mergeChanges(staged: GitFileChange[], unstaged: GitFileChange[]): Map<string, FileChange> {
  const out = new Map<string, FileChange>()
  for (const c of [...staged, ...unstaged]) {
    const key = normalizePath(c.path)
    const prev = out.get(key)
    out.set(key, {
      additions: (prev?.additions ?? 0) + c.added,
      deletions: (prev?.deletions ?? 0) + c.removed,
      status: c.status || prev?.status || '',
    })
  }
  return out
}

export function changeFor(changes: Map<string, FileChange>, absolutePath: string): FileChange | undefined {
  const p = normalizePath(absolutePath)
  for (const [rel, change] of changes) if (p === rel || p.endsWith(`/${rel}`)) return change
  return undefined
}

/** The folder paths between `root` and `filePath`, in the separator and case
 * the tree itself uses, so they can be dropped straight into an expanded set. */
export function ancestorsOf(filePath: string, root: string): string[] {
  const sep = root.includes('\\') ? '\\' : '/'
  const f = filePath.replace(/[\\/]/g, sep)
  const r = root.replace(/[\\/]/g, sep).replace(/[\\/]+$/, '')
  if (f.toLowerCase().slice(0, r.length) !== r.toLowerCase()) return []
  if (f.length <= r.length + 1) return []
  const parts = f.slice(r.length + 1).split(sep)
  parts.pop()
  const out: string[] = []
  let cur = r
  for (const part of parts) {
    cur = `${cur}${sep}${part}`
    out.push(cur)
  }
  return out
}

const VISUAL = new Set([
  'png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'ico', 'bmp',
  'mp4', 'webm', 'mov', 'mp3', 'wav', 'ogg', 'm4a', 'flac',
  'pdf', 'docx', 'doc', 'xlsx', 'xls', 'xlsm', 'ods', 'csv', 'pptx', 'ppt',
])

/** Files the Browser panel can show rather than the code viewer. */
export function isVisualFile(path: string): boolean {
  return VISUAL.has(path.slice(path.lastIndexOf('.') + 1).toLowerCase())
}
