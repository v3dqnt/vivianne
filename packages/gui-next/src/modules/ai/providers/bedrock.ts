/**
 * AWS Bedrock Converse streaming — TS port of `ai/api/bedrock.rs`'s HTTP
 * call + binary event-stream parsing. SigV4 signing needs the AWS secret
 * key, which must never reach the webview, so Rust builds the request body,
 * signs it, and hands this file the finished URL (`req.baseUrl`), headers
 * (`req.extraHeaders`), and exact body bytes (`req.rawBody`) to send as-is —
 * this file only executes the fetch and parses the binary response.
 */
import { emit } from '@tauri-apps/api/event'
import type { AssistantMessage, ProviderStreamRequest, StreamTokenEvent, ToolCall } from './types'
import { emptyUsage, now } from './types'
import { fetchWithTimeout } from './shared'

function emitToken(payload: StreamTokenEvent) {
  void emit('ai-stream-token', payload)
}

function mapStopReason(reason: string): string {
  switch (reason) {
    case 'end_turn':
      return 'stop'
    case 'max_tokens':
      return 'length'
    case 'tool_use':
      return 'toolUse'
    case 'stop_sequence':
      return 'stop'
    case 'guardrail_intervened':
      return 'stop'
    default:
      return reason
  }
}

// ── AWS binary event-stream frame parsing ───────────────────────────────────
// [total_len:4][headers_len:4][prelude_crc:4][headers...][payload...][msg_crc:4]

function parseEventFrames(data: Uint8Array): Array<Map<string, string>> {
  const frames: Array<Map<string, string>> = []
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength)
  const decoder = new TextDecoder()
  let pos = 0

  while (pos + 12 <= data.length) {
    const totalLen = view.getUint32(pos)
    const headersLen = view.getUint32(pos + 4)

    if (pos + totalLen > data.length) break

    const headersStart = pos + 12
    const headersEnd = headersStart + headersLen
    const payloadStart = headersEnd
    const payloadEnd = pos + totalLen - 4

    const headers = new Map<string, string>()
    let h = headersStart
    while (h < headersEnd) {
      if (h >= data.length) break
      const nameLen = data[h]
      h += 1
      if (h + nameLen > data.length) break
      const name = decoder.decode(data.slice(h, h + nameLen))
      h += nameLen
      if (h >= data.length) break
      h += 1 // header type byte (7 = string), unused
      if (h + 2 > data.length) break
      const valLen = view.getUint16(h)
      h += 2
      if (h + valLen > data.length) break
      const val = decoder.decode(data.slice(h, h + valLen))
      h += valLen
      headers.set(name, val)
    }

    if (payloadStart < payloadEnd && payloadEnd <= data.length) {
      headers.set(':payload', decoder.decode(data.slice(payloadStart, payloadEnd)))
    }

    frames.push(headers)
    pos += totalLen
  }

  return frames
}

function makeErrorMessage(modelId: string, msg: string): AssistantMessage {
  return {
    role: 'assistant',
    content: [],
    api: 'bedrock-converse-stream',
    provider: 'bedrock',
    model: modelId,
    usage: emptyUsage(),
    stopReason: 'error',
    errorMessage: msg,
    timestamp: now(),
    diagnostics: null,
  }
}

export async function streamBedrock(req: ProviderStreamRequest): Promise<void> {
  const emitDone = (message: AssistantMessage) => {
    emitToken({ streamId: req.streamId, delta: '', done: true, message, isThinking: false })
  }

  if (!req.rawBody) {
    emitDone(makeErrorMessage(req.modelId, 'No request body provided for Bedrock (signing failed upstream)'))
    return
  }

  const headers: Record<string, string> = { 'content-type': 'application/json', ...(req.extraHeaders ?? {}) }

  let resp: Response
  try {
    resp = await fetchWithTimeout(req.baseUrl, { method: 'POST', headers, body: req.rawBody })
  } catch (e) {
    emitDone(makeErrorMessage(req.modelId, `Request failed: ${e instanceof Error ? e.message : String(e)}`))
    return
  }

  if (!resp.ok) {
    const errBody = await resp.text().catch(() => '')
    emitDone(makeErrorMessage(req.modelId, `${resp.status}: ${errBody}`))
    return
  }

  let bytes: Uint8Array
  try {
    const buf = await resp.arrayBuffer()
    bytes = new Uint8Array(buf)
  } catch (e) {
    emitDone(makeErrorMessage(req.modelId, `Failed to read response: ${e instanceof Error ? e.message : String(e)}`))
    return
  }

  const output: AssistantMessage = {
    role: 'assistant',
    content: [],
    api: 'bedrock-converse-stream',
    provider: 'bedrock',
    model: req.modelId,
    usage: emptyUsage(),
    stopReason: 'stop',
    errorMessage: null,
    timestamp: now(),
    diagnostics: null,
  }

  const frames = parseEventFrames(bytes)
  let fullText = ''
  let fullThinking = ''
  const toolPartial = new Map<number, string>()
  // Tracks which content[] index each contentBlockIndex's tool call landed at,
  // since Rust matches "the first tool call with empty args" which breaks
  // once more than one tool call is in flight — this keeps them distinct.
  const toolContentIndexByBlock = new Map<number, number>()

  for (const frame of frames) {
    const eventType = frame.get(':event-type') ?? ''
    const payloadStr = frame.get(':payload') ?? '{}'
    let payload: Record<string, unknown>
    try {
      payload = JSON.parse(payloadStr) as Record<string, unknown>
    } catch {
      payload = {}
    }

    if (eventType === 'messageStart') {
      emitToken({ streamId: req.streamId, delta: '', done: false, message: null, isThinking: false })
    } else if (eventType === 'contentBlockStart') {
      const idx = (payload.contentBlockIndex as number | undefined) ?? 0
      const start = payload.start as Record<string, unknown> | undefined
      const toolUse = start?.toolUse as Record<string, unknown> | undefined
      if (toolUse) {
        const id = typeof toolUse.toolUseId === 'string' ? toolUse.toolUseId : ''
        const name = typeof toolUse.name === 'string' ? toolUse.name : ''
        const toolCall: ToolCall = { id, name, arguments: {}, thoughtSignature: null }
        toolContentIndexByBlock.set(idx, output.content.length)
        output.content.push({ type: 'toolCall', ...toolCall })
        toolPartial.set(idx, '')
      }
    } else if (eventType === 'contentBlockDelta') {
      const idx = (payload.contentBlockIndex as number | undefined) ?? 0
      const delta = payload.delta as Record<string, unknown> | undefined
      const text = delta?.text
      const reasoningContent = delta?.reasoningContent as Record<string, unknown> | undefined
      const toolUse = delta?.toolUse as Record<string, unknown> | undefined

      if (typeof text === 'string') {
        fullText += text
        emitToken({ streamId: req.streamId, delta: text, done: false, message: null, isThinking: false })
      } else if (typeof reasoningContent?.text === 'string') {
        const thinkingText = reasoningContent.text
        fullThinking += thinkingText
        emitToken({ streamId: req.streamId, delta: thinkingText, done: false, message: null, isThinking: true })
      } else if (typeof toolUse?.input === 'string') {
        const acc = toolPartial.get(idx)
        if (acc !== undefined) toolPartial.set(idx, acc + toolUse.input)
      }
    } else if (eventType === 'contentBlockStop') {
      const idx = (payload.contentBlockIndex as number | undefined) ?? 0
      const partial = toolPartial.get(idx)
      if (partial !== undefined) {
        toolPartial.delete(idx)
        const contentIdx = toolContentIndexByBlock.get(idx)
        if (contentIdx !== undefined) {
          const block = output.content[contentIdx]
          if (block?.type === 'toolCall') {
            try {
              block.arguments = JSON.parse(partial)
            } catch {
              // leave empty
            }
          }
        }
      }
    } else if (eventType === 'messageStop') {
      const stopReason = typeof payload.stopReason === 'string' ? payload.stopReason : 'stop'
      output.stopReason = mapStopReason(stopReason)
    } else if (eventType === 'metadata') {
      const usage = payload.usage as Record<string, unknown> | undefined
      if (usage) {
        const num = (v: unknown): number => (typeof v === 'number' ? v : 0)
        output.usage.input = num(usage.inputTokens)
        output.usage.output = num(usage.outputTokens)
        output.usage.totalTokens = output.usage.input + output.usage.output
        output.usage.cacheRead = num(usage.cacheReadInputTokenCount)
        output.usage.cacheWrite = num(usage.cacheWriteInputTokenCount)
      }
    } else if (
      eventType === 'internalServerException' ||
      eventType === 'modelStreamErrorException' ||
      eventType === 'validationException' ||
      eventType === 'throttlingException' ||
      eventType === 'serviceUnavailableException'
    ) {
      const errMsg = typeof payload.message === 'string' ? payload.message : eventType
      output.stopReason = 'error'
      output.errorMessage = `${eventType}: ${errMsg}`
    }
  }

  if (fullText && !output.content.some((c) => c.type === 'text')) {
    output.content.unshift({ type: 'text', text: fullText })
  }
  if (fullThinking) {
    output.content.unshift({ type: 'thinking', thinking: fullThinking, thinkingSignature: null, redacted: false })
  }

  emitDone(output)
}
