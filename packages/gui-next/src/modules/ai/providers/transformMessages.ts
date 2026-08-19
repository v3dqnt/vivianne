/**
 * Message transformation pipeline — TS port of `ai/api/transform_messages.rs`
 * (kept byte-for-byte equivalent in logic, not just intent, since this is
 * exactly the kind of cross-provider correctness code that's invisible when
 * right and silently corrupts a conversation when subtly wrong).
 *
 * 1. Downgrade images for non-vision models.
 * 2. Strip/convert thinking blocks for cross-model replay.
 * 3. Normalize tool call IDs for cross-provider compatibility.
 * 4. Insert synthetic tool results for orphaned tool calls.
 */
import type { Message, MessageContent, ToolCall } from './types'

const NON_VISION_USER_IMAGE_PLACEHOLDER = '(image omitted: model does not support images)'
const NON_VISION_TOOL_IMAGE_PLACEHOLDER = '(tool image omitted: model does not support images)'

function replaceImagesWithPlaceholder(content: MessageContent[], placeholder: string): MessageContent[] {
  const result: MessageContent[] = []
  let previousWasPlaceholder = false
  for (const block of content) {
    if (block.type === 'image') {
      if (!previousWasPlaceholder) result.push({ type: 'text', text: placeholder })
      previousWasPlaceholder = true
    } else if (block.type === 'text') {
      previousWasPlaceholder = block.text === placeholder
      result.push(block)
    } else {
      previousWasPlaceholder = false
      result.push(block)
    }
  }
  return result
}

function downgradeUnsupportedImages(messages: Message[], supportsImages: boolean): Message[] {
  if (supportsImages) return messages
  return messages.map((msg) => {
    if (msg.role === 'user') {
      return { ...msg, contentBlocks: replaceImagesWithPlaceholder(msg.contentBlocks, NON_VISION_USER_IMAGE_PLACEHOLDER) }
    }
    if (msg.role === 'toolResult') {
      return { ...msg, contentBlocks: replaceImagesWithPlaceholder(msg.contentBlocks, NON_VISION_TOOL_IMAGE_PLACEHOLDER) }
    }
    return msg
  })
}

export interface TransformOptions {
  supportsImages: boolean
  targetProvider: string
  targetApi: string
  targetModelId: string
  normalizeToolCallId?: (id: string) => string
}

export function transformMessages(messages: Message[], opts: TransformOptions): Message[] {
  const imageAware = downgradeUnsupportedImages(messages, opts.supportsImages)
  const toolCallIdMap = new Map<string, string>()

  // Pass 1: transform content blocks.
  const transformed: Message[] = imageAware.map((msg) => {
    if (msg.role === 'user') return msg

    if (msg.role === 'toolResult') {
      if (msg.toolCallId) {
        const normalized = toolCallIdMap.get(msg.toolCallId)
        if (normalized) return { ...msg, toolCallId: normalized }
      }
      return msg
    }

    if (msg.role === 'assistant') {
      const isSameModel = msg.provider === opts.targetProvider && msg.api === opts.targetApi && msg.modelId === opts.targetModelId

      const newContent: MessageContent[] = msg.contentBlocks.flatMap((block): MessageContent[] => {
        if (block.type === 'thinking') {
          if (block.redacted) return isSameModel ? [block] : []
          if (isSameModel && block.thinkingSignature) return [block]
          if (block.thinking.trim() === '') return []
          if (isSameModel) return [block]
          return [{ type: 'text', text: block.thinking }]
        }
        if (block.type === 'text') return [block]
        if (block.type === 'toolCall') {
          const normalized: MessageContent & ToolCall = { ...block }
          if (!isSameModel) {
            normalized.thoughtSignature = null
            if (opts.normalizeToolCallId) {
              const normalizedId = opts.normalizeToolCallId(block.id)
              if (normalizedId !== block.id) {
                toolCallIdMap.set(block.id, normalizedId)
                normalized.id = normalizedId
              }
            }
          }
          return [normalized]
        }
        return [block]
      })

      return { ...msg, contentBlocks: newContent }
    }

    return msg
  })

  // Pass 2: insert synthetic empty tool results for orphaned tool calls.
  const result: Message[] = []
  let pendingToolCalls: ToolCall[] = []
  let existingToolResultIds = new Set<string>()

  const flushOrphans = () => {
    for (const tc of pendingToolCalls) {
      if (!existingToolResultIds.has(tc.id)) {
        result.push({
          role: 'toolResult',
          contentBlocks: [{ type: 'text', text: 'No result provided' }],
          text: null,
          toolCallId: tc.id,
          toolName: tc.name,
          isError: true,
          timestamp: Date.now(),
          provider: null,
          api: null,
          modelId: null,
          stopReason: null,
          errorMessage: null,
          usage: null,
          diagnostics: null,
        })
      }
    }
    pendingToolCalls = []
    existingToolResultIds = new Set()
  }

  for (const msg of transformed) {
    if (msg.role === 'assistant') {
      flushOrphans()
      if (msg.stopReason === 'error' || msg.stopReason === 'aborted') continue
      pendingToolCalls = msg.contentBlocks.filter((b): b is MessageContent & ToolCall & { type: 'toolCall' } => b.type === 'toolCall')
      result.push(msg)
    } else if (msg.role === 'toolResult') {
      if (msg.toolCallId) existingToolResultIds.add(msg.toolCallId)
      result.push(msg)
    } else if (msg.role === 'user') {
      flushOrphans()
      result.push(msg)
    } else {
      result.push(msg)
    }
  }

  flushOrphans()
  return result
}

/** Same normalization `anthropic.rs::normalize_tool_call_id` applies: alphanumeric/`_`/`-` only, max 64 chars. */
export function normalizeAnthropicToolCallId(id: string): string {
  return id
    .split('')
    .map((c) => (/[a-zA-Z0-9_-]/.test(c) ? c : '_'))
    .join('')
    .slice(0, 64)
}
