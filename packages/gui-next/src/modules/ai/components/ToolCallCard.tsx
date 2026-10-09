import { useState } from 'react'
import { Box, Button, Collapse, CircularProgress, Typography, Stack, alpha, useTheme } from '../../../ui'
import {
  CommandLinear,
  EditLinear,
  SearchNormalLinear,
  DocumentTextLinear,
  CodeLinear,
  ArrowDownLinear,
  CheckLinear,
  DangerLinear,
  ArchiveBookLinear,
  ClipboardTextLinear,
  MessageSquareLinear,
  AgentsLinear,
  GlobalLinear,
  MonitorLinear,
  MagicpenLinear,
} from '../../../icons'
import { usePlanStore } from '../../plan/store/planStore'
import { usePanelStore } from '../../dock/store/panelStore'
import type { AgentMessage } from '../lib/types'
import { contentToText, contentImages } from '../lib/types'
import { ImageBlock } from './ImageBlock'
import { DiffView } from './DiffView'
import { CodeBlock } from './CodeBlock'
import { FileLink, filePathArg } from './FileLink'
import { CHAT_HEADER_SX, CHAT_MAX_WIDTH, CHAT_RADIUS, CHAT_RADIUS_INNER, CHAT_SURFACE_SX, CHAT_TITLE_SX } from '../lib/chatStyle'

type ToolResultMessage = Extract<AgentMessage, { type: 'toolResult' }>

/** One width for everything a tool leaves in the chat — groups, cards,
 * checkpoints, team runs — so they line up as a column instead of each
 * being as wide as its own text. */
export const TOOL_BLOCK_SX = { width: '100%', maxWidth: CHAT_MAX_WIDTH } as const

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
  create_tool: ['Writing a tool', 'Made a tool'],
  delete_tool: ['Retiring a tool', 'Retired a tool'],
  list_custom_tools: ['Reviewing its tools', 'Reviewed its tools'],
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
const ICONS: Record<string, typeof CodeLinear> = {
  todo_write: ClipboardTextLinear,
  submit_plan: ClipboardTextLinear,
  assign_plan_steps: ClipboardTextLinear,
  present_plan: ClipboardTextLinear,
  message_agent: MessageSquareLinear,
  task: AgentsLinear,
  team_run: AgentsLinear,
  web_search: GlobalLinear,
  web_fetch: GlobalLinear,
  web_preview: GlobalLinear,
  dip: MonitorLinear,
  computer: MonitorLinear,
  generate_image: MagicpenLinear,
}

export function iconFor(name: string) {
  if (name in MEMORY_TOOLS) return ArchiveBookLinear
  if (name in ICONS) return ICONS[name]
  const n = name.toLowerCase()
  if (n.startsWith('browser_')) return GlobalLinear
  if (n.includes('bash') || n.includes('shell') || n.includes('command') || n.includes('terminal')) return CommandLinear
  if (n.includes('write') || n.includes('edit') || n.includes('patch')) return EditLinear
  if (n.includes('search') || n.includes('grep') || n.includes('glob') || n.includes('find')) return SearchNormalLinear
  if (n.includes('read') || n.includes('file')) return DocumentTextLinear
  return CodeLinear
}

export function argSummary(args: Record<string, unknown> | undefined): string | null {
  if (!args) return null
  const key = ['path', 'file_path', 'command', 'query', 'pattern', 'url', 'name'].find((k) => typeof args[k] === 'string')
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
  const file = target !== null ? filePathArg(message.arguments) : null
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
  const outputLang = ['read', 'code_search', 'grep', 'glob'].includes(message.toolName) ? 'text' : looksLikeJson(output) ? 'json' : 'text'
  // Only a completed, successful call has real before/after content — a
  // failed `edit` (e.g. old_string not found) changed nothing, so there's
  // nothing to diff. `extractDiffs` itself is cheap (just reshapes
  // arguments/details), but each diff's actual line-by-line comparison is
  // an O(N*M) table (see `lib/diff.ts`) — that only runs once `open` gates
  // `DiffView` below, not here, so `hasBody` can still see whether there's
  // anything to expand.
  const diffs = !pending && !isError ? extractDiffs(message) : []
  // Some tools say something to the model, not to the user: their result is
  // replaced by a view of what they did, always shown, never the raw text.
  const special = !pending && !isError ? specialBody(message) : null
  const hasBody = !special && (!!output || diffs.length > 0)

  return (
    // Open, a card takes the full width it's given: its output shouldn't be
    // squeezed to the width of its header.
    <Stack spacing={0.5} sx={compact ? { width: '100%' } : TOOL_BLOCK_SX}>
    <Box
      sx={{
        ...CHAT_SURFACE_SX,
        borderColor: isError ? 'error.main' : 'divider',
        borderRadius: compact ? CHAT_RADIUS_INNER : CHAT_RADIUS,
      }}
    >
      <Box
        onClick={() => hasBody && setOpen((v) => !v)}
        sx={{
          ...CHAT_HEADER_SX,
          ...(compact && { minHeight: 34, py: 0.5 }),
          cursor: hasBody ? 'pointer' : 'default',
          '&:hover': hasBody ? { bgcolor: 'action.hover' } : undefined,
        }}
      >
        <Box sx={{ display: 'flex', color: 'text.secondary', flexShrink: 0 }}>
          <Icon size={16} color="currentColor" />
        </Box>
        <Typography variant="caption" sx={CHAT_TITLE_SX} title={message.toolName}>
          {label}
        </Typography>
        {file ? (
          <FileLink path={file} />
        ) : (
          target && (
            <Typography
              variant="caption"
              color="text.secondary"
              noWrap
              sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', minWidth: 0 }}
            >
              {target}
            </Typography>
          )
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
      {special}
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
                  <Box sx={{ display: 'flex', px: 1.5, pt: 1, borderTop: i === 0 ? 1 : 0, borderColor: 'divider' }}>
                    <FileLink path={d.path} />
                  </Box>
                  <DiffView before={d.before} after={d.after} />
                </Box>
              ))}
              {output && <ToolOutput code={output} lang={outputLang} />}
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

/**
 * A tool's output, as part of the card rather than a box inside it: the
 * card's own surface, a faint inset tint, small muted monospace, copy on
 * hover, and the long-output collapse. No header — the card's header already
 * says what this is.
 */
function ToolOutput({ code, lang }: { code: string; lang: string }) {
  const theme = useTheme()
  return (
    <Box sx={{ borderTop: 1, borderColor: 'divider', bgcolor: alpha(theme.palette.text.primary, 0.02), px: 1.5, py: 1, color: 'text.secondary' }}>
      <CodeBlock code={code} lang={lang} wrap bare dense collapsible />
    </Box>
  )
}

/** One quiet line under the header, for tools whose result is a pointer to
 * something rather than text to read. */
function BodyLine({ children }: { children: React.ReactNode }) {
  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.5, pb: 1, pt: 0.25, minWidth: 0 }}>
      {children}
    </Stack>
  )
}

/** The view that replaces a tool's raw result, when there is one. */
function specialBody(message: ToolResultMessage): React.ReactNode | null {
  const details = (message.details ?? {}) as Record<string, unknown>
  const args = message.arguments ?? {}
  switch (message.toolName) {
    case 'submit_plan': {
      const planId = typeof details.planId === 'string' ? details.planId : null
      const version = typeof details.version === 'number' ? details.version : null
      if (!planId) return null
      const title = typeof args.title === 'string' ? args.title : 'The plan'
      return (
        <BodyLine>
          <Typography variant="caption" color="text.secondary" noWrap sx={{ flex: 1, minWidth: 0 }}>
            {title}
            {version ? ` · v${version}` : ''} — ready for your review
          </Typography>
          <Button
            size="small"
            onClick={() => {
              void usePlanStore.getState().open(planId, version ?? undefined)
              usePanelStore.getState().openRight('plan')
            }}
            sx={{ flexShrink: 0 }}
          >
            Open plan
          </Button>
        </BodyLine>
      )
    }
    case 'message_agent': {
      const to = typeof args.to === 'string' ? args.to : ''
      const text = typeof args.text === 'string' ? args.text : ''
      if (!text) return null
      return (
        <BodyLine>
          <Typography variant="caption" color="text.secondary" sx={{ whiteSpace: 'pre-wrap', lineHeight: 1.5 }}>
            <Box component="span" sx={{ color: 'text.disabled' }}>
              to {to === 'all' ? 'everyone' : to}:{' '}
            </Box>
            {text}
          </Typography>
        </BodyLine>
      )
    }
    default:
      return null
  }
}

/** JSON only if it parses: a log line like "[01] compiling…" starts with a
 * bracket too, and highlighting it as JSON paints it in random colours. */
function looksLikeJson(text: string): boolean {
  const t = text.trim()
  if (!(t.startsWith('{') || t.startsWith('['))) return false
  try {
    JSON.parse(t)
    return true
  } catch {
    return false
  }
}
