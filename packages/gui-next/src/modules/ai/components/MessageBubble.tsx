import { memo, useState } from 'react'
import Box from '@mui/material/Box'
import Paper from '@mui/material/Paper'
import Typography from '@mui/material/Typography'
import Stack from '@mui/material/Stack'
import Alert from '@mui/material/Alert'
import { UndoLinear } from '../../../icons'
import type { AgentMessage } from '../lib/types'
import { contentToText, contentToThinking, contentImages } from '../lib/types'
import { Markdown } from './Markdown'
import { ToolCallCard } from './ToolCallCard'
import { ReasoningBlock } from './ReasoningBlock'
import { ImageBlock } from './ImageBlock'
import { ThinkingIndicator } from './ThinkingIndicator'

// Memoized — `MessageList` re-renders on every streamed token (the store
// replaces its `messages` array each time), but non-streaming message
// objects keep the same reference across those re-renders. Without memo,
// every finished bubble re-rendered on every token of a long reply anyway;
// with it, only the bubble whose `message` object actually changed does.
function MessageBubbleImpl({ message, index, onRevert }: { message: AgentMessage; index?: number; onRevert?: (index: number) => void }) {
  const [showRevert, setShowRevert] = useState(false)

  if (message.type === 'toolResult') {
    return (
      <Box sx={{ display: 'flex', justifyContent: 'flex-start' }}>
        <ToolCallCard message={message} />
      </Box>
    )
  }

  const isUser = message.type === 'user'
  const text = contentToText(message.content)
  const thinking = contentToThinking(message.content)
  const images = contentImages(message.content)
  // A failed turn ends with `errorMessage` set and (usually) empty content —
  // without checking this, that state was indistinguishable from "still
  // streaming" and rendered a permanent loading spinner instead of the
  // actual failure (e.g. a local provider that's unreachable).
  const errorMessage = message.type === 'assistant' ? message.errorMessage : null
  const isEmptyStreamingAssistant = !isUser && text.length === 0 && !errorMessage
  const tokensPerSec = message.type === 'assistant' ? message.tokensPerSec : undefined

  return (
    <Stack
      spacing={0.5}
      sx={{ position: 'relative', alignItems: isUser ? 'flex-end' : 'flex-start' }}
      onMouseEnter={() => isUser && onRevert && setShowRevert(true)}
      onMouseLeave={() => setShowRevert(false)}
    >
      {isUser && onRevert && index !== undefined && (
        <Box
          component="button"
          onClick={() => onRevert(index)}
          title="Revert conversation to here (checkpoint)"
          sx={{
            position: 'absolute',
            top: -18,
            right: 0,
            display: 'flex',
            alignItems: 'center',
            gap: 0.5,
            px: 1,
            py: 0.25,
            borderRadius: 999,
            border: 1,
            borderColor: 'divider',
            bgcolor: 'background.paper',
            fontSize: '0.625rem',
            color: 'text.secondary',
            cursor: 'pointer',
            opacity: showRevert ? 1 : 0,
            transition: 'opacity 0.1s',
            zIndex: 5,
            '&:hover': { color: 'text.primary' },
          }}
        >
          <UndoLinear size={10} color="currentColor" />
          Revert here
        </Box>
      )}
      {!isUser && thinking && <ReasoningBlock text={thinking} streaming={isEmptyStreamingAssistant} />}
      {images.length > 0 && (
        <Stack direction="row" spacing={1} sx={{ maxWidth: '80%', flexWrap: 'wrap' }}>
          {images.map((block, i) => (
            <ImageBlock key={i} block={block} />
          ))}
        </Stack>
      )}
      {errorMessage && (
        <Alert severity="error" sx={{ maxWidth: '80%', fontSize: '0.8125rem' }}>
          {errorMessage}
        </Alert>
      )}
      {(text.length > 0 || isEmptyStreamingAssistant) && (
        <Paper
          elevation={0}
          sx={{
            maxWidth: '80%',
            minWidth: 0,
            px: 2,
            py: 1,
            bgcolor: isUser ? 'primary.main' : 'background.paper',
            color: isUser ? 'primary.contrastText' : 'text.primary',
            borderRadius: 2,
          }}
        >
          {isEmptyStreamingAssistant ? (
            <ThinkingIndicator />
          ) : isUser ? (
            <Typography variant="body2" sx={{ whiteSpace: 'pre-wrap' }}>
              {text}
            </Typography>
          ) : (
            <Markdown text={text} />
          )}
        </Paper>
      )}
      {tokensPerSec !== undefined && (
        <Typography variant="caption" color="text.secondary" sx={{ px: 0.5, fontFamily: 'monospace', fontSize: '0.6875rem' }}>
          {tokensPerSec.toFixed(1)} tok/s
        </Typography>
      )}
    </Stack>
  )
}

export const MessageBubble = memo(MessageBubbleImpl)
