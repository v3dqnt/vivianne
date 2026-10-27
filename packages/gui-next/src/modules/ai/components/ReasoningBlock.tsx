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
    <Box sx={{ maxWidth: '100%', mb: '4px' }}>
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
        // A soft card, not a rail: the reasoning reads as an aside, set a step
        // quieter than the reply (smaller, muted, on a faint tint).
        <Box
          sx={{
            mt: '4px',
            px: '12px',
            py: '8px',
            borderRadius: '8px',
            bgcolor: 'action.hover',
            color: 'text.secondary',
            fontSize: '12.5px',
            lineHeight: '20px',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            maxHeight: streaming ? 'none' : 360,
            overflowY: 'auto',
          }}
        >
          {throttledText}
        </Box>
      )}
    </Box>
  )
})
