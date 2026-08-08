import { useState } from 'react'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import { ArrowDownLinear } from 'mx-icons'

/** Collapsible model-reasoning text (`thinking` content blocks) — muted and auto-expanded while actively streaming so the reasoning is visible live, not just after the fact. */
export function ReasoningBlock({ text, streaming }: { text: string; streaming?: boolean }) {
  // `null` means "no manual override yet" — the panel then just follows
  // `streaming` (open while thinking, collapses once the reply starts).
  // Once the user clicks it themselves, that choice sticks.
  const [manualOpen, setManualOpen] = useState<boolean | null>(null)
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
          {text}
        </Typography>
      )}
    </Box>
  )
}
