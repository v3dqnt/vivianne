import { useEffect, useRef } from 'react'
import { Box, Stack, Typography, CircularProgress, useTheme } from '../../../ui'
import { TickCircleLinear, CloseCircleLinear } from '../../../icons'
import type { InstantStep } from '../store/instantStore'

/**
 * The live list of what Viv Instant actually did.
 *
 * This is not decoration. Viv Instant runs unattended with no per-step
 * confirmation, so this log is the user's only view into a sequence of real
 * clicks and keystrokes happening on their machine — it is what makes "full
 * auto" observable rather than opaque, and what they read when something went
 * somewhere unexpected.
 */
export function InstantStepLog({ steps }: { steps: InstantStep[] }) {
  const theme = useTheme()
  const endRef = useRef<HTMLDivElement | null>(null)

  // Follow the newest step. The interesting one is always the last.
  useEffect(() => {
    endRef.current?.scrollIntoView({ block: 'end', behavior: 'smooth' })
  }, [steps.length])

  if (steps.length === 0) return null

  return (
    <Stack sx={{ gap: 0.375, overflowY: 'auto', flex: 1, minHeight: 0, pr: 0.5 }}>
      {steps.map((step) => (
        <Stack key={step.index} direction="row" sx={{ alignItems: 'flex-start', gap: 0.875 }}>
          <Box sx={{ width: 14, display: 'flex', justifyContent: 'center', pt: '3px', flexShrink: 0 }}>
            {step.status === 'running' ? (
              <CircularProgress size={10} thickness={6} />
            ) : step.status === 'ok' ? (
              <TickCircleLinear size={12} color={theme.palette.success.main} />
            ) : (
              <CloseCircleLinear size={12} color={theme.palette.error.main} />
            )}
          </Box>
          <Box sx={{ minWidth: 0 }}>
            <Typography
              variant="caption"
              sx={{
                display: 'block',
                lineHeight: 1.45,
                color: step.status === 'failed' ? 'error.main' : 'text.primary',
              }}
            >
              {step.summary}
            </Typography>
            {/* Only failures get their detail expanded: on success it is noise,
                but on a failure it is the whole reason you are reading this. */}
            {step.status === 'failed' && step.detail && (
              <Typography
                variant="caption"
                sx={{
                  display: 'block',
                  color: 'text.secondary',
                  fontFamily: '"Geist Mono", ui-monospace, monospace',
                  fontSize: '0.625rem',
                  lineHeight: 1.4,
                  wordBreak: 'break-word',
                }}
              >
                {step.detail}
              </Typography>
            )}
          </Box>
        </Stack>
      ))}
      <div ref={endRef} />
    </Stack>
  )
}
