import { useEffect, useState } from 'react'
import { Box, Button, Stack, Typography } from '../ui'
import { ThinkingStates } from '../components/ThinkingStates'
import { THINKING_STATES } from '../modules/ai/components/ThinkingIndicator'
import { Markdown } from '../modules/ai/components/Markdown'

const REPLY = `The sea does not hurry, and it does not stop. **Every wave** is the same water going somewhere else.

Near the shore it slows, folds over itself, and hands its salt to the sand. Further out it only breathes: a long, level swell that carries ships and the shape of the weather.

- Tide: the moon pulling twice a day
- Current: a river moving inside the ocean
- Horizon: where the water decides to look like sky`

/** The two transitions, playing on their own: a status line swapping states, and a reply streaming in word by word. */
export function MotionSection() {
  const [shown, setShown] = useState(0)
  const [run, setRun] = useState(0)
  // Reveal the reply a few characters at a time, as a model would.
  useEffect(() => {
    setShown(0)
    const id = setInterval(() => setShown((n) => (n >= REPLY.length ? n : n + 7)), 60)
    return () => clearInterval(id)
  }, [run])
  const done = shown >= REPLY.length
  return (
    <Stack sx={{ gap: '32px', maxWidth: 720 }}>
      <Stack sx={{ gap: '8px' }}>
        <Typography variant="subtitle1" sx={{ fontWeight: 600 }}>Thinking states</Typography>
        <Box sx={{ fontSize: '0.875rem' }}>
          <ThinkingStates states={THINKING_STATES} hold={1600} />
        </Box>
      </Stack>
      <Stack sx={{ gap: '8px' }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: '12px' }}>
          <Typography variant="subtitle1" sx={{ fontWeight: 600, flex: 1 }}>Streaming text</Typography>
          <Button size="small" variant="outlined" onClick={() => setRun((r) => r + 1)}>Replay</Button>
        </Stack>
        <Box data-testid="stream-demo" sx={{ minHeight: 220 }}>
          <Markdown text={REPLY.slice(0, shown)} animate={!done} />
        </Box>
      </Stack>
    </Stack>
  )
}
