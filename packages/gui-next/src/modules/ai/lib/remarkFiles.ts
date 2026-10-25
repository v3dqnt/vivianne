import { isFilePath, splitFilePaths } from './filePaths'

interface Node {
  type: string
  value?: string
  children?: Node[]
  data?: Record<string, unknown>
}

const SKIP = new Set(['link', 'linkReference', 'code', 'math', 'inlineMath', 'html', 'definition', 'image'])

const ref = (path: string): Node => ({
  type: 'fileRef',
  data: { hName: 'span', hProperties: { dataFile: path } },
  children: [{ type: 'text', value: path }],
})

/** Turns file names and paths in a message into `<span data-file>` nodes
 * (inline code that is a path, and paths inside running text) so the renderer
 * can make them links. Leaves links, fences and maths alone. */
export function remarkFiles() {
  return (tree: Node) => {
    const walk = (node: Node) => {
      if (!node.children || SKIP.has(node.type)) return
      const next: Node[] = []
      for (const child of node.children) {
        if (child.type === 'inlineCode' && child.value && isFilePath(child.value)) {
          next.push(ref(child.value.trim()))
        } else if (child.type === 'text' && child.value) {
          const pieces = splitFilePaths(child.value)
          if (pieces.length === 1 && typeof pieces[0] === 'string') next.push(child)
          else for (const p of pieces) next.push(typeof p === 'string' ? { type: 'text', value: p } : ref(p.path))
        } else {
          walk(child)
          next.push(child)
        }
      }
      node.children = next
    }
    walk(tree)
  }
}
