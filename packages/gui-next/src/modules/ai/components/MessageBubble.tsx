import { memo, useState } from 'react'
import { Box, Paper, Typography, Stack, Alert, IconButton, Tooltip } from '../../../ui'
import { UndoLinear, CopyLinear, EditLinear, TickCircleLinear } from '../../../icons'
import type { AgentMessage } from '../lib/types'
import { contentToText, contentToThinking, contentImages } from '../lib/types'
import { Markdown } from './Markdown'
import { ToolCallCard } from './ToolCallCard'
import { TeamRunCard } from '../../team/components/TeamRunCard'
import { ReasoningBlock } from './ReasoningBlock'
import { ImageBlock } from './ImageBlock'
import { ThinkingIndicator } from './ThinkingIndicator'
import { HandoffDivider, isHandoffText, SummaryDivider, isSummaryText, PairActivityDivider, isPairActivityText, ResumeDivider, isResumeText, PlanNoteDivider, isPlanNoteText } from './HandoffDivider'

// Memoized — `MessageList` re-renders on every streamed token (the store
// replaces its `messages` array each time), but non-streaming message
// objects keep the same reference across those re-renders. Without memo,
// every finished bubble re-rendered on every token of a long reply anyway;
// with it, only the bubble whose `message` object actually changed does.
/** "Jun 30, 1:03 AM" — same shape the action row shows under every bubble. */
function formatTimestamp(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  })
}

function MessageBubbleImpl({
  message,
  index,
  streaming = false,
  onRevert,
  onEdit,
  compact = false,
}: {
  message: AgentMessage
  index?: number
  /** This is the message being generated right now. */
  streaming?: boolean
  onRevert?: (index: number) => void
  onEdit?: (index: number, text: string) => void
  /** Rendered inside a tool group (see `ToolGroup`). */
  compact?: boolean
}) {
  const [hovered, setHovered] = useState(false)
  const [copied, setCopied] = useState(false)

  if (message.type === 'toolResult' && message.toolName === 'team_run') {
    const details = message.details as { teamRun?: string } | null
    return <TeamRunCard runId={details?.teamRun ?? null} pending={message.isError === null} />
  }
  if (message.type === 'toolResult') {
    return (
      <Box sx={{ display: 'flex', justifyContent: 'flex-start' }}>
        <ToolCallCard message={message} compact={compact} />
      </Box>
    )
  }

  const isUser = message.type === 'user'
  const text = contentToText(message.content)
  // A new day's handoff note rides in a user message but isn't from the user.
  if (isUser && isHandoffText(text)) return <HandoffDivider text={text} />
  if (isUser && isSummaryText(text)) return <SummaryDivider text={text} />
  if (isUser && isPairActivityText(text)) return <PairActivityDivider text={text} />
  if (isUser && isResumeText(text)) return <ResumeDivider text={text} />
  if (isUser && isPlanNoteText(text)) return <PlanNoteDivider text={text} />
  const thinking = contentToThinking(message.content)
  const images = contentImages(message.content)
  // A failed turn ends with `errorMessage` set and (usually) empty content —
  // without checking this, that state was indistinguishable from "still
  // streaming" and rendered a permanent loading spinner instead of the
  // actual failure (e.g. a local provider that's unreachable).
  const errorMessage = message.type === 'assistant' ? message.errorMessage : null
  // "Still streaming" must come from the stream itself, not be guessed from
  // an empty body: a finished reply can have no text — reasoning cut off at
  // the output limit, or only tool calls — and guessing left those showing
  // "Thinking…" forever after the turn had ended.
  const isEmptyStreamingAssistant = !isUser && text.length === 0 && !errorMessage && streaming
  const cutOffWhileReasoning =
    !isUser && !streaming && text.length === 0 && !errorMessage && message.type === 'assistant' && message.stopReason === 'length' && !!thinking
  const tokensPerSec = message.type === 'assistant' ? message.tokensPerSec : undefined

  const copy = () => {
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1200)
    })
  }

  // Hidden while a reply is still streaming: the text isn't final yet, so
  // copying it would silently grab a partial message.
  const showActions = !isEmptyStreamingAssistant && text.length > 0

  return (
    <Stack
      spacing={0.5}
      sx={{ position: 'relative', alignItems: isUser ? 'flex-end' : 'flex-start' }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
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
      {cutOffWhileReasoning && (
        <Typography variant="caption" color="text.secondary" sx={{ px: 0.5 }}>
          This reply hit the model’s output limit while it was still reasoning, so it ended before answering.
        </Typography>
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
      {showActions && (
        <Stack
          direction="row"
          sx={{
            alignItems: 'center',
            gap: 0.25,
            px: 0.5,
            minHeight: 22,
            // The timestamp stays legible at rest; the buttons only appear on
            // hover so a long conversation isn't a wall of icons.
            color: 'text.disabled',
          }}
        >
          <Typography variant="caption" sx={{ fontSize: '0.6875rem', mr: 0.5 }}>
            {formatTimestamp(message.timestamp)}
          </Typography>
          {tokensPerSec !== undefined && (
            <Typography variant="caption" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.6875rem', mr: 0.5 }}>
              {tokensPerSec.toFixed(1)} tok/s
            </Typography>
          )}
          <Stack
            direction="row"
            sx={{ alignItems: 'center', gap: 0.25, opacity: hovered ? 1 : 0, transition: 'opacity 0.12s' }}
          >
            <Tooltip title={copied ? 'Copied' : 'Copy'}>
              <IconButton size="small" onClick={copy} sx={{ width: 22, height: 22, color: 'inherit' }}>
                {copied ? <TickCircleLinear size={12} color="currentColor" /> : <CopyLinear size={12} color="currentColor" />}
              </IconButton>
            </Tooltip>
            {isUser && onEdit && index !== undefined && (
              <Tooltip title="Edit and resend — removes everything after this">
                <IconButton
                  size="small"
                  onClick={() => onEdit(index, text)}
                  sx={{ width: 22, height: 22, color: 'inherit' }}
                >
                  <EditLinear size={12} color="currentColor" />
                </IconButton>
              </Tooltip>
            )}
            {isUser && onRevert && index !== undefined && (
              <Tooltip title="Revert conversation to here (checkpoint)">
                <IconButton
                  size="small"
                  onClick={() => onRevert(index)}
                  sx={{ width: 22, height: 22, color: 'inherit' }}
                >
                  <UndoLinear size={12} color="currentColor" />
                </IconButton>
              </Tooltip>
            )}
          </Stack>
        </Stack>
      )}
    </Stack>
  )
}

export const MessageBubble = memo(MessageBubbleImpl)
