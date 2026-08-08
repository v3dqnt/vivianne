import Box from '@mui/material/Box'
import Paper from '@mui/material/Paper'
import Typography from '@mui/material/Typography'
import Stack from '@mui/material/Stack'
import type { AgentMessage } from '../lib/types'
import { contentToText, contentToThinking, contentImages } from '../lib/types'
import { Markdown } from './Markdown'
import { ToolCallCard } from './ToolCallCard'
import { ReasoningBlock } from './ReasoningBlock'
import { ImageBlock } from './ImageBlock'
import { ThinkingIndicator } from './ThinkingIndicator'

export function MessageBubble({ message }: { message: AgentMessage }) {
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
  const isEmptyStreamingAssistant = !isUser && text.length === 0

  return (
    <Stack spacing={0.5} sx={{ alignItems: isUser ? 'flex-end' : 'flex-start' }}>
      {!isUser && thinking && <ReasoningBlock text={thinking} streaming={isEmptyStreamingAssistant} />}
      {images.length > 0 && (
        <Stack direction="row" spacing={1} sx={{ maxWidth: '80%', flexWrap: 'wrap' }}>
          {images.map((block, i) => (
            <ImageBlock key={i} block={block} />
          ))}
        </Stack>
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
    </Stack>
  )
}
