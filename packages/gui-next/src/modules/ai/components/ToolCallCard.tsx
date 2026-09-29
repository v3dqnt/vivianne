import { useState } from 'react'
import { Box, Collapse, CircularProgress, Typography, Stack } from '../../../ui'
import { CommandLinear, EditLinear, SearchNormalLinear, DocumentTextLinear, CodeLinear, ArrowDownLinear, CheckLinear, DangerLinear, ArchiveBookLinear } from '../../../icons'
import type { AgentMessage } from '../lib/types'
import { contentToText, contentImages } from '../lib/types'
import { ImageBlock } from './ImageBlock'
import { DiffView } from './DiffView'
import { CodeBlock } from './CodeBlock'

type ToolResultMessage = Extract<AgentMessage, { type: 'toolResult' }>

const MEMORY_TOOLS: Record<string, string> = {
  memory_get: 'Recalling memory…',
  memory_search: 'Searching memory…',
  notes_write: 'Taking a note…',
}

/** Human phrasing per tool, as `[running, finished]`.
 *
 * The card used to print the raw tool name — `todo_write`, `apply_patch`,
 * `code_search` — which is the agent's vocabulary, not the reader's. Naming
 * the ACTION lets the transcript be skimmed as a narrative of what happened
 * rather than decoded as a list of function calls. Tools not listed here fall
 * back to their own name, so a playbook or MCP tool still renders sensibly. */
const DECLARATIONS: Record<string, [string, string]> = {
  bash: ['Running', 'Ran'],
  powershell: ['Running', 'Ran'],
  sandbox_execute: ['Running sandboxed', 'Ran sandboxed'],
  run_script: ['Running script', 'Ran script'],
  read: ['Reading', 'Read'],
  write: ['Writing', 'Wrote'],
  edit: ['Editing', 'Edited'],
  apply_patch: ['Applying patch to', 'Patched'],
  notebook_edit: ['Editing notebook', 'Edited notebook'],
  ls: ['Listing', 'Listed'],
  glob: ['Matching', 'Matched'],
  grep: ['Searching for', 'Searched for'],
  code_search: ['Searching code for', 'Searched code for'],
  web_search: ['Searching the web for', 'Searched the web for'],
  web_fetch: ['Fetching', 'Fetched'],
  web_preview: ['Opening', 'Opened'],
  create_document: ['Creating', 'Created'],
  generate_image: ['Generating image', 'Generated image'],
  math: ['Calculating', 'Calculated'],
  todo_write: ['Updating the plan', 'Updated the plan'],
  submit_plan: ['Writing the plan', 'Proposed a plan'],
  message_agent: ['Messaging a teammate', 'Messaged a teammate'],
  assign_plan_steps: ['Carrying out the plan', 'Carried out the plan'],
  task: ['Delegating to a subagent', 'Delegated to a subagent'],
  dip: ['Using the screen', 'Used the screen'],
  computer: ['Using the screen', 'Used the screen'],
  repo_clone: ['Cloning', 'Cloned'],
  repo_overview: ['Reading the repo', 'Read the repo'],
  diagnostics: ['Checking diagnostics', 'Checked diagnostics'],
}

/** `[label, target]` for the card header. */
export function declarationFor(name: string, pending: boolean, summary: string | null): [string, string | null] {
  if (pending && name in MEMORY_TOOLS) return [MEMORY_TOOLS[name], null]
  const phrasing = DECLARATIONS[name]
  if (!phrasing) return [name, summary]
  return [pending ? phrasing[0] : phrasing[1], summary]
}

// Coarse icon-by-name-substring — good enough to visually distinguish the
// common tool families without a per-tool registry to maintain.
export function iconFor(name: string) {
  if (name in MEMORY_TOOLS) return ArchiveBookLinear
  const n = name.toLowerCase()
  if (n.includes('bash') || n.includes('shell') || n.includes('command') || n.includes('terminal')) return CommandLinear
  if (n.includes('write') || n.includes('edit') || n.includes('patch')) return EditLinear
  if (n.includes('search') || n.includes('grep') || n.includes('glob') || n.includes('find')) return SearchNormalLinear
  if (n.includes('read') || n.includes('file')) return DocumentTextLinear
  return CodeLinear
}

export function argSummary(args: Record<string, unknown> | undefined): string | null {
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

type ImageContent = Parameters<typeof ImageBlock>[0]['block']

/** Every picture a tool call produced: its image blocks, plus plots from
 * `math`, which ride in `details` when the model can't take images — the
 * user should see them either way. */
export function toolImages(message: ToolResultMessage): ImageContent[] {
  const detailPlots = ((message.details as { plots?: unknown } | null)?.plots ?? []) as unknown[]
  const plots = detailPlots
    .filter((p): p is string => typeof p === 'string')
    .map((b64) => ({ type: 'image', base64: b64, mediaType: 'image/png', url: null }) as ImageContent)
  return [...contentImages(message.content), ...plots]
}

export function ToolCallCard({
  message,
  compact = false,
}: {
  message: ToolResultMessage
  /** Inside a tool group: full width, and no media row — the group shows
   * its members' media itself, so it stays visible while collapsed. */
  compact?: boolean
}) {
  const [open, setOpen] = useState(false)
  // `isError` is the wire type's in-flight sentinel: null until the tool
  // finishes, then true/false. Nothing else needs checking for "pending."
  const pending = message.isError === null
  const isError = message.isError === true
  const Icon = iconFor(message.toolName)
  const summary = argSummary(message.arguments)
  const [label, target] = declarationFor(message.toolName, pending, summary)
  const output = contentToText(message.content).trim()
  const images = compact ? [] : toolImages(message)
  // Declared AFTER `output`, because it reads it. Putting this above the
  // `const output` line threw `Cannot access 'output' before initialization`
  // at render — a temporal dead zone error, so it only fired once a thread
  // containing a tool call was actually rendered, which took the whole
  // message list down when switching threads.
  //
  // Shell output is not any language shiki knows; tagging it `text` avoids
  // spurious highlighting of arbitrary stdout. Structured tools that really
  // do emit JSON get the JSON grammar.
  const outputLang = ['read', 'code_search', 'grep', 'glob'].includes(message.toolName)
    ? 'text'
    : output.trimStart().startsWith('{') || output.trimStart().startsWith('[')
      ? 'json'
      : 'text'
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
    <Stack spacing={0.5} sx={{ maxWidth: compact ? '100%' : '80%' }}>
    <Box
      sx={{
        border: 1,
        borderColor: isError ? 'error.main' : 'divider',
        borderRadius: compact ? '8px' : '12px',
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
        <Typography variant="caption" sx={{ fontWeight: 600, flexShrink: 0 }} title={message.toolName}>
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
      {isError && output && !open && (
        <Typography
          variant="caption"
          color="error.main"
          sx={{
            display: '-webkit-box',
            WebkitLineClamp: 2,
            WebkitBoxOrient: 'vertical',
            overflow: 'hidden',
            px: 1.5,
            pb: 0.75,
            fontFamily: '"Geist Mono", ui-monospace, monospace',
            fontSize: '0.6875rem',
            lineHeight: 1.5,
          }}
        >
          {output}
        </Typography>
      )}
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
                    sx={{ display: 'block', px: 1.5, pt: 1, borderTop: i === 0 ? 1 : 0, borderColor: 'divider', fontFamily: '"Geist Mono", ui-monospace, monospace' }}
                  >
                    {d.path}
                  </Typography>
                  <DiffView before={d.before} after={d.after} />
                </Box>
              ))}
              {output && (
                // Routed through CodeBlock so tool output, chat fences and
                // file previews all render as the same object. It brings
                // theme-correct highlighting and the long-output collapse
                // with it, replacing a bespoke <pre> that had neither.
                <Box sx={{ borderTop: 1, borderColor: 'divider' }}>
                  <CodeBlock code={output} lang={outputLang} wrap bare={false} collapsible />
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
