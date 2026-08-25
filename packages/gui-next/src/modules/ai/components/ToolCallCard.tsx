import { useState } from 'react'
import Box from '@mui/material/Box'
import Collapse from '@mui/material/Collapse'
import CircularProgress from '@mui/material/CircularProgress'
import Typography from '@mui/material/Typography'
import { CommandLinear, EditLinear, SearchNormalLinear, DocumentTextLinear, CodeLinear, ArrowDownLinear, CheckLinear, DangerLinear, ArchiveBookLinear } from '../../../icons'
import Stack from '@mui/material/Stack'
import type { AgentMessage } from '../lib/types'
import { contentToText, contentImages } from '../lib/types'
import { ImageBlock } from './ImageBlock'
import { DiffView } from './DiffView'

type ToolResultMessage = Extract<AgentMessage, { type: 'toolResult' }>

const MEMORY_TOOLS: Record<string, string> = {
  memory_get: 'Recalling memory…',
  memory_search: 'Searching memory…',
  notes_write: 'Taking a note…',
}

// Coarse icon-by-name-substring — good enough to visually distinguish the
// common tool families without a per-tool registry to maintain.
function iconFor(name: string) {
  if (name in MEMORY_TOOLS) return ArchiveBookLinear
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

interface FileDiff {
  path: string
  before: string | null
  after: string | null
}

/** `write`/`edit` carry everything needed to reconstruct a diff in the
 * persisted `arguments` + `details` fields — no backend change needed for
 * those two. `apply_patch` needed one small addition (`details.filesDiff`,
 * added alongside this UI) since it touches multiple files per call and
 * nothing else captured per-file before/after content. */
function extractDiffs(message: ToolResultMessage): FileDiff[] {
  const args = message.arguments
  const details = message.details as Record<string, unknown> | null
  switch (message.toolName) {
    case 'write': {
      if (!args || typeof args.path !== 'string' || typeof args.content !== 'string') return []
      const previous = details && typeof details.previous_content === 'string' ? details.previous_content : null
      return [{ path: args.path, before: previous, after: args.content }]
    }
    case 'edit': {
      if (!args || typeof args.path !== 'string') return []
      const before = typeof args.old_string === 'string' ? args.old_string : null
      const after = typeof args.new_string === 'string' ? args.new_string : null
      if (before === null && after === null) return []
      return [{ path: args.path, before, after }]
    }
    case 'apply_patch': {
      const raw = details && Array.isArray(details.filesDiff) ? (details.filesDiff as unknown[]) : []
      return raw
        .filter((d): d is Record<string, unknown> => !!d && typeof d === 'object')
        .map((d) => ({
          path: typeof d.path === 'string' ? d.path : '',
          before: typeof d.before === 'string' ? d.before : null,
          after: typeof d.after === 'string' ? d.after : null,
        }))
    }
    default:
      return []
  }
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
  // Only a completed, successful call has real before/after content — a
  // failed `edit` (e.g. old_string not found) changed nothing, so there's
  // nothing to diff. `extractDiffs` itself is cheap (just reshapes
  // arguments/details), but each diff's actual line-by-line comparison is
  // an O(N*M) table (see `lib/diff.ts`) — that only runs once `open` gates
  // `DiffView` below, not here, so `hasBody` can still see whether there's
  // anything to expand.
  const diffs = !pending && !isError ? extractDiffs(message) : []
  const hasBody = !!output || diffs.length > 0

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
        onClick={() => hasBody && setOpen((v) => !v)}
        sx={{
          display: 'flex',
          alignItems: 'center',
          gap: 1,
          px: 1.5,
          py: 0.75,
          cursor: hasBody ? 'pointer' : 'default',
          '&:hover': hasBody ? { bgcolor: 'action.hover' } : undefined,
        }}
      >
        <Box sx={{ display: 'flex', color: 'text.secondary', flexShrink: 0 }}>
          <Icon size={16} color="currentColor" />
        </Box>
        <Typography variant="caption" sx={{ fontWeight: 600, flexShrink: 0 }}>
          {pending && message.toolName in MEMORY_TOOLS ? MEMORY_TOOLS[message.toolName] : message.toolName}
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
        {hasBody && (
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
      {hasBody && (
        // MUI's Collapse keeps its children mounted regardless of `in` — it
        // animates height, so it needs them present to measure. That meant
        // every diff's O(N*M) line comparison (`DiffView` → `diffLines`) ran
        // on every render of a long thread even while collapsed. Gating the
        // body on `open` means nothing here mounts — or diffs — until the
        // card is actually expanded.
        <Collapse in={open}>
          {open && (
            <>
              {diffs.map((d, i) => (
                <Box key={i}>
                  <Typography
                    variant="caption"
                    color="text.secondary"
                    sx={{ display: 'block', px: 1.5, pt: 1, borderTop: i === 0 ? 1 : 0, borderColor: 'divider', fontFamily: 'monospace' }}
                  >
                    {d.path}
                  </Typography>
                  <DiffView before={d.before} after={d.after} />
                </Box>
              ))}
              {output && (
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
              )}
            </>
          )}
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
