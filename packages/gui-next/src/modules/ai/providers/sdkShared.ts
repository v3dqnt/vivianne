/**
 * Shared conversion layer for the Vercel AI SDK-based providers (Anthropic,
 * OpenAI Chat, OpenAI Responses, Google). One converter instead of four
 * hand-rolled ones — `transformMessages()` already normalizes every
 * provider's history into the same `Message[]` shape, so the SDK-message
 * conversion only needs to be written once.
 */
import { APICallError, jsonSchema, tool, type AssistantContent, type ModelMessage, type ToolSet } from 'ai'
import type { AssistantMessage, MessageContent, Message, StreamTokenEvent, ToolCall } from './types'
import { emptyUsage, now } from './types'
import type { IdleWatchdog } from './shared'

function imagePart(b: MessageContent & { type: 'image' }) {
  return { type: 'image' as const, image: b.base64 ?? b.url ?? '', mediaType: b.mediaType ?? 'image/jpeg' }
}

/** Converts an already-`transformMessages()`-normalized history into AI SDK
 * `ModelMessage[]`. Tool results carry only text (SDK tool output requires a
 * single typed value) — an image in a tool result becomes a synthetic
 * follow-up user message, same convention the old hand-rolled providers used. */
export function toModelMessages(messages: Message[]): ModelMessage[] {
  const out: ModelMessage[] = []

  for (const msg of messages) {
    if (msg.role === 'user') {
      if (msg.text) {
        out.push({ role: 'user', content: msg.text })
      } else if (msg.contentBlocks.length > 0) {
        const parts = msg.contentBlocks
          .map((b) => (b.type === 'text' ? { type: 'text' as const, text: b.text } : b.type === 'image' ? imagePart(b) : null))
          .filter((p): p is NonNullable<typeof p> => p !== null)
        if (parts.length > 0) out.push({ role: 'user', content: parts })
      }
    } else if (msg.role === 'assistant') {
      const parts: Exclude<AssistantContent, string> = []
      for (const b of msg.contentBlocks) {
        if (b.type === 'text') {
          if (b.text.trim() !== '') parts.push({ type: 'text', text: b.text })
        } else if (b.type === 'thinking') {
          if (b.thinking.trim() === '' && !b.redacted) continue
          const text = b.redacted ? '[Reasoning redacted]' : b.thinking
          parts.push({
            type: 'reasoning',
            text,
            providerOptions: b.thinkingSignature ? { anthropic: { signature: b.thinkingSignature } } : undefined,
          })
        } else if (b.type === 'toolCall') {
          parts.push({ type: 'tool-call', toolCallId: b.id, toolName: b.name, input: b.arguments })
        }
      }
      if (parts.length > 0) out.push({ role: 'assistant', content: parts })
    } else if (msg.role === 'toolResult') {
      const text = msg.contentBlocks
        .filter((b): b is MessageContent & { type: 'text' } => b.type === 'text')
        .map((b) => b.text)
        .join('\n')
      if (msg.toolCallId) {
        out.push({
          role: 'tool',
          content: [{ type: 'tool-result', toolCallId: msg.toolCallId, toolName: msg.toolName ?? '', output: { type: 'text', value: text } }],
        })
      }
      const images = msg.contentBlocks.filter((b): b is MessageContent & { type: 'image' } => b.type === 'image').map((b) => imagePart(b))
      if (images.length > 0) {
        out.push({ role: 'user', content: [{ type: 'text', text: '(Image from the tool call above)' }, ...images] })
      }
    }
  }

  return out
}

/** Raw JSON-Schema tool defs (as the Rust backend already builds them) — no
 * Zod needed, `jsonSchema()` wraps a plain JSON Schema object directly. No
 * `execute`: the SDK returns the tool call to us instead of running it —
 * Rust's agent loop owns tool execution, not the SDK. */
export function toSdkTools(tools: unknown[] | null | undefined, cacheLastTool: boolean): ToolSet | undefined {
  if (!tools || tools.length === 0) return undefined
  const out: ToolSet = {}
  tools.forEach((t, i) => {
    const obj = t as Record<string, unknown>
    const name = obj.name
    if (typeof name !== 'string') return
    out[name] = tool({
      description: typeof obj.description === 'string' ? obj.description : undefined,
      inputSchema: jsonSchema((obj.parameters ?? obj.input_schema ?? { type: 'object', properties: {} }) as never),
      providerOptions: cacheLastTool && i === tools.length - 1 ? { anthropic: { cacheControl: { type: 'ephemeral' } } } : undefined,
    })
  })
  return out
}

/** Consumes an AI SDK `streamText().fullStream`, translating chunks into the
 * exact `emitToken`/`AssistantMessage` shape the hand-rolled providers
 * already produced — Rust's consumer side (`call_provider_stream_and_await`)
 * doesn't change at all. Not ported here: the `<think>`-tag text-leak
 * workaround the old `openaiChat.ts` had for a handful of weak-tool-calling
 * open-weight hosts — those hosts normally use the SDK's native
 * `reasoning-delta` separation instead, and the raw-tag leak is a narrow
 * enough edge case that it's better disclosed as a known gap than silently
 * half-ported.
 *
 * Text/reasoning are accumulated as flat strings and only assembled into
 * `output.content` once, at the end — matching the old hand-rolled
 * providers' `finalizeOutput` exactly (this is not a style choice: an
 * earlier per-`id` version pushed a *new* content block whenever the SDK's
 * stream introduced a new text-part id, which happens on legitimate
 * multi-step continuation. Two adjacent text blocks with no separator
 * between them is indistinguishable from one garbled reply once
 * `contentToText` flattens content back to a string — confirmed live as a
 * repeated-greeting glitch on a single turn with no tool calls involved).
 * `stopWhen: stepCountIs(1)` on the `streamText()` call sites is the actual
 * fix (Rust's agent loop owns turn/tool continuation, never the SDK) — this
 * accumulation is defense in depth for any step boundary that still slips
 * through. */
export async function runSdkStream(opts: {
  streamId: string
  provider: string
  api: string
  modelId: string
  stream: AsyncIterable<Record<string, unknown> & { type: string }>
  watchdog: IdleWatchdog
  emitToken: (payload: StreamTokenEvent) => void
}): Promise<AssistantMessage> {
  const output: AssistantMessage = {
    role: 'assistant',
    content: [],
    api: opts.api,
    provider: opts.provider,
    model: opts.modelId,
    usage: emptyUsage(),
    stopReason: 'stop',
    errorMessage: null,
    timestamp: now(),
    diagnostics: null,
  }
  let textAcc = ''
  let reasoningAcc = ''
  let reasoningSignature: string | null = null
  const toolCalls: ToolCall[] = []
  let sawFinish = false

  // Labeled so the terminal chunks below can leave the loop from inside the
  // switch. Breaking here also matters beyond tidiness: `for await`'s exit
  // calls the iterator's `return()`, which cancels the underlying HTTP
  // stream — so a provider that holds the connection open after it's done
  // gets hung up on instead of keeping us here.
  consume: for await (const part of opts.stream) {
    opts.watchdog.poke()
    switch (part.type) {
      case 'text-delta': {
        const delta = String(part.text ?? '')
        textAcc += delta
        opts.emitToken({ streamId: opts.streamId, delta, done: false, message: null, isThinking: false })
        break
      }
      case 'reasoning-delta': {
        const delta = String(part.text ?? '')
        reasoningAcc += delta
        const sig = (part.providerMetadata as Record<string, unknown> | undefined)?.anthropic as Record<string, unknown> | undefined
        if (typeof sig?.signature === 'string') reasoningSignature = sig.signature
        opts.emitToken({ streamId: opts.streamId, delta, done: false, message: null, isThinking: true })
        break
      }
      case 'tool-call': {
        toolCalls.push({
          id: String(part.toolCallId ?? ''),
          name: String(part.toolName ?? ''),
          arguments: part.input,
          thoughtSignature: null,
        })
        output.stopReason = 'toolUse'
        break
      }
      case 'finish': {
        sawFinish = true
        const usage = part.totalUsage as Record<string, unknown> | undefined
        const details = usage?.inputTokenDetails as Record<string, unknown> | undefined
        const num = (v: unknown): number => (typeof v === 'number' ? v : 0)
        output.usage = {
          input: num(details?.noCacheTokens),
          output: num(usage?.outputTokens),
          cacheRead: num(details?.cacheReadTokens),
          cacheWrite: num(details?.cacheWriteTokens),
          totalTokens: num(usage?.totalTokens),
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
        }
        const reason = String(part.finishReason ?? 'stop')
        if (output.stopReason !== 'toolUse') {
          output.stopReason = reason === 'length' ? 'length' : reason === 'error' ? 'error' : 'stop'
        }
        // `finish` is terminal — nothing after it can change the reply. The
        // loop used to fall through and wait for the iterator to end by
        // itself, so a provider that kept its connection open left the turn
        // streaming forever: the watchdog was still being poked (so it never
        // fired) and the turn died on Rust's 120s guard instead of ending
        // the moment the model was actually done.
        break consume
      }
      case 'error': {
        output.stopReason = 'error'
        output.errorMessage = describeProviderError(opts.provider, part.error)
        break consume
      }
      case 'abort': {
        output.stopReason = 'aborted'
        break consume
      }
      default:
        break
    }
  }

  // Assemble content once, at the end — same order the old hand-rolled
  // providers used: thinking, then text, then tool calls in arrival order.
  if (reasoningAcc) {
    output.content.push({ type: 'thinking', thinking: reasoningAcc, thinkingSignature: reasoningSignature, redacted: false })
  }
  if (textAcc) {
    output.content.push({ type: 'text', text: textAcc })
  }
  for (const tc of toolCalls) {
    output.content.push({ type: 'toolCall', ...tc })
  }

  if (!sawFinish && output.stopReason !== 'error' && output.stopReason !== 'aborted' && output.content.length === 0) {
    output.stopReason = 'error'
    output.errorMessage = `${opts.provider} returned an empty response — the stream ended before producing any content.`
  }

  return output
}

/**
 * A provider failure as one line the Rust loop can act on: the HTTP status
 * and any `retry-after` the provider sent ride along in the text, because a
 * string is all that crosses to Rust. Without them a rate limit looks like
 * any other failure, and the loop can only guess how long to wait — when the
 * provider has already said exactly how long.
 */
/** The machine-readable error code in a provider's error body, if any
 * (`subscription_sharing_usage_limit_exceeded`, `insufficient_quota`…).
 * The recovery classifier on the Rust side decides on these. */
function errorCode(body: unknown): string | null {
  const o = (typeof body === 'string' ? safeJson(body) : body) as Record<string, any> | null
  if (!o || typeof o !== 'object') return null
  const code = o.error?.code ?? o.code ?? o.error?.type ?? o.response?.error?.code
  return typeof code === 'string' ? code : null
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text)
  } catch {
    return null
  }
}

export function describeProviderError(provider: string, e: unknown): string {
  if (!APICallError.isInstance(e)) {
    if (e instanceof Error) return e.message
    // Errors that arrive mid-stream (`response.failed`) are plain objects.
    if (e && typeof e === 'object') {
      const o = e as Record<string, any>
      const message = o.message ?? o.error?.message ?? JSON.stringify(o).slice(0, 300)
      const code = errorCode(o)
      return `${provider}: ${message}${code && !String(message).includes(code) ? ` [${code}]` : ''}`
    }
    return String(e)
  }
  const headers = e.responseHeaders ?? {}
  const header = (name: string) => Object.entries(headers).find(([k]) => k.toLowerCase() === name)?.[1]
  const ms = Number(header('retry-after-ms'))
  const raw = header('retry-after')
  let retryAfterSecs: number | null = null
  if (Number.isFinite(ms) && ms > 0) retryAfterSecs = ms / 1000
  else if (raw) {
    // Either delta-seconds or an HTTP date.
    const secs = Number(raw)
    retryAfterSecs = Number.isFinite(secs) ? secs : Math.max(0, (Date.parse(raw) - Date.now()) / 1000)
  }
  const status = e.statusCode ? `(HTTP ${e.statusCode}) ` : ''
  const retry = retryAfterSecs !== null && Number.isFinite(retryAfterSecs) ? ` [retry-after: ${Math.ceil(retryAfterSecs)}s]` : ''
  const code = errorCode(e.responseBody ?? e.data)
  const tag = code && !e.message.includes(code) ? ` [${code}]` : ''
  return `${provider}: ${status}${e.message}${tag}${retry}`
}
