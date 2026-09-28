import { memo, useState } from 'react'
import { Box, Typography } from '../../../ui'
import { ArrowDownLinear } from '../../../icons'
import { useThrottledValue } from '../../../lib/useThrottledValue'

/** Collapsible model-reasoning text (`thinking` content blocks) — muted and auto-expanded while actively streaming so the reasoning is visible live, not just after the fact. */
export const ReasoningBlock = memo(function ReasoningBlock({ text, streaming }: { text: string; streaming?: boolean }) {
  // `null` means "no manual override yet" — the panel then just follows
  // `streaming` (open while thinking, collapses once the reply starts).
  // Once the user clicks it themselves, that choice sticks.
  const [manualOpen, setManualOpen] = useState<boolean | null>(null)
  // Thinking streams token-by-token into one big pre-wrap text node — a full
  // node swap per token. The same leading+trailing throttle Markdown uses for
  // its parse work keeps DOM text updates to ~10/sec while staying live;
  // the trailing edge guarantees the final text still lands.
  const throttledText = useThrottledValue(text, 100)
  if (!text) return null
  const open = manualOpen ?? !!streaming

  return (
    <Box sx={{ maxWidth: '80%', mb: 0.5 }}>
      <Box
        onClick={() => setManualOpen(!open)}
        sx={{
          display: 'flex',
          alignItems: 'center',
          gap: 0.75,
          cursor: 'pointer',
          color: 'text.secondary',
          '&:hover': { color: 'text.primary' },
        }}
      >
        <Typography variant="caption">{streaming ? 'Thinking…' : 'Reasoning'}</Typography>
        <Box sx={{ display: 'flex', transform: open ? 'rotate(180deg)' : 'none', transition: 'transform 0.15s' }}>
          <ArrowDownLinear size={12} color="currentColor" />
        </Box>
      </Box>
      {open && (
        <Typography
          variant="caption"
          sx={{
            display: 'block',
            mt: 0.5,
            pl: 1.5,
            borderLeft: 2,
            borderColor: 'divider',
            color: 'text.secondary',
            whiteSpace: 'pre-wrap',
          }}
        >
          {throttledText}
        </Typography>
      )}
    </Box>
  )
})
