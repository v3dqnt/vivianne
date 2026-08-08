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

export type AgentMessage =
  | { type: 'user'; content: MessageContent[]; timestamp: number }
  | {
      type: 'assistant'
      content: MessageContent[]
      stopReason: string
      errorMessage: string | null
      timestamp: number
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

/** `#[serde(tag = "type", content = "payload", rename_all = "snake_case")]` */
export type AgentEvent =
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
