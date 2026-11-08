/**
 * Words that have just arrived fade in (the Streaming-text pattern from Transitions.dev).
 *
 * The reply is parsed again on every update, so each word is wrapped in its own `<span class="viv-w">`; a span that is new to the page
 * plays the fade (opacity and a hair of blur) once, and one that was already there stays put. Only text near the end of the reply is
 * wrapped: words further back finished fading long ago, and wrapping a long reply whole would cost thousands of nodes per update.
 */

interface Node {
  type: string
  value?: string
  children?: Node[]
  data?: Record<string, unknown>
  position?: { start?: { offset?: number } }
}

/** Nodes whose text must stay as it is. */
const SKIP = new Set(['code', 'math', 'inlineMath', 'html', 'inlineCode', 'definition', 'image', 'fileRef'])

/** How much of the end of the reply is still animating, in characters. */
export const TAIL_CHARS = 400

const word = (value: string): Node => ({
  type: 'streamWord',
  data: { hName: 'span', hProperties: { className: ['viv-w'] } },
  children: [{ type: 'text', value }],
})

/** `value` as alternating words (wrapped) and the whitespace between them (plain text). */
export function wrapWords(value: string): Node[] {
  return value
    .split(/(\s+)/)
    .filter((p) => p !== '')
    .map((p) => (/\S/.test(p) ? word(p) : { type: 'text', value: p }))
}

/** A remark plugin: wraps the words of text that starts at or after character `from` of the source. */
export function remarkStreamWords({ from }: { from: number }) {
  return (tree: Node) => {
    const walk = (node: Node, offset: number) => {
      if (!node.children || SKIP.has(node.type)) return
      const next: Node[] = []
      for (const child of node.children) {
        const at = child.position?.start?.offset ?? offset
        if (child.type === 'text' && child.value && at >= from) next.push(...wrapWords(child.value))
        else {
          walk(child, at)
          next.push(child)
        }
      }
      node.children = next
    }
    walk(tree, 0)
  }
}
