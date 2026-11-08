import { Box, Orb } from '../../../ui'
import { ThinkingStates } from '../../../components/ThinkingStates'

/** What the line says while the model works, one after another. */
export const THINKING_STATES = ['Thinking…', 'Reasoning…', 'Working it through…', 'Pondering…']

export function ThinkingIndicator() {
  return (
    <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, py: 0.25, fontSize: '0.875rem' }}>
      <Orb state="working" label="Working" />
      <ThinkingStates states={THINKING_STATES} />
    </Box>
  )
}
