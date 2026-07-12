import type { IconSvgElement } from "@hugeicons/react";
import {
  Settings02Icon,
  CpuIcon,
  PlusSignIcon,
  Copy01Icon,
  PencilEdit02Icon,
  InformationCircleIcon,
  Layers01Icon,
  GitForkIcon,
  FlowIcon,
  Clock01Icon,
  RefreshIcon,
  Download01Icon,
  ComputerIcon,
} from "@hugeicons/core-free-icons";

/**
 * Where a command runs. Ported from viv-main's `BUILTIN_SLASH_COMMANDS`
 * (packages/coding-agent/src/core/slash-commands.ts), adapted for the GUI:
 *
 * - "action":  handled entirely in the frontend (calls a context fn / navigates).
 * - "compose": becomes a highlighted prefix chip in the composer. The user types
 *              their instruction after it (e.g. `/computer open terminal`) and the
 *              whole thing is sent as a structured command the agent understands.
 */
export type SlashCommandKind = "action" | "compose";

export interface SlashCommand {
  name: string;
  description: string;
  icon: IconSvgElement;
  kind: SlashCommandKind;
  /** For kind: "compose" — placeholder shown after the chip. */
  argHint?: string;
  /** For kind: "compose" — turn the typed argument into the sent message. */
  buildMessage?: (arg: string) => string;
}

export const SLASH_COMMANDS: SlashCommand[] = [
  { name: "new", description: "Start a new session", icon: PlusSignIcon, kind: "action" },
  { name: "settings", description: "Open settings menu", icon: Settings02Icon, kind: "action" },
  { name: "model", description: "Select the active model", icon: CpuIcon, kind: "action" },
  { name: "copy", description: "Copy the last agent message", icon: Copy01Icon, kind: "action" },
  { name: "name", description: "Set the session display name", icon: PencilEdit02Icon, kind: "action" },
  { name: "session", description: "Show session info and stats", icon: InformationCircleIcon, kind: "action" },
  { name: "compact", description: "Manually compact the session context", icon: Layers01Icon, kind: "action" },
  { name: "fork", description: "Fork the current session", icon: GitForkIcon, kind: "action" },
  { name: "clone", description: "Duplicate the current session", icon: Copy01Icon, kind: "action" },
  { name: "tree", description: "Navigate the session tree", icon: FlowIcon, kind: "action" },
  { name: "resume", description: "Resume a different session", icon: Clock01Icon, kind: "action" },
  { name: "reload", description: "Reload skills, prompts, and settings", icon: RefreshIcon, kind: "action" },
  { name: "export", description: "Export the session to a file", icon: Download01Icon, kind: "action" },
  {
    name: "computer",
    description: "Let the agent control the computer (mouse, keyboard, screenshots)",
    icon: ComputerIcon,
    kind: "compose",
    argHint: "open the terminal, click the sidebar, type…",
    buildMessage: (arg) => `/computer ${arg}`.trim(),
  },
];

/** Filter commands by the text typed after the leading slash. */
export function filterSlashCommands(query: string): SlashCommand[] {
  const q = query.trim().toLowerCase();
  if (!q) return SLASH_COMMANDS;
  return SLASH_COMMANDS.filter(
    (c) => c.name.toLowerCase().includes(q) || c.description.toLowerCase().includes(q)
  );
}

/**
 * Detects a slash-command trigger: the input must start with `/` and contain no
 * whitespace yet (i.e. the user is still typing the command name, not arguments).
 * Returns the partial command name, or null when not in trigger state.
 */
export function getSlashQuery(value: string): string | null {
  if (!value.startsWith("/")) return null;
  const rest = value.slice(1);
  if (/\s/.test(rest)) return null;
  return rest;
}

/**
 * Detects an in-progress `@skill` mention being typed at the very end of the
 * input (the composer is a plain textarea, not a rich cursor-aware editor,
 * so — like slash commands — this only triggers for the token currently
 * being typed at the tail, not an arbitrary mid-text cursor position).
 * Requires the `@` to be preceded by whitespace or the start of the string
 * so it doesn't fire mid-word or on something like an email address.
 * Returns the partial name after `@`, or null when not in trigger state.
 */
export function getSkillMentionQuery(value: string): string | null {
  const match = value.match(/(^|\s)@([A-Za-z0-9_-]*)$/);
  return match ? match[2] : null;
}

/** Replace the trailing in-progress `@partial` with a completed `@name `,
 * preserving whatever preceded it (start-of-string or the whitespace char). */
export function applySkillMention(value: string, skillName: string): string {
  return value.replace(/(^|\s)@([A-Za-z0-9_-]*)$/, (_full, lead: string) => `${lead}@${skillName} `);
}
