import { useState } from 'react'
import Box from '@mui/material/Box'
import Collapse from '@mui/material/Collapse'
import CircularProgress from '@mui/material/CircularProgress'
import Typography from '@mui/material/Typography'
import { CommandLinear, EditLinear, SearchNormalLinear, DocumentTextLinear, CodeLinear, ArrowDownLinear, CheckLinear, DangerLinear } from 'mx-icons'
import Stack from '@mui/material/Stack'
import type { AgentMessage } from '../lib/types'
import { contentToText, contentImages } from '../lib/types'
import { ImageBlock } from './ImageBlock'

type ToolResultMessage = Extract<AgentMessage, { type: 'toolResult' }>

// Coarse icon-by-name-substring — good enough to visually distinguish the
// common tool families without a per-tool registry to maintain.
function iconFor(name: string) {
  const n = name.toLowerCase()
  if (n.includes('bash') || n.includes('shell') || n.includes('command') || n.includes('terminal')) return CommandLinear
  if (n.includes('write') || n.includes('edit') || n.includes('patch')) return EditLinear
  if (n.includes('search') || n.includes('grep') || n.includes('glob') || n.includes('find')) return SearchNormalLinear
  if (n.includes('read') || n.includes('file')) return DocumentTextLinear
  return CodeLinear
}

function argSummary(args: Record<string, unknown> | undefined): string | null {
  if (!args) return null
  const key = ['path', 'file_path', 'command', 'query', 'pattern', 'url'].find((k) => typeof args[k] === 'string')
  if (!key) return null
  return String(args[key])
}

export function ToolCallCard({ message }: { message: ToolResultMessage }) {
  const [open, setOpen] = useState(false)
  // `isError` is the wire type's in-flight sentinel: null until the tool
  // finishes, then true/false. Nothing else needs checking for "pending."
  const pending = message.isError === null
  const isError = message.isError === true
  const Icon = iconFor(message.toolName)
  const summary = argSummary(message.arguments)
  const output = contentToText(message.content).trim()
  const images = contentImages(message.content)

  return (
    <Stack spacing={0.5} sx={{ maxWidth: '80%' }}>
    <Box
      sx={{
        border: 1,
        borderColor: isError ? 'error.main' : 'divider',
        borderRadius: 2,
        overflow: 'hidden',
        bgcolor: 'background.paper',
      }}
    >
      <Box
        onClick={() => output && setOpen((v) => !v)}
        sx={{
          display: 'flex',
          alignItems: 'center',
          gap: 1,
          px: 1.5,
          py: 0.75,
          cursor: output ? 'pointer' : 'default',
          '&:hover': output ? { bgcolor: 'action.hover' } : undefined,
        }}
      >
        <Box sx={{ display: 'flex', color: 'text.secondary', flexShrink: 0 }}>
          <Icon size={16} color="currentColor" />
        </Box>
        <Typography variant="caption" sx={{ fontWeight: 600, flexShrink: 0 }}>
          {message.toolName}
        </Typography>
        {summary && (
          <Typography
            variant="caption"
            color="text.secondary"
            noWrap
            sx={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', minWidth: 0 }}
          >
            {summary}
          </Typography>
        )}
        <Box sx={{ flex: 1 }} />
        {pending ? (
          <CircularProgress size={12} thickness={5} />
        ) : isError ? (
          <Box sx={{ display: 'flex', color: 'error.main' }}>
            <DangerLinear size={15} color="currentColor" />
          </Box>
        ) : (
          <Box sx={{ display: 'flex', color: 'success.main' }}>
            <CheckLinear size={15} color="currentColor" />
          </Box>
        )}
        {output && (
          <Box
            sx={{
              display: 'flex',
              color: 'text.secondary',
              transform: open ? 'rotate(180deg)' : 'none',
              transition: 'transform 0.15s',
            }}
          >
            <ArrowDownLinear size={16} color="currentColor" />
          </Box>
        )}
      </Box>
      {output && (
        <Collapse in={open}>
          <Box
            component="pre"
            sx={{
              m: 0,
              px: 1.5,
              py: 1,
              borderTop: 1,
              borderColor: 'divider',
              bgcolor: 'action.hover',
              fontFamily: '"JetBrains Mono", ui-monospace, monospace',
              fontSize: '0.75rem',
              lineHeight: 1.5,
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
              maxHeight: 320,
              overflowY: 'auto',
            }}
          >
            {output}
          </Box>
        </Collapse>
      )}
    </Box>
    {images.length > 0 && (
      <Stack direction="row" spacing={1} sx={{ flexWrap: 'wrap' }}>
        {images.map((block, i) => (
          <ImageBlock key={i} block={block} />
        ))}
      </Stack>
    )}
    </Stack>
  )
}
