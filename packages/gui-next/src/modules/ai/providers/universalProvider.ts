/**
 * Universal LLM streaming — one file for every AI-SDK-backed provider
 * (Anthropic, OpenAI Chat Completions, OpenAI Responses, Google). Each one
 * needs a different `LanguageModel` factory, different `providerOptions`
 * (thinking config shape, cache control), and a couple of small
 * message-building differences — but the actual streaming loop and
 * event-emission are 100% shared via `sdkShared.ts`. Four files that mostly
 * differed in a ~20-line setup block are now one file with a switch over
 * that setup, sharing everything else — including the OpenRouter
 * low-credit-retry loop, which was previously openaiChat-only but is
 * harmless (a no-op) for providers whose error bodies don't match its
 * pattern, so it now covers all four uniformly instead of being special-cased.
 *
 * NOT here: Azure OpenAI and Bedrock, each in its own file still. Bedrock's
 * AWS SigV4 signing has to stay in Rust — the webview must never see the AWS
 * secret key, so Rust pre-signs the request and hands this side raw
 * bytes+headers, which doesn't fit this SDK-driven shape at all. Azure's
 * auth (an `api-key` header + a Rust-prebuilt URL) could technically join,
 * but its current implementation doesn't even stream progressively yet
 * (waits for the full response, then replays it) — migrating that is
 * separate work from moving a file.
 */
import { emit } from '@tauri-apps/api/event'
import { createAnthropic } from '@ai-sdk/anthropic'
import { createOpenAI } from '@ai-sdk/openai'
import { createOpenAICompatible } from '@ai-sdk/openai-compatible'
import { createGoogleGenerativeAI } from '@ai-sdk/google'
import { APICallError, stepCountIs, streamText, type LanguageModel, type ModelMessage, type ToolSet } from 'ai'

/** `streamText`'s `providerOptions` param type isn't exported by name from
 * `ai` — derived from the function signature itself instead of reaching
 * into an SDK-internal package for it. */
type SdkProviderOptions = Parameters<typeof streamText>[0]['providerOptions']
import type { AssistantMessage, ProviderStreamRequest, StreamTokenEvent } from './types'
import { emptyUsage, now } from './types'
import { transformMessages, normalizeAnthropicToolCallId } from './transformMessages'
import { toModelMessages, toSdkTools, runSdkStream, describeProviderError } from './sdkShared'
import { APP_HEADERS, IdleWatchdog, parseOpenRouterAffordableTokens } from './shared'

function emitToken(payload: StreamTokenEvent) {
  void emit('ai-stream-token', payload)
}

function makeErrorMessage(model: string, provider: string, api: string, msg: string): AssistantMessage {
  return {
    role: 'assistant',
    content: [],
    api,
    provider,
    model,
    usage: emptyUsage(),
    stopReason: 'error',
    errorMessage: msg,
    timestamp: now(),
    diagnostics: null,
  }
}

interface RequestSetup {
  model: LanguageModel
  api: string
  messages: ModelMessage[]
  system?: string
  tools: ToolSet | undefined
  maxOutputTokens: number | undefined
  providerOptions: SdkProviderOptions
  /** The SDK's own reasoning level: it maps it per provider and per model
   * (adaptive thinking with `effort` where the model has it, a token budget
   * where it doesn't), so no per-model table lives here. */
  reasoning?: Reasoning
}

type Reasoning = 'provider-default' | 'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh'

/** The user's thinking level as the SDK's reasoning level; undefined when
 * the request didn't say (older backend), which keeps the old behaviour. */
function reasoningFor(req: ProviderStreamRequest): Reasoning | undefined {
  switch (req.thinkingLevel) {
    case 'off':
      return 'none'
    case 'minimal':
    case 'low':
    case 'medium':
    case 'high':
    case 'xhigh':
      return req.thinkingLevel
    default:
      return undefined
  }
}

/** Everything that varies per provider. Auth, message shape, caching, and
 * thinking config all live here; the shared execution path below never
 * branches on provider again after this call. */
function buildRequestSetup(req: ProviderStreamRequest, apiKey: string, maxOutputTokens: number | undefined): RequestSetup {
  switch (req.apiKind) {
    case 'anthropicMessages': {
      const anthropic = createAnthropic({
        apiKey,
        baseURL: (req.baseUrl || 'https://api.anthropic.com').replace(/\/+$/, ''),
        headers: { 'anthropic-dangerous-direct-browser-access': 'true' },
      })
      const transformed = transformMessages(req.context.messages, {
        supportsImages: true,
        targetProvider: '',
        targetApi: 'anthropic-messages',
        targetModelId: '',
        normalizeToolCallId: normalizeAnthropicToolCallId,
      })
      const messages = toModelMessages(transformed)
      // A rolling cache breakpoint on the newest message: Anthropic only
      // reads a cache back from an explicit breakpoint, so without this the
      // whole conversation was billed at full price on every step. With it,
      // each step reads everything before it from cache (0.1×) and writes
      // only what's new. Three breakpoints in all, of the four allowed:
      // last tool, system, last message.
      const last = messages[messages.length - 1]
      if (last) {
        last.providerOptions = {
          ...(last.providerOptions ?? {}),
          anthropic: { ...((last.providerOptions?.anthropic as Record<string, unknown>) ?? {}), cacheControl: { type: 'ephemeral' } },
        }
      }
      // Cached as a `system`-role message (not the `system:` string
      // shorthand, which can't carry `providerOptions`) — the tool list
      // barely changes within a session, so caching it (and everything
      // before it) is the single biggest cost/latency lever available.
      if (req.context.systemPrompt) {
        messages.unshift({
          role: 'system',
          content: req.context.systemPrompt,
          providerOptions: { anthropic: { cacheControl: { type: 'ephemeral' } } },
        })
      }
      return {
        model: anthropic(req.modelId),
        api: 'anthropic-messages',
        messages,
        tools: toSdkTools(req.context.tools, true),
        maxOutputTokens: maxOutputTokens || 4096,
        ...(reasoningFor(req)
          ? { reasoning: reasoningFor(req), providerOptions: {} }
          : {
              providerOptions: {
                anthropic: { thinking: req.thinkingEnabled ? { type: 'enabled', budgetTokens: req.thinkingBudgetTokens || 1024 } : undefined },
              },
            }),
      }
    }
    case 'openAiChat':
    case 'mistralChat': {
      // Mistral's chat-completions wire format is identical to OpenAI's —
      // `ai/api/mistral.rs` itself just delegates to openai_completions.rs
      // with a different default base URL, so this does the same.
      //
      // `@ai-sdk/openai-compatible`, not `@ai-sdk/openai` — this branch is
      // every OpenAI-compatible chat-completions HOST that isn't OpenAI
      // itself: OpenRouter, Groq, xAI, DeepSeek, Together, Fireworks,
      // Cerebras, NVIDIA NIM, Mistral, and local llama.cpp/mistral.rs.
      // `@ai-sdk/openai`'s `.chat()` targets real OpenAI's exact response
      // shape; confirmed live that it silently produced empty replies
      // against a local llama-server — direct curl against the same
      // endpoint showed a well-formed stream, including a valid final
      // `choices: []` usage-only chunk (a shape real OpenAI's API also
      // sends, but apparently not one `.chat()` handles the same way this
      // package does). `openai-compatible` is the SDK's own recommended
      // package for exactly this class of host (LM Studio is one of its
      // documented examples).
      const openai = createOpenAICompatible({ name: req.provider, apiKey, baseURL: req.baseUrl.replace(/\/+$/, ''), headers: APP_HEADERS })
      const transformed = transformMessages(req.context.messages, {
        supportsImages: true,
        targetProvider: '',
        targetApi: 'openai-completions',
        targetModelId: '',
      })
      return {
        model: openai(req.modelId),
        api: 'openai-completions',
        messages: toModelMessages(transformed),
        system: req.context.systemPrompt ?? undefined,
        tools: toSdkTools(req.context.tools, false),
        maxOutputTokens,
        providerOptions: {},
      }
    }
    case 'openAiResponses': {
      const openai = createOpenAI({ apiKey, baseURL: req.baseUrl.replace(/\/+$/, ''), headers: APP_HEADERS })
      const transformed = transformMessages(req.context.messages, {
        supportsImages: true,
        targetProvider: '',
        targetApi: 'openai-responses',
        targetModelId: '',
      })
      return {
        model: openai.responses(req.modelId),
        api: 'openai-responses',
        messages: toModelMessages(transformed),
        system: req.context.systemPrompt ?? undefined,
        tools: toSdkTools(req.context.tools, false),
        maxOutputTokens,
        // The chosen level, not a fixed 'medium' whatever the user picked.
        ...(reasoningFor(req) && req.thinkingEnabled ? { reasoning: reasoningFor(req) } : {}),
        providerOptions: {
          openai: {
            ...(req.thinkingEnabled ? (reasoningFor(req) ? { reasoningSummary: 'auto' } : { reasoningEffort: 'medium', reasoningSummary: 'auto' }) : {}),
            // Requests with the same key go to the same cache: a long
            // conversation keeps hitting its prefix.
            ...(req.cacheKey ? { promptCacheKey: req.cacheKey } : {}),
            // On a ChatGPT plan nothing is stored server-side (`store: false`
            // is required), so reasoning travels encrypted with each turn.
            ...(req.provider === 'chatgpt' ? { store: false, include: ['reasoning.encrypted_content'] } : {}),
          },
        },
      }
    }
    case 'googleGenerative': {
      const google = createGoogleGenerativeAI({
        apiKey,
        baseURL: (req.baseUrl || 'https://generativelanguage.googleapis.com').replace(/\/+$/, '') + '/v1beta',
      })
      const transformed = transformMessages(req.context.messages, {
        supportsImages: true,
        targetProvider: req.provider,
        targetApi: 'google-generative-ai',
        targetModelId: req.modelId,
      })
      return {
        model: google(req.modelId),
        api: 'google-generative-ai',
        messages: toModelMessages(transformed),
        system: req.context.systemPrompt ?? undefined,
        tools: toSdkTools(req.context.tools, false),
        maxOutputTokens,
        providerOptions: {
          google: req.thinkingEnabled
            ? { thinkingConfig: { includeThoughts: true, thinkingBudget: req.thinkingBudgetTokens > 0 ? req.thinkingBudgetTokens : -1 } }
            : {},
        },
      }
    }
    default:
      throw new Error(`streamUniversal does not handle apiKind "${req.apiKind}"`);
  }
}

export async function streamUniversal(req: ProviderStreamRequest): Promise<void> {
  const emitErr = (api: string, msg: string) => {
    emitToken({ streamId: req.streamId, delta: '', done: true, message: makeErrorMessage(req.modelId, req.provider, api, msg), isThinking: false })
  }

  if (!req.apiKey) {
    emitErr(req.apiKind, `No API key for provider: ${req.provider}`)
    return
  }

  // OpenRouter's low-credit 402 tells you exactly how many tokens you can
  // afford — retry once at that budget instead of failing outright. Applies
  // uniformly to all four providers here; it's a no-op for the other three
  // since their error bodies never match the OpenRouter-specific pattern.
  //
  // The model's full output limit is requested — no pre-emptive cap. OpenRouter
  // used to be clamped to 4096 "to avoid" that 402, which the retry below
  // already handles precisely; the clamp's real effect was that a reasoning
  // model on high effort spent all 4096 tokens thinking and never got to write
  // its answer or tool call.
  let attemptMaxTokens = req.maxTokens || undefined
  let retriedForCredits = false
  let setupApi: string = req.apiKind

  for (;;) {
    const watchdog = new IdleWatchdog()
    try {
      const setup = buildRequestSetup(req, req.apiKey, attemptMaxTokens)
      setupApi = setup.api

      const result = streamText({
        model: setup.model,
        system: setup.system,
        messages: setup.messages,
        // The Anthropic branch sends its system prompt as a `system`-role
        // message so it can carry a cache breakpoint. AI SDK 7 rejects those
        // unless told otherwise ("System messages are not allowed in the
        // prompt or messages fields"), which failed every Anthropic request
        // before it was sent.
        allowSystemInMessages: true,
        tools: setup.tools,
        maxOutputTokens: setup.maxOutputTokens,
        maxRetries: 0,
        // Force exactly one LLM call per invocation. Rust's agent loop owns
        // turn/tool continuation — without this, the SDK's own
        // step-continuation logic can trigger a second internal generation
        // (observed live: a plain no-tool-call greeting came back as two
        // distinct replies glued together with no separator, and the turn
        // stayed "busy" well past when the first reply had actually finished).
        stopWhen: stepCountIs(1),
        providerOptions: setup.providerOptions,
        ...(setup.reasoning ? { reasoning: setup.reasoning } : {}),
        // The only timeout on this path. Without it a provider that accepts
        // the connection and then goes quiet hangs the whole turn until
        // Rust's 120s backstop — see `IdleWatchdog`.
        abortSignal: watchdog.signal,
        // Errors reach us as an `error` chunk on `fullStream` (and via the
        // catch below). Without this hook the SDK also rethrows them
        // internally, which surfaces as an unhandled rejection.
        onError: () => {},
      })

      // `streamText` exposes result promises (`.text`, `.usage`, …) that
      // reject on their own whenever the stream fails or is aborted. We only
      // ever consume `fullStream`, so nothing observes those rejections —
      // and an unobserved one is an unhandled rejection, which in a webview
      // is a hard crash. Seen live as `AI_NoOutputGeneratedError` the moment
      // the watchdog aborted a stalled stream.
      // `AI_NoOutputGeneratedError` specifically comes from the promises that
      // resolve only once there IS output — `content`, `steps`, `response`,
      // `totalUsage` — not from `.text`/`.usage`, so the original four missed
      // exactly the case a failing provider produces (stream errors before a
      // single token: out of credits, upstream 5xx, rate limit). Reading a
      // property the installed SDK version doesn't expose just yields
      // `undefined`, which `Promise.resolve` handles, so listing generously
      // is safe and beats rediscovering this one rejection at a time.
      const settledProps = [
        'text',
        'usage',
        'totalUsage',
        'finishReason',
        'toolCalls',
        'toolResults',
        'content',
        'steps',
        'response',
        'reasoning',
        'reasoningText',
        'sources',
        'files',
        'warnings',
        'providerMetadata',
      ] as const
      for (const prop of settledProps) {
        void Promise.resolve((result as unknown as Record<string, unknown>)[prop]).catch(() => {})
      }

      // Raced, not just awaited: `abortSignal` above cancels the HTTP
      // request, but the SDK doesn't always surface that as a throw from
      // `fullStream` — when it doesn't, this loop would block past Rust's
      // 120s guard and the turn would die there instead of here.
      const output = await Promise.race([
        runSdkStream({
          streamId: req.streamId,
          provider: req.provider,
          api: setup.api,
          modelId: req.modelId,
          stream: result.fullStream,
          watchdog,
          emitToken,
        }),
        watchdog.expired,
      ])
      // An abort can surface as a clean `abort` chunk rather than a throw,
      // which would otherwise be reported as a normal (truncated) reply.
      if (watchdog.timedOut) {
        emitErr(setup.api, watchdog.message)
        return
      }
      emitToken({ streamId: req.streamId, delta: '', done: true, message: output, isThinking: false })
      return
    } catch (e) {
      if (watchdog.timedOut) {
        emitErr(setupApi, watchdog.message)
        return
      }
      if (APICallError.isInstance(e) && e.statusCode === 402 && !retriedForCredits) {
        const affordable = parseOpenRouterAffordableTokens(e.responseBody ?? '')
        if (affordable && affordable > 0) {
          retriedForCredits = true
          attemptMaxTokens = affordable
          continue
        }
      }
      if (APICallError.isInstance(e) && e.statusCode === 402 && retriedForCredits) {
        emitErr(setupApi, `${req.provider}: out of credits — even a reduced-size request was rejected. Add credits or switch to a different model/provider.`)
        return
      }
      emitErr(setupApi, describeProviderError(req.provider, e))
      return
    } finally {
      watchdog.stop()
    }
  }
}
