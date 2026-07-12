// Shared frontend types

export interface ToolArgs {
  path?: string;
  command?: string;
  pattern?: string;
  offset?: number | string;
  limit?: number | string;
  content?: string;
  old_string?: string;
  new_string?: string;
  query?: string;
  task?: string;
  prompt?: string;
  size?: string;
}

export interface AgentImage {
  base64: string;
  mediaType: string;
}

export interface AgentMessage {
  id: string;
  role: "user" | "assistant" | "system" | "toolResult" | "toolCall";
  content: any;
  isStreaming?: boolean;
  toolName?: string;
  toolCallId?: string;
  isError?: boolean;
  args?: ToolArgs | null;
  details?: any;
  /** For `toolCall` (assistant) messages — the parsed args the model sent */
  status?: "running" | "success" | "error" | "pending";
  /** Images attached to a user message, or extracted from an assistant/tool
   * message's content blocks (e.g. a screenshot the model analysed). */
  images?: AgentImage[];
}

export interface ModelInfo {
  id: string;
  name: string;
  provider: string;
  displayName: string;
  api: string;
}
