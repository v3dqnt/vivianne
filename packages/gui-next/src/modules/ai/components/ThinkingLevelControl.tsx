import { useState } from 'react'
import { Box, Button, Popover, Slider, Stack, Typography, alpha } from '../../../ui'
import { THINKING_LEVELS, type ThinkingLevel } from '../lib/types'
import { useModelStore } from '../store/modelStore'

/**
 * Copy for each stop on the slider.
 *
 * `note` is for a level whose behaviour does not match what its name implies.
 * Right now that is X-High: `ai::api::simple_options::clamp_reasoning` folds it
 * onto High for every provider, so dragging to the end changes nothing. Saying
 * so is the difference between a control that reports what the app does and
 * one that implies a tier that is not there.
 */
const LEVELS: Record<ThinkingLevel, { label: string; hint: string; note?: string }> = {
  off: { label: 'Off', hint: 'Answers straight away. Lookups and small edits.' },
  minimal: { label: 'Minimal', hint: 'A quick pass before answering.' },
  low: { label: 'Low', hint: 'Light reasoning for contained tasks.' },
  medium: { label: 'Medium', hint: 'Balanced. A good default for most work.' },
  high: { label: 'High', hint: 'Hard debugging and multi-step planning.' },
  xhigh: {
    label: 'X-High',
    hint: 'Hard debugging and multi-step planning.',
    note: 'Currently resolves to High — no provider exposes a tier above it yet.',
  },
}

const MAX_INDEX = THINKING_LEVELS.length - 1

export function ThinkingLevelControl() {
  const thinkingLevel = useModelStore((s) => s.thinkingLevel)
  const setThinkingLevel = useModelStore((s) => s.setThinkingLevel)
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)

  const index = THINKING_LEVELS.indexOf(thinkingLevel)
  // A stored value outside the known set used to render `undefined` on the
  // trigger button while the slider silently sat on "low".
  const safeIndex = index === -1 ? THINKING_LEVELS.indexOf('medium') : index
  const current = LEVELS[THINKING_LEVELS[safeIndex]]

  return (
    <>
      <Button
        size="small"
        className="cq-collapsible"
        onClick={(e) => setAnchor(e.currentTarget)}
        title={`Thinking: ${current.label}`}
        aria-label={`Thinking level: ${current.label}`}
        sx={{
          textTransform: 'none',
          fontSize: '0.8125rem',
          fontWeight: 500,
          color: 'text.secondary',
          minWidth: 0,
          borderRadius: '8px',
          px: 1,
          py: 0.375,
          '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
        }}
      >
        {/* Always shown: the level in use must be readable at any width. */}
        <Box component="span">{current.label}</Box>
      </Button>
      <Popover
        open={!!anchor}
        anchorEl={anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: 'top', horizontal: 'left' }}
        transformOrigin={{ vertical: 'bottom', horizontal: 'left' }}
        slotProps={{ paper: { sx: { width: 268, maxWidth: '92vw', borderRadius: '14px' } } }}
      >
        <Box sx={{ px: 2, pt: 1.75, pb: 1.75 }}>
          {/* The chosen level is stated as text rather than left to a tooltip
              that only appears mid-drag — previously the one thing you needed
              in order to choose was the one thing you could not see. */}
          <Stack direction="row" sx={{ alignItems: 'baseline', justifyContent: 'space-between', mb: 0.25 }}>
            <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 600, letterSpacing: 0.3 }}>
              Thinking level
            </Typography>
            <Typography variant="body2" sx={{ fontWeight: 700, color: 'primary.main', lineHeight: 1 }}>
              {current.label}
            </Typography>
          </Stack>

          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', opacity: 0.85, minHeight: 32, lineHeight: 1.45 }}>
            {current.hint}
          </Typography>

          <Box sx={{ px: 0.5 }}>
            <Slider
              aria-label="Thinking level"
              min={0}
              max={MAX_INDEX}
              step={1}
              marks
              value={safeIndex}
              onChange={(_, value) => void setThinkingLevel(THINKING_LEVELS[value as number])}
              sx={{
                mt: 1,
                mb: 0,
                height: 6,
                // The filled half reads as the "amount" of thinking, so it
                // keeps full weight while the rest of the rail recedes.
                '& .ui-Slider-rail': { opacity: 1, bgcolor: 'action.selected' },
                '& .ui-Slider-track': { border: 'none' },
                '& .ui-Slider-mark': {
                  width: 3,
                  height: 3,
                  borderRadius: '50%',
                  bgcolor: 'background.paper',
                  opacity: 0.9,
                  // Endpoint marks sit under the thumb's travel limits and
                  // only add visual noise there.
                  '&[data-index="0"], &[data-index="5"]': { opacity: 0 },
                },
                '& .ui-Slider-markActive': { bgcolor: 'background.paper', opacity: 0.55 },
                '& .ui-Slider-thumb': {
                  width: 16,
                  height: 16,
                  bgcolor: 'background.paper',
                  border: 3,
                  borderColor: 'primary.main',
                  boxShadow: (t) => `0 1px 4px ${alpha(t.palette.common.black, t.palette.mode === 'dark' ? 0.5 : 0.2)}`,
                  transition: 'box-shadow 0.15s, width 0.15s, height 0.15s',
                  '&:hover, &:focus-visible': {
                    boxShadow: (t) => `0 0 0 7px ${alpha(t.palette.primary.main, 0.16)}`,
                  },
                  '&:active': {
                    width: 18,
                    height: 18,
                    boxShadow: (t) => `0 0 0 10px ${alpha(t.palette.primary.main, 0.2)}`,
                  },
                },
              }}
            />
          </Box>

          {/* Endpoint anchors: six labels will not fit across 236px, but the
              two that define the scale do, and the header names the rest. */}
          <Stack direction="row" sx={{ justifyContent: 'space-between', mt: -0.25 }}>
            <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem', opacity: 0.7 }}>
              {LEVELS.off.label}
            </Typography>
            <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem', opacity: 0.7 }}>
              {LEVELS.xhigh.label}
            </Typography>
          </Stack>

          {current.note && (
            <Typography variant="caption" sx={{ display: 'block', mt: 1, lineHeight: 1.45, color: 'warning.main', opacity: 0.95 }}>
              {current.note}
            </Typography>
          )}
        </Box>
      </Popover>
    </>
  )
}
