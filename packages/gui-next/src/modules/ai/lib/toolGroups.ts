import type { AgentMessage } from './types'
import { contentToText } from './types'

type ToolResultMessage = Extract<AgentMessage, { type: 'toolResult' }>

/** One row of the transcript: a single message, or a run of tool activity
 * folded under one heading. */
export type TranscriptItem =
  | { kind: 'message'; index: number }
  | { kind: 'group'; start: number; indices: number[]; toolIndices: number[] }
  /** Part of the plan got done at this message (see `checkpoints.ts`). */
  | { kind: 'checkpoint'; index: number }

/** Assistant turns that said nothing visible and exist only to carry tool
 * calls. They belong inside the group rather than breaking it into pieces.
 * A silent turn with no calls (cut off while reasoning, say) is not one of
 * these: its bubble explains what happened, so it must stay visible. */
function isSilentAssistant(m: AgentMessage): boolean {
  return (
    m.type === 'assistant' &&
    !m.errorMessage &&
    m.content.some((c) => c.type === 'toolCall') &&
    contentToText(m.content).trim().length === 0
  )
}

/** Folds consecutive tool activity into groups.
 *
 * A group is a maximal run of tool results and silent assistant turns that
 * holds at least two tool results; a lone call keeps its own card, since a
 * heading over one item only adds a click. The message being streamed is
 * never folded: while it streams it has no text yet, and hiding the
 * "thinking" indicator inside a collapsed group would make a working agent
 * look idle. */
export function groupTranscript(
  messages: AgentMessage[],
  streamingIndex: number | null,
  checkpointAt: ReadonlySet<number> = new Set(),
): TranscriptItem[] {
  const items: TranscriptItem[] = []
  let run: number[] = []

  const flush = () => {
    const toolIndices = run.filter((i) => messages[i].type === 'toolResult')
    if (toolIndices.length >= 2) {
      items.push({ kind: 'group', start: run[0], indices: run, toolIndices })
    } else {
      for (const index of run) items.push({ kind: 'message', index })
    }
    run = []
  }

  messages.forEach((m, i) => {
    const foldable =
      i !== streamingIndex && ((m.type === 'toolResult' && !STANDALONE_TOOLS.has(m.toolName)) || isSilentAssistant(m))
    if (foldable) run.push(i)
    else {
      flush()
      items.push({ kind: 'message', index: i })
    }
    // A checkpoint is never folded away: it closes the group before it, so
    // progress reads as "worked… ✓ done… worked… ✓ done".
    if (checkpointAt.has(i)) {
      flush()
      items.push({ kind: 'checkpoint', index: i })
    }
  })
  flush()
  return items
}

type Family = 'read' | 'search' | 'edit' | 'run' | 'web' | 'screen' | 'delegate' | 'media' | 'other'

const FAMILY: Record<string, Family> = {
  read: 'read',
  ls: 'read',
  inspect_document: 'read',
  repo_overview: 'read',
  notebook_read: 'read',
  grep: 'search',
  glob: 'search',
  code_search: 'search',
  recall: 'search',
  write: 'edit',
  edit: 'edit',
  apply_patch: 'edit',
  notebook_edit: 'edit',
  edit_document: 'edit',
  create_document: 'edit',
  bash: 'run',
  powershell: 'run',
  run_script: 'run',
  sandbox_execute: 'run',
  terminal_open: 'run',
  terminal_send: 'run',
  math: 'run',
  web_search: 'web',
  web_fetch: 'web',
  web_preview: 'web',
  dip: 'screen',
  computer: 'screen',
  task: 'delegate',
  generate_image: 'media',
  generate_video: 'media',
  generate_audio: 'media',
  generate_3d: 'media',
  present_media: 'media',
}

function familyOf(name: string): Family {
  if (name in FAMILY) return FAMILY[name]
  if (name.startsWith('browser_')) return 'web'
  if (name.startsWith('terminal_')) return 'run'
  return 'other'
}

const PLAN_TOOLS = new Set(['todo_write', 'todo_write_rich'])

/** Tools whose result is its own view, and stays visible rather than folded
 * into a group: a team at work is the point of the turn. */
const STANDALONE_TOOLS = new Set(['team_run'])

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`

function pathOf(m: ToolResultMessage): string | null {
  const a = m.arguments
  if (!a) return null
  for (const k of ['path', 'file_path', 'notebook_path']) if (typeof a[k] === 'string') return a[k] as string
  return null
}

/** "Read 4 files · Edited 2 files · Ran 3 commands" — what a finished group
 * did, in the order a reader cares about it: changes first, then evidence. */
export function summarizeTools(tools: ToolResultMessage[]): string {
  const by = new Map<Family, ToolResultMessage[]>()
  // Plan updates are bookkeeping, and show up as checkpoints on their own.
  for (const t of tools.filter((t) => !PLAN_TOOLS.has(t.toolName))) {
    const f = familyOf(t.toolName)
    by.set(f, [...(by.get(f) ?? []), t])
  }
  const distinctPaths = (list: ToolResultMessage[]) => {
    const paths = new Set(list.map(pathOf).filter((p): p is string => !!p))
    return paths.size || list.length
  }

  const parts: string[] = []
  const edit = by.get('edit')
  if (edit) parts.push(`Edited ${plural(distinctPaths(edit), 'file', 'files')}`)
  const run = by.get('run')
  if (run) parts.push(`Ran ${plural(run.length, 'command', 'commands')}`)
  const media = by.get('media')
  if (media) parts.push(`Generated ${plural(media.length, 'item', 'items')}`)
  const delegate = by.get('delegate')
  if (delegate) parts.push(`Delegated ${plural(delegate.length, 'task', 'tasks')}`)
  const read = by.get('read')
  if (read) parts.push(`Read ${plural(distinctPaths(read), 'file', 'files')}`)
  const search = by.get('search')
  if (search) parts.push(plural(search.length, 'search', 'searches'))
  const web = by.get('web')
  if (web) parts.push(`${plural(web.length, 'web step', 'web steps')}`)
  const screen = by.get('screen')
  if (screen) parts.push(`${plural(screen.length, 'screen action', 'screen actions')}`)
  const other = by.get('other')
  if (other) parts.push(`${plural(other.length, 'other step', 'other steps')}`)
  return parts.join(' · ')
}
