/**
 * Wire types shared with the Rust backend's provider bridge. Hand-verified
 * field-by-field against `ai/types.rs` (all `#[serde(rename_all = "camelCase")]`
 * except the tagged `MessageContent` union, which uses `tag = "type"`) — a
 * backend rename should be a TS type error here, not a silent blank reply.
 */

export interface ToolCall {
  id: string
  name: string
  arguments: unknown
  thoughtSignature: string | null
}

export type MessageContent =
  | { type: 'text'; text: string }
  | { type: 'image'; url: string | null; base64: string | null; mediaType: string | null }
  | { type: 'thinking'; thinking: string; thinkingSignature: string | null; redacted: boolean }
  | ({ type: 'toolCall' } & ToolCall)
  | { type: 'toolResult'; toolCallId: string; content: MessageContent[]; isError: boolean | null }

/** `ai::types::Message` — the provider-agnostic message shape inside `Context`. */
export interface Message {
  role: string
  contentBlocks: MessageContent[]
  text: string | null
  toolCallId: string | null
  toolName: string | null
  isError: boolean | null
  timestamp: number | null
  provider: string | null
  api: string | null
  modelId: string | null
  stopReason: string | null
  errorMessage: string | null
  usage: Usage | null
  diagnostics: unknown[] | null
}

export interface CostBreakdown {
  input: number
  output: number
  cacheRead: number
  cacheWrite: number
  total: number
}

export interface Usage {
  input: number
  output: number
  cacheRead: number
  cacheWrite: number
  totalTokens: number
  cost: CostBreakdown | null
}

export interface Context {
  messages: Message[]
  systemPrompt: string | null
  tools: unknown[] | null
}

export interface AssistantMessage {
  role: string
  content: MessageContent[]
  api: string
  provider: string
  model: string
  usage: Usage
  stopReason: string
  errorMessage: string | null
  timestamp: number
  diagnostics: unknown[] | null
}

/** Emitted by the TS provider dispatcher, consumed by Rust's `call_provider_stream_and_await` (unchanged on the Rust side). */
export interface StreamTokenEvent {
  streamId: string
  delta: string
  done: boolean
  message: AssistantMessage | null
  isThinking: boolean
}

/** Emitted by Rust, consumed by the TS dispatcher — everything needed to make the actual provider call. */
export interface ProviderStreamRequest {
  streamId: string
  apiKind:
    | 'anthropicMessages'
    | 'openAiChat'
    | 'openAiResponses'
    | 'googleGenerative'
    | 'mistralChat'
    | 'bedrock'
    | 'azureOpenAiResponses'
  baseUrl: string
  modelId: string
  provider: string
  apiKey: string | null
  maxTokens: number
  context: Context
  thinkingEnabled: boolean
  thinkingBudgetTokens: number
  /** Pre-built headers to send as-is (e.g. Bedrock's SigV4 signature, computed in Rust since it needs the AWS secret key). */
  extraHeaders: Record<string, string> | null
  /** Pre-serialized request body to send byte-for-byte as-is — required whenever `extraHeaders` includes a signature computed over the body. */
  rawBody: string | null
  /** The chosen thinking level: off, minimal, low, medium, high, xhigh. */
  thinkingLevel?: string
  /** Stable per conversation, for providers that route by a cache key. */
  cacheKey?: string | null
}

export function emptyUsage(): Usage {
  return { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: null }
}

export function now(): number {
  return Date.now()
}
