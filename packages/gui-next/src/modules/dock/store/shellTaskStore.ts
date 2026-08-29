import { create } from 'zustand'

/** One agent-run shell command (`bash` / `powershell` / `sandbox_execute`). */
export interface ShellTask {
  toolCallId: string
  tool: string
  command: string
  startedAt: number
  /** Unset while running. */
  endedAt?: number
  isError?: boolean
}

/** Tool names whose invocations show up as shell tasks. These run as one-shot
 * child processes rather than PTY sessions, which is exactly why the Background
 * Tasks panel couldn't see them: it lists `cmd_terminal_list_sessions`, and a
 * `bash` tool call never creates a session. Rather than adding a backend
 * registry, this reconstructs the list from the `tool_execution_start` /
 * `tool_execution_end` events the agent already emits — the data was already
 * flowing, nothing was reading it. */
const SHELL_TOOLS = new Set(['bash', 'powershell', 'sandbox_execute'])

export function isShellTool(name: string): boolean {
  return SHELL_TOOLS.has(name)
}

/** Finished commands worth keeping around so the panel shows recent history
 * rather than blanking the moment a command exits. */
const MAX_FINISHED = 30

interface ShellTaskState {
  tasks: ShellTask[]
  start: (toolCallId: string, tool: string, args: Record<string, unknown> | undefined) => void
  finish: (toolCallId: string, isError: boolean) => void
  clearFinished: () => void
}

/** Pulls the human-readable command out of whatever the tool's args look
 * like — `bash`/`powershell` use `command`, `sandbox_execute` uses `code`. */
function commandOf(args: Record<string, unknown> | undefined): string {
  if (!args) return ''
  const raw = args.command ?? args.code
  return typeof raw === 'string' ? raw : ''
}

export const useShellTaskStore = create<ShellTaskState>((set) => ({
  tasks: [],

  start: (toolCallId, tool, args) =>
    set((s) => ({
      tasks: [
        ...s.tasks,
        { toolCallId, tool, command: commandOf(args), startedAt: Date.now() },
      ],
    })),

  finish: (toolCallId, isError) =>
    set((s) => {
      const idx = s.tasks.findIndex((t) => t.toolCallId === toolCallId)
      if (idx === -1) return s
      const tasks = [...s.tasks]
      tasks[idx] = { ...tasks[idx], endedAt: Date.now(), isError }
      // Trim oldest finished entries only — a running command is never
      // dropped, however long it has been going.
      const finished = tasks.filter((t) => t.endedAt !== undefined)
      if (finished.length > MAX_FINISHED) {
        const drop = new Set(finished.slice(0, finished.length - MAX_FINISHED).map((t) => t.toolCallId))
        return { tasks: tasks.filter((t) => !drop.has(t.toolCallId)) }
      }
      return { tasks }
    }),

  clearFinished: () => set((s) => ({ tasks: s.tasks.filter((t) => t.endedAt === undefined) })),
}))
