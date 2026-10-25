import { useMemo, useState } from 'react'
import { ButtonBase, Stack, Typography } from '../../../ui'
import { autoCard } from '../lib/autoCard'
import { InteractiveBlock } from './InteractiveBlock'
import { Markdown } from './Markdown'

/**
 * A finished reply, as a card when it reads like a report (headings, a
 * table, labelled lists) and as plain markdown when it doesn't. The card is
 * built from the text itself, so it works whatever the model wrote; "Show as
 * text" puts the original back.
 */
export function ReportMarkdown({ text }: { text: string }) {
  const card = useMemo(() => autoCard(text), [text])
  const [raw, setRaw] = useState(false)
  if (!card) return <Markdown text={text} />
  if (raw) {
    return (
      <Stack sx={{ alignItems: 'flex-start' }}>
        <Markdown text={text} />
        <ButtonBase onClick={() => setRaw(false)} sx={{ borderRadius: '6px', color: 'text.secondary', '&:hover': { color: 'text.primary' } }}>
          <Typography variant="caption">Show as card</Typography>
        </ButtonBase>
      </Stack>
    )
  }
  return (
    <Stack sx={{ alignItems: 'stretch' }}>
      {card.intro && <Markdown text={card.intro} />}
      <InteractiveBlock code={card.json} />
      {card.outro && <Markdown text={card.outro} />}
      <ButtonBase onClick={() => setRaw(true)} sx={{ alignSelf: 'flex-start', borderRadius: '6px', color: 'text.disabled', '&:hover': { color: 'text.primary' } }}>
        <Typography variant="caption">Show as text</Typography>
      </ButtonBase>
    </Stack>
  )
}
