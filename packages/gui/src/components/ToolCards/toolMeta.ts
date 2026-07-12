"use client";

import { cn } from "@/lib/utils";
import type { IconSvgElement } from "@hugeicons/react";
import {
  CommandLineIcon,
  CommandIcon,
  File01Icon,
  FileAddIcon,
  FileEditIcon,
  PencilEdit02Icon,
  Folder02Icon,
  FolderSearchIcon,
  SearchAreaIcon,
  Search01Icon,
  GlobeIcon,
  Globe02Icon,
  Clock01Icon,
  CheckListIcon,
  ComputerIcon,
  ToolsIcon,
  Github01Icon,
  FolderLibraryIcon,
  UserGroupIcon,
  AiBrain02Icon,
  AiImageIcon,
} from "@hugeicons/core-free-icons";

/**
 * Per-tool visual identity. Each tool defines:
 * - label        Human-friendly title shown in the card header
 * - icon         Hugeicons icon definition (pass to <HugeiconsIcon icon={...} />)
 * - accent       Tailwind text color for the title (on the dark card)
 * - accentSoft   Slightly softer variant, used for the arg-summary line
 * - dotBg        Tailwind background for the small status dot when running/success
 * - description  Compact description for argument preview lines
 */
export interface ToolMeta {
  label: string;
  icon: IconSvgElement;
  accent: string;
  accentSoft: string;
  dotBg: string;
  description: string;
}

export const TOOL_META: Record<string, ToolMeta> = {
  bash: {
    label: "Terminal",
    icon: CommandLineIcon,
    accent: "text-emerald-700",
    accentSoft: "text-emerald-600",
    dotBg: "bg-emerald-500",
    description: "Run a shell command",
  },
  read: {
    label: "Read",
    icon: File01Icon,
    accent: "text-sky-700",
    accentSoft: "text-sky-600",
    dotBg: "bg-sky-500",
    description: "Read file",
  },
  write: {
    label: "Write",
    icon: FileAddIcon,
    accent: "text-violet-700",
    accentSoft: "text-violet-600",
    dotBg: "bg-violet-500",
    description: "Write file",
  },
  edit: {
    label: "Edit",
    icon: PencilEdit02Icon,
    accent: "text-amber-700",
    accentSoft: "text-amber-600",
    dotBg: "bg-amber-500",
    description: "Edit file",
  },
  ls: {
    label: "List",
    icon: Folder02Icon,
    accent: "text-ink",
    accentSoft: "text-body",
    dotBg: "bg-mute",
    description: "List directory",
  },
  find: {
    label: "Find",
    icon: SearchAreaIcon,
    accent: "text-fuchsia-700",
    accentSoft: "text-fuchsia-600",
    dotBg: "bg-fuchsia-500",
    description: "Find files",
  },
  grep: {
    label: "Grep",
    icon: Search01Icon,
    accent: "text-rose-700",
    accentSoft: "text-rose-600",
    dotBg: "bg-rose-500",
    description: "Search files",
  },
  glob: {
    label: "Glob",
    icon: FolderSearchIcon,
    accent: "text-fuchsia-700",
    accentSoft: "text-fuchsia-600",
    dotBg: "bg-fuchsia-500",
    description: "Find files by pattern",
  },
  powershell: {
    label: "PowerShell",
    icon: CommandIcon,
    accent: "text-blue-700",
    accentSoft: "text-blue-600",
    dotBg: "bg-blue-500",
    description: "Run a PowerShell command",
  },
  notebook_edit: {
    label: "Notebook",
    icon: FileEditIcon,
    accent: "text-amber-700",
    accentSoft: "text-amber-600",
    dotBg: "bg-amber-500",
    description: "Edit a notebook cell",
  },
  web_fetch: {
    label: "Fetch",
    icon: GlobeIcon,
    accent: "text-sky-700",
    accentSoft: "text-sky-600",
    dotBg: "bg-sky-500",
    description: "Fetch a URL",
  },
  web_search: {
    label: "Search",
    icon: Globe02Icon,
    accent: "text-teal-700",
    accentSoft: "text-teal-600",
    dotBg: "bg-teal-500",
    description: "Search the web",
  },
  sleep: {
    label: "Sleep",
    icon: Clock01Icon,
    accent: "text-body",
    accentSoft: "text-mute",
    dotBg: "bg-mute",
    description: "Wait",
  },
  todo_write: {
    label: "Todos",
    icon: CheckListIcon,
    accent: "text-violet-700",
    accentSoft: "text-violet-600",
    dotBg: "bg-violet-500",
    description: "Update the task list",
  },
  repo_clone: {
    label: "Repo Clone",
    icon: Github01Icon,
    accent: "text-slate-700",
    accentSoft: "text-slate-600",
    dotBg: "bg-slate-500",
    description: "Clone a remote repository",
  },
  repo_overview: {
    label: "Repo Overview",
    icon: FolderLibraryIcon,
    accent: "text-slate-700",
    accentSoft: "text-slate-600",
    dotBg: "bg-slate-500",
    description: "Inspect a repository",
  },
  computer: {
    label: "Computer",
    icon: ComputerIcon,
    accent: "text-indigo-700",
    accentSoft: "text-indigo-600",
    dotBg: "bg-indigo-500",
    description: "Control the computer",
  },
  code_search: {
    label: "Code Search",
    icon: AiBrain02Icon,
    accent: "text-rose-700",
    accentSoft: "text-rose-600",
    dotBg: "bg-rose-500",
    description: "Search the codebase by relevance",
  },
  delegate: {
    label: "Subagent",
    icon: UserGroupIcon,
    accent: "text-teal-700",
    accentSoft: "text-teal-600",
    dotBg: "bg-teal-500",
    description: "Delegated subtask",
  },
  generate_image: {
    label: "Generate Image",
    icon: AiImageIcon,
    accent: "text-pink-700",
    accentSoft: "text-pink-600",
    dotBg: "bg-pink-500",
    description: "Generate an image from a prompt",
  },
};

export function getToolMeta(name: string | undefined): ToolMeta {
  const key = (name ?? "").toLowerCase();
  if (TOOL_META[key]) return TOOL_META[key];
  return {
    label: name ?? "Tool",
    icon: name && /browser|http|web/i.test(name) ? GlobeIcon : ToolsIcon,
    accent: "text-ink",
    accentSoft: "text-body",
    dotBg: "bg-mute",
    description: name ?? "Tool call",
  };
}

/**
 * Build a short, single-line preview of the tool args. Used in the card header
 * so the user sees what was called without expanding the args.
 */
export function summarizeArgs(toolName: string | undefined, args: any): string {
  const a = args && typeof args === "object" ? args : {};
  switch ((toolName ?? "").toLowerCase()) {
    case "bash":
    case "powershell":
      return (a.command ?? "").toString();
    case "read":
    case "write":
    case "edit":
      return (a.path ?? "").toString();
    case "ls":
      return (a.path ?? ".").toString();
    case "grep":
    case "find":
    case "glob":
      return `${a.pattern ?? ""}${a.path ? `  in  ${a.path}` : ""}`;
    case "notebook_edit":
      return `${a.edit_mode ?? "replace"}  ${a.notebook_path ?? ""}`.trim();
    case "web_fetch":
      return (a.url ?? "").toString();
    case "web_search":
      return (a.query ?? "").toString();
    case "sleep":
      return a.seconds != null ? `${a.seconds}s` : "";
    case "todo_write":
      return Array.isArray(a.todos) ? `${a.todos.length} item${a.todos.length === 1 ? "" : "s"}` : "";
    case "computer":
      return `${a.action ?? ""}${a.coordinate ? ` [${a.coordinate.join(", ")}]` : ""}${a.text ? ` "${a.text}"` : ""}`.trim();
    case "repo_clone":
      return (a.repository ?? "").toString();
    case "repo_overview":
      return (a.repository ?? a.path ?? "").toString();
    case "code_search":
      return `${a.query ?? ""}${a.path ? `  in  ${a.path}` : ""}`;
    case "delegate":
      return (a.task ?? "").toString();
    case "generate_image":
      return `${a.prompt ?? ""}${a.size ? `  [${a.size}]` : ""}`;
    default:
      return a.path ?? a.command ?? a.pattern ?? a.url ?? a.query ?? "";
  }
}

/**
 * Returns the rendered args as ordered [key, value] pairs, with sensible
 * per-tool filtering so we don't show empty fields.
 */
export function renderArgsRows(
  toolName: string | undefined,
  args: any,
): Array<{ key: string; value: string }> {
  const a = args && typeof args === "object" ? args : {};
  const rows: Array<{ key: string; value: string }> = [];
  const push = (key: string, value: unknown) => {
    if (value === undefined || value === null || value === "") return;
    rows.push({ key, value: typeof value === "string" ? value : JSON.stringify(value) });
  };

  switch ((toolName ?? "").toLowerCase()) {
    case "bash":
      push("command", a.command);
      break;
    case "read":
      push("path", a.path);
      push("offset", a.offset);
      push("limit", a.limit);
      break;
    case "write":
      push("path", a.path);
      push("bytes", typeof a.content === "string" ? a.content.length : undefined);
      break;
    case "edit":
      push("path", a.path);
      push("old_string", a.old_string);
      push("new_string", a.new_string);
      break;
    case "ls":
      push("path", a.path);
      break;
    case "grep":
      push("pattern", a.pattern);
      push("path", a.path);
      break;
    case "find":
      push("pattern", a.pattern);
      push("path", a.path);
      break;
    default:
      push("path", a.path);
      push("command", a.command);
      push("pattern", a.pattern);
      break;
  }
  return rows;
}

/**
 * Tiny helpers shared by the per-tool result renderers.
 */
export function statusColor(tone: "running" | "success" | "error") {
  switch (tone) {
    case "running":
      return { ring: "ring-sky-500/40", bg: "bg-sky-500", text: "text-sky-500" };
    case "error":
      return { ring: "ring-rose-500/40", bg: "bg-rose-500", text: "text-rose-500" };
    case "success":
      return { ring: "ring-emerald-500/40", bg: "bg-emerald-500", text: "text-emerald-500" };
  }
}

export function toneFor(status: "running" | "success" | "error" | "pending" | undefined): "running" | "success" | "error" {
  if (status === "error") return "error";
  if (status === "success") return "success";
  return "running";
}

export function cn2(...inputs: Array<string | false | null | undefined>): string {
  return cn(...inputs);
}
