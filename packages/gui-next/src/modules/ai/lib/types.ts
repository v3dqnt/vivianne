/**
 * Mirrors of the Rust wire types (`agent::types::{AgentMessage, MessageContent}`,
 * `agent::events::AgentEvent`). Kept as a single typed source instead of `any`
 * scattered through components — a backend field rename becomes a type error
 * here, not a silently-blank UI somewhere downstream.
 */

export type MessageContent =
  | { type: 'text'; text: string }
  | { type: 'image'; url: string | null; base64: string | null; mediaType: string | null }
  | { type: 'thinking'; thinking: string }
  | { type: 'toolCall'; id: string; name: string; arguments: Record<string, unknown> }
  | {
      type: 'toolResult'
      content: MessageContent[]
      details: unknown
      isError: boolean | null
    }

/** `ai::types::Usage` — `#[serde(rename_all = "camelCase")]`. */
export interface Usage {
  input: number
  output: number
  cacheRead: number
  cacheWrite: number
  totalTokens: number
  cost: { input: number; output: number; cacheRead: number; cacheWrite: number; total: number } | null
}

export type AgentMessage =
  | { type: 'user'; content: MessageContent[]; timestamp: number }
  | {
      type: 'assistant'
      content: MessageContent[]
      stopReason: string
      errorMessage: string | null
      timestamp: number
      /** Only present on the final message of a turn — absent on streaming updates and synthetic system messages. */
      usage?: Usage | null
      /** Frontend-computed only (not on the wire): `usage.output` divided by
       * message-start-to-end wall-clock time. Set once, on `message_end`. */
      tokensPerSec?: number
    }
  | {
      type: 'toolResult'
      toolCallId: string
      toolName: string
      content: MessageContent[]
      details: unknown
      isError: boolean | null
      timestamp: number
      arguments?: Record<string, unknown>
    }

export interface AgentToolResult {
  content: MessageContent[]
  details: unknown
  terminate: boolean | null
}

/** `agent::events::QuestionOption` — `#[serde(rename_all = "camelCase")]`. */
export interface QuestionOption {
  label: string
  description: string
}

/** `agent::events::QuestionSpec` — `#[serde(rename_all = "camelCase")]`. */
export interface QuestionSpec {
  question: string
  header: string
  options: QuestionOption[]
  multiSelect: boolean
}

/** `agent::run_ledger::RunStatus`. */
export type RunStatus = 'running' | 'waiting' | 'paused' | 'blocked' | 'done' | 'stopped'

/** `agent::run_ledger::Milestone` — a finished item of a long task's plan. */
export interface Milestone {
  title: string
  summary: string | null
  at: number
  spentUsd: number
  elapsedMs: number
}

/** `agent::run_ledger::RunLedger` — `#[serde(rename_all = "camelCase")]`. */
export interface RunLedger {
  key: string
  thread: string
  workspace: string | null
  objective: string
  status: RunStatus
  capUsd: number | null
  deadlineMs: number | null
  startedAt: number
  updatedAt: number
  spentUsd: number
  steps: number
  model: string | null
  waitUntil: number | null
  statusReason: string | null
  milestones: Milestone[]
  /** `(title, status)` pairs. */
  todos: [string, string][]
}

/** `#[serde(tag = "type", content = "payload", rename_all = "snake_case")]` */
export type AgentEvent =
  | { type: 'memory_injected'; payload: { entry_count: number } }
  | { type: 'agent_start' }
  | { type: 'agent_end'; payload: { messages: AgentMessage[]; stats: unknown; cost: unknown } }
  | { type: 'turn_start' }
  | {
      type: 'turn_end'
      payload: { message: AgentMessage; tool_results: AgentMessage[]; stats: unknown; cost: unknown }
    }
  | { type: 'message_start'; payload: { message: AgentMessage } }
  | { type: 'message_update'; payload: { message: AgentMessage; chunk: string } }
  | { type: 'message_end'; payload: { message: AgentMessage } }
  | {
      type: 'tool_execution_start'
      payload: { tool_call_id: string; tool_name: string; args: unknown }
    }
  | {
      type: 'tool_execution_end'
      payload: { tool_call_id: string; tool_name: string; result: AgentToolResult; is_error: boolean }
    }
  | {
      type: 'permission_asked'
      payload: { request_id: string; tool_call_id: string; tool_name: string; args: unknown; reason: string | null }
    }
  | { type: 'permission_resolved'; payload: { request_id: string; approved: boolean } }
  | {
      type: 'question_asked'
      payload: { request_id: string; tool_call_id: string; questions: QuestionSpec[] }
    }
  | { type: 'question_resolved'; payload: { request_id: string } }
  | {
      type: 'confirmation_asked'
      payload: { request_id: string; tool_call_id: string; message: string; danger_level: string }
    }
  | { type: 'confirmation_resolved'; payload: { request_id: string; approved: boolean } }
  | { type: 'retry_status'; payload: { attempt: number; max_attempts: number; delay_ms: number; reason: string } }
  | {
      type: 'model_fallback'
      payload: { from_provider: string; from_model: string; to_provider: string; to_model: string; reason: string }
    }
  | { type: 'run_waiting'; payload: { until_ms: number; reason: string } }
  | { type: 'long_task'; payload: { ledger: RunLedger } }
  | {
      type: 'credential_rotated'
      payload: { provider: string; from_label: string; to_label: string; reason: string }
    }
  | {
      type: 'web_preview_requested'
      payload: { url: string; screenshot_base64: string | null; error: string | null }
    }
  | {
      type: 'panel_focus_requested'
      payload: { panel: string; action: string }
    }

/** Flatten an assistant/user message's content blocks to plain text for display. */
export function contentToText(content: MessageContent[]): string {
  return content
    .map((c) => {
      if (c.type === 'text') return c.text
      if (c.type === 'thinking') return ''
      return ''
    })
    .join('')
}

/** Concatenate every `thinking` block — model reasoning shown separately from the reply text. */
export function contentToThinking(content: MessageContent[]): string {
  return content
    .filter((c): c is Extract<MessageContent, { type: 'thinking' }> => c.type === 'thinking')
    .map((c) => c.thinking)
    .join('')
}

/** Every `image` block with actual data — attachments on a user message, or a tool's image output (screenshots, generated images). */
export function contentImages(content: MessageContent[]): Extract<MessageContent, { type: 'image' }>[] {
  return content.filter((c): c is Extract<MessageContent, { type: 'image' }> => c.type === 'image' && (c.base64 !== null || c.url !== null))
}

/** `ai::model_discovery::DiscoveredModel` — `#[serde(rename_all = "camelCase")]`. */
export interface DiscoveredModel {
  id: string
  name: string
  provider: string
  api: string
  contextWindow: number | null
  maxOutputTokens: number | null
  inputCostPerMillion: number | null
  outputCostPerMillion: number | null
  isImageModel: boolean
  isAudioModel: boolean
  supportsVision: boolean
}

/** `agent::commands::SessionStats` — `#[serde(rename_all = "camelCase")]`. */
export interface SessionStats {
  contextTokens: number
  contextWindow: number
  contextPercent: number
  messageCount: number
  inputTokens: number
  outputTokens: number
  cacheRead: number
  cacheWrite: number
  totalTokens: number
  modelId: string
  modelName: string
  provider: string
  maxTokens: number
  breakdown: ContextBreakdown
}

/** `agent::commands::ContextBreakdown` — where the context window is going.
 * Same char/4 estimate as `contextTokens`, so the parts sum to the whole. */
export interface ContextBreakdown {
  systemPrompt: number
  toolSchemas: number
  user: number
  assistant: number
  toolResults: number
}

export const THINKING_LEVELS = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh'] as const
export type ThinkingLevel = (typeof THINKING_LEVELS)[number]

/**
 * Base64 image attachment sent up with a prompt — mirrors
 * `agent::commands::ImageInput`, which has no `rename_all` (unlike
 * `MessageContent`), so the wire field is `media_type`, not `mediaType`.
 */
export interface ImageAttachment {
  base64: string
  media_type: string
}

/**
 * A file context chip — read client-side via the browser `File` API when
 * attached, formatted into the outgoing message text by `Composer` (there's
 * no dedicated wire type for this on the Rust side; it rides along as plain
 * text the same way image-attachment counts get woven into the prompt).
 */
export interface FileAttachment {
  name: string
  content: string
  truncated: boolean
}
