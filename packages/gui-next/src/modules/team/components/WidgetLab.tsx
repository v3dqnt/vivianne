import { useState } from 'react'
import { Box, ButtonBase, Stack, Typography } from '../../../ui'
import { WIDGET_LAB, type WidgetState } from '../lib/widgetLab'
import { describeApproval } from '../lib/widgets'
import { ApprovalWidget } from './ApprovalWidget'

const LAB_COLOR = '#7c9cff'

/**
 * The Widget Lab: a debugger agent whose chat is every card an agent can
 * show, in every state and edge case, laid out as its messenger would. The
 * pending ones answer for real (locally), so the stamped states can be seen
 * arriving too. Dev builds only.
 */
export function WidgetLab() {
  const [states, setStates] = useState<Record<number, WidgetState>>({})
  const reset = () => setStates({})
  return (
    <Stack sx={{ flex: 1, minHeight: 0, bgcolor: 'background.default' }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: '12px', px: '20px', py: '14px' }}>
        <Box sx={{ width: 44, height: 44, borderRadius: '12px', display: 'grid', placeItems: 'center', bgcolor: 'action.hover', fontSize: '1.25rem' }}>🧪</Box>
        <Stack sx={{ flex: 1, minWidth: 0 }}>
          <Typography sx={{ fontSize: '1rem', fontWeight: 600 }}>Widget Lab</Typography>
          <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>Every card an agent can show, in every state</Typography>
        </Stack>
        <ButtonBase onClick={reset} sx={{ height: 32, px: '12px', borderRadius: '999px', fontSize: '0.75rem', bgcolor: 'action.hover' }}>
          Reset
        </ButtonBase>
      </Stack>
      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: '20px', pb: '24px' }}>
        <Stack sx={{ maxWidth: 760, mx: 'auto', gap: '20px' }}>
          {WIDGET_LAB.map((card, i) => {
            const state = states[i] ?? card.state
            return (
              <Stack key={i} sx={{ gap: '6px' }}>
                <Typography sx={{ fontSize: '0.6875rem', fontWeight: 500, color: 'text.disabled', pl: '44px' }}>
                  {card.label} · {card.state}
                </Typography>
                <Stack direction="row" sx={{ alignItems: 'flex-end', gap: '8px' }}>
                  <Box sx={{ width: 36, height: 36, borderRadius: '50%', bgcolor: 'action.hover', flexShrink: 0 }} />
                  <ApprovalWidget
                    widget={describeApproval(card.approval, 'Lab')}
                    state={state}
                    accent={LAB_COLOR}
                    onAnswer={(yes) => setStates((s) => ({ ...s, [i]: yes ? 'approved' : 'denied' }))}
                  />
                </Stack>
              </Stack>
            )
          })}
        </Stack>
      </Box>
    </Stack>
  )
}
