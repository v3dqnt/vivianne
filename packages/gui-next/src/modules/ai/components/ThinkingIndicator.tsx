import { useEffect, useRef, useState } from 'react'
import { Box, Typography } from '../../../ui'

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
      <Box
        sx={{
          width: 6,
          height: 6,
          borderRadius: '50%',
          bgcolor: 'primary.main',
          animation: 'viv-pulse 1.1s ease-in-out infinite',
          '@keyframes viv-pulse': {
            '0%, 100%': { opacity: 0.35, transform: 'scale(0.85)' },
            '50%': { opacity: 1, transform: 'scale(1)' },
          },
        }}
      />
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
