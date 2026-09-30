import { memo } from 'react'
import { Box } from '../../../ui'
import { diffLines } from '../lib/diff'

const LINE_STYLE = {
  add: { bgcolor: 'rgba(46,160,67,0.15)', color: '#3fb950' },
  remove: { bgcolor: 'rgba(248,81,73,0.15)', color: '#f85149' },
  context: { bgcolor: 'transparent', color: 'text.secondary' },
} as const

/** Renders a unified line diff between two full-file (or snippet) contents.
 * `before === null` means a new file (all-green added); `after === null`
 * means a deleted file (all-red removed). Memoized — `diffLines` runs an
 * O(N*M) table for its exact path, so it shouldn't re-run on unrelated
 * parent re-renders when `before`/`after` haven't changed. */
export const DiffView = memo(function DiffView({ before, after }: { before: string | null; after: string | null }) {
  const lines = diffLines(before ?? '', after ?? '')

  return (
    <Box
      className="select-text"
      sx={{
        fontFamily: '"Geist Mono", ui-monospace, monospace',
        fontSize: '0.75rem',
        lineHeight: 1.6,
        maxHeight: 360,
        overflowY: 'auto',
        borderTop: 1,
        borderColor: 'divider',
      }}
    >
      {lines.map((line, i) => (
        <Box
          key={i}
          sx={{
            px: 1.5,
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            ...LINE_STYLE[line.kind],
          }}
        >
          <Box component="span" sx={{ display: 'inline-block', width: 14, opacity: 0.6, userSelect: 'none' }}>
            {line.kind === 'add' ? '+' : line.kind === 'remove' ? '-' : ' '}
          </Box>
          {line.text || ' '}
        </Box>
      ))}
    </Box>
  )
})
