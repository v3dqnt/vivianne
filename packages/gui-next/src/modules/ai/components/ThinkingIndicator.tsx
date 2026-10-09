import { useEffect, useRef, useState } from 'react'
import { Box, Orb, Typography } from '../../../ui'

const WORDS = ['Thinking', 'Cooking', 'Reasoning', 'Introspecting', 'Loading', 'Pondering', 'Working', 'Computing']

// Picks a new random word every tick, but never repeats the one just shown —
// a fixed cycle order reads as robotic, true random can repeat and looks
// broken ("Thinking... Thinking...").
function nextWord(current: string): string {
  const pool = WORDS.filter((w) => w !== current)
  return pool[Math.floor(Math.random() * pool.length)]
}

export function ThinkingIndicator() {
  const [word, setWord] = useState(() => WORDS[Math.floor(Math.random() * WORDS.length)])
  const wordRef = useRef(word)
  wordRef.current = word

  useEffect(() => {
    const id = setInterval(() => setWord(nextWord(wordRef.current)), 1600)
    return () => clearInterval(id)
  }, [])

  return (
    <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, py: 0.25 }}>
      <Orb state="working" label="Working" />
      <Typography
        key={word}
        variant="body2"
        color="text.secondary"
        sx={{
          animation: 'viv-fade-in 0.25s ease-out',
          '@keyframes viv-fade-in': {
            from: { opacity: 0, transform: 'translateY(2px)' },
            to: { opacity: 1, transform: 'translateY(0)' },
          },
        }}
      >
        {word}…
      </Typography>
    </Box>
  )
}
