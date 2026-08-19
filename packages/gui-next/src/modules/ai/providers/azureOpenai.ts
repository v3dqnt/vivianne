/**
 * Azure OpenAI Responses API — streaming implementation. TS port of
 * `ai/api/azure_openai.rs`. Env-var-based config resolution (base URL,
 * API version, deployment name) only Rust's process env can see, so Rust
 * resolves all of that first and hands this a single fully-built endpoint
 * URL (`req.baseUrl` already includes `/responses?api-version=...`) and the
 * resolved deployment name as `req.modelId` — this file just streams it.
 */
import { emit } from '@tauri-apps/api/event'
import type { AssistantMessage, Context, MessageContent, ProviderStreamRequest, StreamTokenEvent, ToolCall } from './types'
import { emptyUsage, now } from './types'
import { fetchWithTimeout } from './shared'

function emitToken(payload: StreamTokenEvent) {
  void emit('ai-stream-token', payload)
}

function imageUrlPart(b: MessageContent & { type: 'image' }): Record<string, unknown> {
  const url = b.url && b.url.length > 0 ? b.url : b.base64 && b.mediaType ? `data:${b.mediaType};base64,${b.base64}` : ''
  return { type: 'image_url', image_url: { url } }
}

function buildMessages(context: Context): Array<Record<string, unknown>> {
  const msgs: Array<Record<string, unknown>> = []
  if (context.systemPrompt) msgs.push({ role: 'system', content: context.systemPrompt })

  for (const msg of context.messages) {
    const role = msg.role === 'user' || msg.role === 'assistant' || msg.role === 'tool' ? msg.role : msg.role

    if (msg.contentBlocks.length > 0) {
      const content: Array<Record<string, unknown>> = []
      for (const b of msg.contentBlocks) {
        if (b.type === 'text') content.push({ type: 'text', text: b.text })
        else if (b.type === 'image') {
          const part = imageUrlPart(b)
          if ((part.image_url as Record<string, unknown>).url) content.push(part)
        } else if (b.type === 'toolCall') {
          content.push({ type: 'function', id: b.id, name: b.name, arguments: JSON.stringify(b.arguments ?? {}) })
        } else if (b.type === 'toolResult') {
          const textContent = b.content
            .filter((c): c is MessageContent & { type: 'text' } => c.type === 'text')
            .map((c) => c.text)
            .join('\n')
          content.push({ type: 'tool_result', tool_call_id: b.toolCallId, content: textContent })
        }
      }
      msgs.push({ role, content })
    } else if (msg.text) {
      msgs.push({ role, content: msg.text })
    }
  }

  return msgs
}

function buildRequestBody(req: ProviderStreamRequest): Record<string, unknown> {
  const messages = buildMessages(req.context)
  const body: Record<string, unknown> = {
    model: req.modelId,
    input: messages,
    stream: true,
    store: false,
  }
  if (req.maxTokens) body.max_output_tokens = req.maxTokens
  if (req.thinkingEnabled) {
    body.reasoning = { effort: 'medium', summary: 'auto' }
    body.include = ['reasoning.encrypted_content']
  }
  if (req.context.tools && req.context.tools.length > 0) body.tools = req.context.tools
  return body
}

function mapStopReason(reason: string): string {
  switch (reason) {
    case 'max_output_tokens':
      return 'length'
    case 'content_filter':
      return 'stop'
    case 'tool_calls':
      return 'toolUse'
    case 'stop':
    case 'end_turn':
      return 'stop'
    default:
      return reason
  }
}

function parseSseEvents(raw: string): Array<Record<string, unknown>> {
  const events: Array<Record<string, unknown>> = []
  for (const rawLine of raw.split('\n')) {
    const line = rawLine.trim()
    if (line.startsWith('data: ')) {
      const data = line.slice('data: '.length)
      if (data === '[DONE]') continue
      try {
        events.push(JSON.parse(data) as Record<string, unknown>)
      } catch {
        // ignore parse errors for individual events
      }
    }
  }
  return events
}

function makeErrorMessage(model: string, msg: string): AssistantMessage {
  return {
    role: 'assistant',
    content: [],
    api: 'azure-openai-responses',
    provider: 'azure-openai-responses',
    model,
    usage: emptyUsage(),
    stopReason: 'error',
    errorMessage: msg,
    timestamp: now(),
    diagnostics: null,
  }
}

export async function streamAzureOpenAi(req: ProviderStreamRequest): Promise<void> {
  const emitDone = (message: AssistantMessage) => {
    emitToken({ streamId: req.streamId, delta: '', done: true, message, isThinking: false })
  }

  if (!req.apiKey) {
    emitDone(makeErrorMessage(req.modelId, 'No API key for Azure OpenAI provider'))
    return
  }

  const output: AssistantMessage = {
    role: 'assistant',
    content: [],
    api: 'azure-openai-responses',
    provider: 'azure-openai-responses',
    model: req.modelId,
    usage: emptyUsage(),
    stopReason: 'stop',
    errorMessage: null,
    timestamp: now(),
    diagnostics: null,
  }

  const body = buildRequestBody(req)

  let resp: Response
  try {
    resp = await fetchWithTimeout(req.baseUrl, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'api-key': req.apiKey },
      body: JSON.stringify(body),
    })
  } catch (e) {
    output.stopReason = 'error'
    output.errorMessage = `Request failed: ${e instanceof Error ? e.message : String(e)}`
    emitDone(output)
    return
  }

  if (!resp.ok) {
    const errBody = await resp.text().catch(() => '')
    output.stopReason = 'error'
    output.errorMessage = `Azure OpenAI API error (${resp.status}): ${errBody}`
    emitDone(output)
    return
  }

  let raw: string
  try {
    raw = await resp.text()
  } catch (e) {
    output.stopReason = 'error'
    output.errorMessage = `Failed to read response: ${e instanceof Error ? e.message : String(e)}`
    emitDone(output)
    return
  }

  const events = parseSseEvents(raw)
  let fullText = ''
  const currentToolCalls = new Map<string, { name: string; args: string }>()

  for (const event of events) {
    const eventType = event.type as string | undefined

    if (eventType === 'response.output_text.delta') {
      const delta = event.delta as string | undefined
      if (delta) {
        fullText += delta
        emitToken({ streamId: req.streamId, delta, done: false, message: null, isThinking: false })
      }
    } else if (eventType === 'response.reasoning_summary_text.delta') {
      const delta = event.delta as string | undefined
      if (delta) {
        emitToken({ streamId: req.streamId, delta, done: false, message: null, isThinking: true })
      }
    } else if (eventType === 'response.output_item.added') {
      const item = event.item as Record<string, unknown> | undefined
      if (item?.type === 'function_call') {
        const id = typeof item.call_id === 'string' ? item.call_id : ''
        const name = typeof item.name === 'string' ? item.name : ''
        currentToolCalls.set(id, { name, args: '' })
      }
    } else if (eventType === 'response.function_call_arguments.delta') {
      const callId = event.call_id as string | undefined
      const delta = event.delta as string | undefined
      if (callId && delta) {
        const tc = currentToolCalls.get(callId)
        if (tc) tc.args += delta
      }
    } else if (eventType === 'response.function_call_arguments.done') {
      const callId = event.call_id as string | undefined
      if (callId) {
        const tc = currentToolCalls.get(callId)
        currentToolCalls.delete(callId)
        if (tc) {
          let args: unknown = {}
          try {
            args = JSON.parse(tc.args)
          } catch {
            // leave empty
          }
          const toolCall: ToolCall = { id: callId, name: tc.name, arguments: args, thoughtSignature: null }
          output.content.push({ type: 'toolCall', ...toolCall })
        }
      }
    } else if (eventType === 'response.completed' || eventType === 'response.done') {
      const resp2 = event.response as Record<string, unknown> | undefined
      const usage = resp2?.usage as Record<string, unknown> | undefined
      if (usage) {
        const input = (usage.input_tokens as number | undefined) ?? 0
        const outputTokens = (usage.output_tokens as number | undefined) ?? 0
        const details = usage.input_tokens_details as Record<string, unknown> | undefined
        output.usage.input = input
        output.usage.output = outputTokens
        output.usage.totalTokens = input + outputTokens
        output.usage.cacheRead = (details?.cached_tokens as number | undefined) ?? 0
      }
      if (resp2?.status === 'completed') output.stopReason = 'stop'
      const incompleteReason = (resp2?.incomplete_details as Record<string, unknown> | undefined)?.reason
      if (typeof incompleteReason === 'string') output.stopReason = mapStopReason(incompleteReason)
    }
  }

  if (fullText) output.content.unshift({ type: 'text', text: fullText })
  if (output.content.some((c) => c.type === 'toolCall') && output.stopReason === 'stop') {
    output.stopReason = 'toolUse'
  }

  emitDone(output)
}
