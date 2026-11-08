/**
 * Slash commands in the composer: `/plan`, `/permission auto`, `/goal ship the release`.
 *
 * A command is a name, a one-line description, and a function that runs against a small context the composer supplies, so the registry has no
 * knowledge of React or the stores and can be tested alone. A message that starts with `/name` for a known command runs it instead of being sent;
 * anything else (an unknown `/word`, a path like `/etc/hosts`) is an ordinary message.
 */

import { rankByName } from './rankByName.ts'

export interface SlashContext {
  /** Turn plan mode on or off. */
  setPlanMode(on: boolean): Promise<void>
  /** Auto (no approval prompts) or ask. */
  setAutoApprove(on: boolean): Promise<void>
  /** What the permission mode is now. */
  mode(): 'ask' | 'auto' | 'plan'
  /** A goal action on this thread; `text` for create and edit. */
  goal(action: 'create' | 'edit' | 'pause' | 'resume' | 'complete' | 'clear', text?: string): Promise<void>
  /** The thread's goal as a line of text, or null when it has none. */
  currentGoal(): Promise<string | null>
  /** Stop the running turn. */
  stop(): Promise<void>
  /** Open the model picker. */
  openModelPicker(): void
  /** Say something back to the user without sending it to the model. */
  notice(text: string): void
}

export interface SlashCommand {
  name: string
  description: string
  /** Shown after the name while typing, e.g. `<objective>`. A command with no `args` runs as soon as it is chosen. */
  args?: string
  run(ctx: SlashContext, arg: string): Promise<void> | void
}

const PERMISSION_MODES = ['ask', 'auto', 'plan'] as const

export const SLASH_COMMANDS: readonly SlashCommand[] = [
  {
    name: 'plan',
    description: 'Research first and propose a plan before changing anything',
    args: '[on|off]',
    async run(ctx, arg) {
      const on = arg === 'off' ? false : arg === '' || arg === 'on' ? ctx.mode() !== 'plan' || arg === 'on' : null
      if (on === null) return ctx.notice('Use /plan, /plan on or /plan off.')
      await ctx.setPlanMode(on)
      ctx.notice(on ? 'Plan mode is on.' : 'Plan mode is off.')
    },
  },
  {
    name: 'permission',
    description: 'Ask before tools run, or let the agent run them (ask, auto, plan)',
    args: '<ask|auto|plan>',
    async run(ctx, arg) {
      const mode = PERMISSION_MODES.find((m) => m === arg)
      if (!mode) return ctx.notice(`Current mode: ${ctx.mode()}. Use /permission ask, auto or plan.`)
      if (mode === 'plan') await ctx.setPlanMode(true)
      else {
        if (ctx.mode() === 'plan') await ctx.setPlanMode(false)
        await ctx.setAutoApprove(mode === 'auto')
      }
      ctx.notice(`Permission mode: ${mode}.`)
    },
  },
  {
    name: 'goal',
    description: 'Set a goal for this thread, or pause, resume, finish or clear it',
    args: '<objective> | pause | resume | done | clear',
    async run(ctx, arg) {
      const word = arg.toLowerCase()
      if (word === 'pause' || word === 'resume' || word === 'clear') return ctx.goal(word)
      if (word === 'done' || word === 'complete') return ctx.goal('complete')
      if (!arg) {
        const now = await ctx.currentGoal()
        return ctx.notice(now ? `Goal: ${now}. Change it with /goal <objective>, or pause, resume, done, clear.` : 'No goal yet. Set one with /goal <objective>.')
      }
      try {
        await ctx.goal('create', arg)
      } catch {
        // A goal already exists: this edits it instead.
        await ctx.goal('edit', arg)
      }
    },
  },
  { name: 'model', description: 'Choose the model', run: (ctx) => ctx.openModelPicker() },
  { name: 'stop', description: 'Stop the agent now', run: (ctx) => ctx.stop() },
]

export interface SlashRange {
  /** Index of the `/`. */
  start: number
  /** End of the command word the cursor is in. */
  end: number
  query: string
}

/** The command word being typed: a `/` at the start of the message, with the cursor still inside the word. */
export function slashAt(value: string, cursor: number = value.length): SlashRange | null {
  const m = /^\/([A-Za-z0-9_-]*)/.exec(value)
  if (!m) return null
  const end = m[0].length
  if (cursor < 1 || cursor > end) return null
  return { start: 0, end, query: m[1] }
}

/** The commands a typed word could mean, best first. */
export function matchCommands(query: string, commands: readonly SlashCommand[] = SLASH_COMMANDS): SlashCommand[] {
  return rankByName(commands, query, (c) => c.name)
}

/** `/name rest` where `name` is a registered command; null for anything else. */
export function parseSlash(text: string, commands: readonly SlashCommand[] = SLASH_COMMANDS): { command: SlashCommand; arg: string } | null {
  const m = /^\/([A-Za-z0-9_-]+)(?:\s+([\s\S]*))?$/.exec(text.trim())
  if (!m) return null
  const command = commands.find((c) => c.name === m[1].toLowerCase())
  return command ? { command, arg: (m[2] ?? '').trim() } : null
}
