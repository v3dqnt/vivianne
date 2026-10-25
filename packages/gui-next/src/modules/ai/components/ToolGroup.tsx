import { useState, type ReactNode } from 'react'
import { Box, CircularProgress, Collapse, Stack, Typography } from '../../../ui'
import { ArrowDownLinear } from '../../../icons'
import type { AgentMessage } from '../lib/types'
import { summarizeTools } from '../lib/toolGroups'
import { argSummary, declarationFor, toolImages, TOOL_BLOCK_SX } from './ToolCallCard'
import { ImageBlock } from './ImageBlock'
import { FileLink, filePathArg } from './FileLink'
import { CHAT_META_SX, TOOL_LIST_SX, TOOL_ROW_SX } from '../lib/chatStyle'

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

  let label: string
  let target: string | null = null
  let file: string | null = null
  if (running) {
    ;[label, target] = declarationFor(running.toolName, true, argSummary(running.arguments))
    file = target !== null ? filePathArg(running.arguments) : null
  } else {
    label = summarizeTools(tools)
  }

  return (
    <Stack spacing={0.5} sx={TOOL_BLOCK_SX}>
      <Box>
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
            ...TOOL_ROW_SX,
            cursor: 'pointer',
            '&:hover': { color: 'text.primary' },
          }}
        >
          <Box component="span" sx={{ flexShrink: 0, whiteSpace: 'nowrap' }}>
            {label}
          </Box>
          {file ? (
            <FileLink path={file} />
          ) : (
            target && (
              <Typography noWrap sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.75rem', color: 'text.secondary', minWidth: 0 }}>
                {target}
              </Typography>
            )
          )}
          {failed > 0 && (
            <Box component="span" sx={{ ...CHAT_META_SX, color: 'error.main', flexShrink: 0 }}>
              · {failed} failed
            </Box>
          )}
          {running && <CircularProgress size={11} thickness={5} />}
          <Box sx={{ display: 'flex', flexShrink: 0, opacity: 0.8, transform: open ? 'rotate(180deg)' : 'none', transition: 'transform 0.15s' }}>
            <ArrowDownLinear size={12} color="currentColor" />
          </Box>
        </Box>
        <Collapse in={open}>
          {/* Mounted only while open: a long group's steps (and their diffs)
              shouldn't render behind a closed heading. */}
          {open && <Box sx={TOOL_LIST_SX}>{children}</Box>}
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
