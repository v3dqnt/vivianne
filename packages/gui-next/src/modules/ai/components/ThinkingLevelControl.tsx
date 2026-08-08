import { useState } from 'react'
import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import Popover from '@mui/material/Popover'
import Slider from '@mui/material/Slider'
import Typography from '@mui/material/Typography'
import { Setting2Linear } from 'mx-icons'
import { THINKING_LEVELS } from '../lib/types'
import { useModelStore } from '../store/modelStore'

const LABELS: Record<(typeof THINKING_LEVELS)[number], string> = {
  off: 'Off',
  minimal: 'Minimal',
  low: 'Low',
  medium: 'Medium',
  high: 'High',
  xhigh: 'X-High',
}

export function ThinkingLevelControl() {
  const thinkingLevel = useModelStore((s) => s.thinkingLevel)
  const setThinkingLevel = useModelStore((s) => s.setThinkingLevel)
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)

  const index = THINKING_LEVELS.indexOf(thinkingLevel)

  return (
    <>
      <Button
        size="small"
        onClick={(e) => setAnchor(e.currentTarget)}
        endIcon={<Setting2Linear size={12} color="currentColor" />}
        sx={{
          textTransform: 'none',
          fontSize: '0.8125rem',
          fontWeight: 500,
          color: 'text.secondary',
          minWidth: 0,
          borderRadius: 999,
          px: 1,
          py: 0.5,
          '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
        }}
      >
        {LABELS[thinkingLevel]}
      </Button>
      <Popover
        open={!!anchor}
        anchorEl={anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: 'top', horizontal: 'left' }}
        transformOrigin={{ vertical: 'bottom', horizontal: 'left' }}
      >
        <Box sx={{ px: 2.5, pt: 3, pb: 1.5, width: 220 }}>
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 1.5 }}>
            Thinking level
          </Typography>
          <Slider
            size="small"
            min={0}
            max={THINKING_LEVELS.length - 1}
            step={1}
            value={index === -1 ? 2 : index}
            marks
            onChange={(_, value) => void setThinkingLevel(THINKING_LEVELS[value as number])}
            valueLabelDisplay="auto"
            valueLabelFormat={(v) => LABELS[THINKING_LEVELS[v]]}
          />
        </Box>
      </Popover>
    </>
  )
}
