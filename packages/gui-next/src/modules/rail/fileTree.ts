/**
 * The folder tree the Files tab shows: folders first then files, each by name, with the noise directories hidden.
 * Pure, so the ordering and hiding can be tested without a backend.
 */
export interface TreeNode {
  name: string
  path: string
  type?: string
  children?: TreeNode[]
}

/** Folders nobody browses by hand. */
const HIDDEN = new Set(['.git', 'node_modules', 'target', 'dist', '.next', '.turbo', '.venv', '__pycache__', '.DS_Store'])

export const isDir = (n: TreeNode) => n.type === 'directory'

export function visibleChildren(nodes: readonly TreeNode[] | undefined): TreeNode[] {
  return (nodes ?? [])
    .filter((n) => !HIDDEN.has(n.name))
    .sort((a, b) => Number(isDir(b)) - Number(isDir(a)) || a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' }))
}

/** Every node whose name contains `query`, with the folders above it, so a filtered tree still reads as a tree. */
export function filterTree(nodes: readonly TreeNode[], query: string): TreeNode[] {
  const q = query.trim().toLowerCase()
  if (!q) return [...nodes]
  const out: TreeNode[] = []
  for (const n of nodes) {
    if (HIDDEN.has(n.name)) continue
    const kids = isDir(n) ? filterTree(n.children ?? [], q) : []
    if (n.name.toLowerCase().includes(q) || kids.length > 0) out.push(isDir(n) ? { ...n, children: kids.length > 0 ? kids : n.children } : n)
  }
  return out
}
