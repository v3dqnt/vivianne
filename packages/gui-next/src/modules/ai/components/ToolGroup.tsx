import { useState, type ReactNode } from 'react'
import { Box, CircularProgress, Collapse, Stack, Typography } from '../../../ui'
import { ArrowDownLinear, LayerLinear } from '../../../icons'
import type { AgentMessage } from '../lib/types'
import { summarizeTools } from '../lib/toolGroups'
import { argSummary, declarationFor, iconFor, toolImages } from './ToolCallCard'
import { ImageBlock } from './ImageBlock'

type ToolResultMessage = Extract<AgentMessage, { type: 'toolResult' }>

/**
 * A run of tool calls under one heading, the way a reader wants to see it:
 * "Edited 2 files · Ran 3 commands · Read 4 files", with the individual
 * calls one click away.
 *
 * While the run is live the heading is the action happening right now, so
 * a collapsed group still says what the agent is doing. What the calls
 * produced for the eye — screenshots, plots, generated images — shows below
 * the heading even when collapsed: that output is the point, not a detail.
 */
export function ToolGroup({ tools, children }: { tools: ToolResultMessage[]; children: ReactNode }) {
  const [open, setOpen] = useState(false)
  const running = [...tools].reverse().find((t) => t.isError === null)
  const failed = tools.filter((t) => t.isError === true).length
  const images = tools.flatMap(toolImages)

  let Icon = LayerLinear
  let label: string
  let target: string | null = null
  if (running) {
    Icon = iconFor(running.toolName)
    ;[label, target] = declarationFor(running.toolName, true, argSummary(running.arguments))
  } else {
    label = summarizeTools(tools)
  }

  return (
    <Stack spacing={0.75} sx={{ maxWidth: '80%' }}>
      <Box
        sx={{
          border: 1,
          borderColor: 'divider',
          borderRadius: '12px',
          overflow: 'hidden',
          bgcolor: 'background.paper',
        }}
      >
        <Box
          role="button"
          tabIndex={0}
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault()
              setOpen((v) => !v)
            }
          }}
          sx={{
            display: 'flex',
            alignItems: 'center',
            gap: 1,
            px: 1.5,
            py: 0.75,
            cursor: 'pointer',
            '&:hover': { bgcolor: 'action.hover' },
          }}
        >
          <Box sx={{ display: 'flex', color: 'text.secondary', flexShrink: 0 }}>
            <Icon size={16} color="currentColor" />
          </Box>
          <Typography variant="caption" sx={{ fontWeight: 600, flexShrink: 0 }}>
            {label}
          </Typography>
          {target && (
            <Typography
              variant="caption"
              color="text.secondary"
              noWrap
              sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', minWidth: 0 }}
            >
              {target}
            </Typography>
          )}
          <Box sx={{ flex: 1 }} />
          {failed > 0 && (
            <Typography variant="caption" color="error.main" sx={{ flexShrink: 0 }}>
              {failed} failed
            </Typography>
          )}
          <Typography variant="caption" color="text.disabled" sx={{ flexShrink: 0 }}>
            {tools.length} steps
          </Typography>
          {running && <CircularProgress size={12} thickness={5} />}
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
        </Box>
        <Collapse in={open}>
          {/* Mounted only while open: a long group's cards (and their diffs)
              shouldn't render behind a closed heading. */}
          {open && (
            <Stack spacing={0.75} sx={{ p: 1, borderTop: 1, borderColor: 'divider' }}>
              {children}
            </Stack>
          )}
        </Collapse>
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
