import { call } from '../../../ipc'
import { notifyFileChanged } from '../../ide/lib/fileChangeBus'
import { contentToText, type AgentMessage } from './types'
import { isHandoffText } from '../components/HandoffDivider'

/** Mirrors `agent::review::FileChange` (`rename_all = "camelCase"`). */
export interface DiffLine {
  kind: 'ctx' | 'add' | 'del'
  oldNo: number | null
  newNo: number | null
  text: string
}

export interface Hunk {
  oldStart: number
  newStart: number
  lines: DiffLine[]
}

export type ChangeStatus = 'added' | 'modified' | 'deleted' | 'unchanged'

export interface FileChange {
  path: string
  status: ChangeStatus
  additions: number
  deletions: number
  binary: boolean
  tooLarge: boolean
  truncated: boolean
  hunks: Hunk[]
  modifiedSince: boolean
}

export interface AssignmentChanges {
  files: FileChange[]
  possiblyIncomplete: boolean
}

export interface UndoReport {
  restored: string[]
  deleted: string[]
  skipped: [string, string][]
  failed: [string, string][]
  redoCheckpointId: string | null
}

/** Tools whose edits are checkpointed — mirrors `checkpoints::CHECKPOINTABLE`. */
const TRACKED_TOOLS = new Set(['write', 'edit', 'apply_patch', 'notebook_edit'])
/** Tools that can change files without the review seeing it — checked against
 * `tool_schemas.rs`: the two shells, scripts, and anything typed into a
 * terminal session. */
const SHELL_TOOLS = new Set(['bash', 'powershell', 'run_script', 'terminal_send'])

/**
 * One assignment: everything between a user message and the next.
 *
 * Derived from the transcript rather than recorded anywhere — message and
 * checkpoint timestamps come from the same backend clock, so the span
 * `[fromMs, toMs)` picks out exactly this assignment's checkpoints, today or
 * after a restart.
 */
export interface Assignment {
  fromMs: number
  /** The next user message's timestamp; null for the latest assignment. */
  toMs: number | null
  /** Index of the last message in the assignment — where its summary goes. */
  endIndex: number
  /** Whether it called any tool whose edits are tracked. No tracked tool
   * means no possible changes, so no summary and no query at all. */
  editedFiles: boolean
  /** Whether it ran shell commands, whose file changes the review can't see. */
  ranShell: boolean
}

export function findAssignments(messages: AgentMessage[]): Assignment[] {
  const starts: number[] = []
  messages.forEach((m, i) => {
    if (m.type === 'user' && !isHandoffText(contentToText(m.content))) starts.push(i)
  })
  return starts.map((start, k) => {
    const next = starts[k + 1]
    const end = next === undefined ? messages.length - 1 : next - 1
    let editedFiles = false
    let ranShell = false
    for (let i = start; i <= end; i++) {
      const m = messages[i]
      if (m.type !== 'assistant') continue
      for (const c of m.content) {
        if (c.type !== 'toolCall') continue
        if (TRACKED_TOOLS.has(c.name)) editedFiles = true
        if (SHELL_TOOLS.has(c.name) || (c.name === 'terminal' && c.arguments?.action === 'send')) ranShell = true
      }
    }
    const from = messages[start]
    return {
      fromMs: from.type === 'user' ? from.timestamp : 0,
      toMs: next === undefined ? null : (messages[next] as Extract<AgentMessage, { type: 'user' }>).timestamp,
      endIndex: end,
      editedFiles,
      ranShell,
    }
  })
}

export async function loadChanges(a: Pick<Assignment, 'fromMs' | 'toMs'>, sessionId?: string): Promise<AssignmentChanges | null> {
  return call<AssignmentChanges>('cmd_assignment_changes', {
    fromMs: a.fromMs,
    toMs: a.toMs,
    sessionId: sessionId ?? null,
  })
}

export async function undoFiles(a: Pick<Assignment, 'fromMs' | 'toMs'>, paths: string[]): Promise<UndoReport | null> {
  const report = await call<UndoReport>('cmd_undo_assignment', { fromMs: a.fromMs, toMs: a.toMs, paths })
  // Open editors were showing the version undo just replaced.
  if (report) for (const p of [...report.restored, ...report.deleted]) notifyFileChanged(p)
  return report
}

export async function redo(checkpointId: string, paths: string[]): Promise<void> {
  await call('cmd_restore_checkpoint', { checkpointId })
  for (const p of paths) notifyFileChanged(p)
}

/** A path as the user thinks of it: relative to the project, forward slashes. */
export function displayPath(path: string, root: string | null): string {
  const norm = (p: string) => p.replace(/^\\\\\?\\/, '').replace(/\\/g, '/')
  const p = norm(path)
  if (!root) return p
  const r = norm(root).replace(/\/+$/, '')
  // Windows paths are case-insensitive; a drive letter's case can differ
  // between how the workspace was picked and how a tool resolved the file.
  return p.toLowerCase().startsWith(r.toLowerCase() + '/') ? p.slice(r.length + 1) : p
}

export function totals(files: FileChange[]): { additions: number; deletions: number } {
  return files.reduce(
    (t, f) => ({ additions: t.additions + f.additions, deletions: t.deletions + f.deletions }),
    { additions: 0, deletions: 0 },
  )
}
