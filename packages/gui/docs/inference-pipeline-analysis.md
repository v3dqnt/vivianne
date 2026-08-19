# Inference Pipeline — Latency Analysis

## End-to-End Flow: `sendMessage()` → Provider Response

```
VoiceMode.tsx / ChatInterface
  └─ sendMessage(text, images)
       │
       ▼
AgentProvider.tsx
  ├─ [Auto mode] modelRouter.ts → chooseBestModel() (~1ms)
  ├─ [Auto mode] invoke("cmd_agent_set_model") → Tauri IPC (~5-10ms)
  ├─ [Auto mode] invoke("cmd_agent_set_thinking_level") → Tauri IPC (~5-10ms)
  ├─ setMessages(prev => [...prev, userMessage]) → React re-render (~1-5ms)
  └─ invoke("cmd_agent_prompt", { text, images }) → Tauri IPC (~5-50ms)
       │
       ▼  (Tauri IPC boundary ─ serialize prompt + images)
       │
Rust: commands.rs → cmd_agent_prompt
  ├─ Deserialize args
  ├─ Create AgentMessage::User
  ├─ Acquire agent lock (may block if another turn is running)
  └─ run_agent_loop_with_prompts()
       │
       ▼
Rust: loop_core.rs → run_agent_loop_continue()
  ├── Emit AgentStart
  │
  ├── [PER TURN START] Step action check (Continue/Nudge/Stop)
  │
  ├── Emit TurnStart
  │
  ├── state.model().await  ── read model from state
  ├── state.thinking_level().await  ── read thinking level
  │
  ├── maybe_compact_context() ── token-budget check
  │   ├── calculate_context_tokens() ── iterates all messages
  │   ├── If over budget: find_cut_point() + generate_llm_summary() (LLM call!) + replace history
  │   └── Rarely fires (only when approaching context limit)
  │
  ├── resolve_api_key() ── keychain read or env var
  │
  ├── prune_aging_tool_results() ── iterates messages (~fast)
  ├── evict_old_images() ── iterates messages (~fast)
  │
  ├── convert_agent_messages_to_ai() ── iterates ALL messages, builds content blocks
  ├── get_agent_tool_tool_definitions() ── builds tool schemas from scratch
  │
  ├── Build Context { messages, system_prompt, tools }
  │
  ├── [SAFETY] serde_json::to_string(&context) ── FULL serialization to check size
  │   └── If > 4MB: evict images + potentially drop half of history
  │
  └── call_provider_with_fallback()
       └── call_provider_with_retry()
            └── call_provider_stream_and_await()
                 │
                 ├── provider_spec() ── lookup route
                 ├── resolve_base_url() ── get endpoint URL
                 │
                 ├── Create mpsc::channel (stream tokens)
                 ├── Create oneshot::channel (final message)
                 ├── app.listen_any("ai-stream-token") ── register Tauri event listener
                 │
                 ├── tokio::spawn(async { provider-specific stream call })
                 │   ├── Anthropic: anthropic::stream()
                 │   ├── OpenAI: openai_completions::stream_chat_completions()
                 │   ├── OpenAI Responses: openai_responses::stream_responses()
                 │   ├── Google: google::stream()
                 │   ├── Mistral: mistral::stream()
                 │   ├── Bedrock: bedrock::stream_bedrock()
                 │   └── Azure: azure_openai::stream_azure_openai()
                 │
                 ├── [TOKENS STREAM BACK]
                 │   Each token:
                 │   provider → Rust stream handler
                 │     → mpsc::channel::send() (lock, push, wake)
                 │     → tokio::select! drains rx
                 │     → Emit MessageStart (first token only)
                 │     → Emit MessageUpdate { message, chunk }
                 │       → app.emit("agent_event", payload)
                 │         → Tauri IPC serialize payload as JSON
                 │           → Frontend event listener
                 │             → handleBackendEvent(payload)
                 │               → setMessages(prev => prev.map(m => ...))
                 │                 → React re-render
                 │
                 └── Provider done → oneshot fires
                     → app.unlisten(handler_id)
                     → record_usage() (async, fire-and-forget)
                     → Emit MessageEnd { message }
                     → Return Ok(AgentMessage)
               │
               [RETRY on transient failure: 1s, 2s, 4s, 8s + jitter]
               [FALLBACK on model failure: switch to fallback model, retry]
               │
               ▼
         ◀── Back in loop_core.run_agent_loop_continue()
               │
               ├── context.messages.push(assistant_message)
               ├── state.add_message(assistant_message)
               │
               ├── extract_tool_calls() ── parse content for tool calls
               ├── If tool calls: execute_tool_calls() → emit tool results
               ├── If tool results: maybe_compact_context() again
               │
               ├── Emit TurnEnd { message, tool_results }
               │
               ├── Check steering queue → drain pending messages
               └── If tool calls exist → LOOP to next turn
                        │
                        ▼
                  (If no tool calls + no pending → break outer loop)
               │
               ▼
         ◀── Outer loop exit
               │
               ├── build_turn_memory_summary() → maybe auto-capture (async)
               ├── maybe_send_completion_notification()
               └── Emit AgentEnd
                    │
                    ▼
               ┌─────────────────────────────────────────────┐
               │  Frontend receives AgentEnd                  │
               │  ├── setIsBusy(false)                         │
               │  ├── setRetryStatus(null)                     │
               │  ├── loadMessages() ─── invoke("cmd_agent_messages")  │
               │  ├── refreshSessionStats() ─── IPC            │
               │  └── refreshSessionCost() ─── IPC             │
               └─────────────────────────────────────────────┘
```

## Latency Budget (perceived from user's perspective)

### Phase 1: Pre-request (50-200ms)

| Source | Latency | Notes |
|--------|---------|-------|
| Auto model routing (if enabled) | 2-20ms | JS-side model selection + 2x IPC calls (`setModel` + `setThinkingLevel`) |
| Voice prompt wrapper | ~0ms | `voicePrompt()` is synchronous string concat |
| Tauri IPC invoke | 5-50ms | Serialize text + images, send, deserialize on Rust side |
| Agent lock contention | 0-5000ms | If another turn is running, blocks until it finishes |
| **Total Phase 1** | **~10-70ms** (typical, no contention) | |

### Phase 2: Context assembly (5-200ms)

| Source | Latency | Notes |
|--------|---------|-------|
| State reads (model, thinking level) | ~1ms | In-memory state |
| `maybe_compact_context()` check | 1-50ms | Token count + budget check; rare compaction = 2-5s |
| `prune_aging_tool_results()` | 0.5-10ms | Per-message iteration |
| `evict_old_images()` | 0.5-5ms | Per-message iteration |
| `convert_agent_messages_to_ai()` | 2-50ms | Iterates all messages, clones content blocks |
| `get_agent_tool_definitions()` | 5-50ms | Builds tool schemas from scratch each turn |
| Serialization safety check | **5-200ms** | `serde_json::to_string` on entire context (4MB cap) |
| API key resolution | 2-30ms | Keychain read or env var lookup |
| **Total Phase 2** | **~15-300ms** | Varies with session size |

### Phase 3: Provider setup (20-300ms)

| Source | Latency | Notes |
|--------|---------|-------|
| Provider spec lookup | ~0.1ms | Registry map lookup |
| Channel creation (mpsc + oneshot) | ~0.1ms | |
| Tauri event listener registration | 2-10ms | `app.listen_any` |
| tokio::spawn | ~0.1ms | |
| **Total Phase 3** | **~2-10ms** | Negligible |

### Phase 4: Network + TTFT (500-5000ms) ← DOMINANT

| Source | Latency | Notes |
|--------|---------|-------|
| DNS resolution | 5-100ms | Usually cached |
| TLS handshake | 20-200ms | Depends on provider CDN |
| HTTP request upload | 10-1000ms | Proportional to context size (images = large) |
| Provider queuing + inference | **200-3000ms** | Provider-side. Varies by model + load |
| **Total Phase 4: TTFT** | **~500-5000ms** | TIME TO FIRST TOKEN |

### Phase 5: Streaming (variable, output-length dependent)

| Source | Latency | Notes |
|--------|---------|-------|
| Per-token provider latency | 10-200ms/token | Varies by model |
| Rust mpsc channel | ~0.01ms | In-process, negligible |
| Tauri event emit per token | **1-10ms** | JSON serialize + IPC for EACH token |
| Frontend event listener | 0.5-2ms | |
| `setMessages` + React re-render | **5-50ms** | Creates new array, diff, re-render tree |
| **Total per-token overhead** | **~7-60ms** | Can accumulate significantly for long responses |

### Phase 6: Post-turn cleanup (20-150ms)

| Source | Latency | Notes |
|--------|---------|-------|
| `loadMessages()` IPC | 5-50ms | Full message reload from backend |
| `refreshSessionStats()` IPC | 5-30ms | |
| `refreshSessionCost()` IPC | 5-30ms | |
| React re-render from message reload | 5-30ms | |
| Memory auto-capture (async) | 0ms | Fire-and-forget, non-blocking |
| **Total Phase 6** | **~20-150ms** | Runs AFTER user sees the reply |

### Total from sendMessage() to text appearing:

| Stage | Typical | Worst case |
|-------|---------|------------|
| Pre-request | 50ms | 100ms |
| Context assembly | 50ms | 300ms (+5s if compaction fires) |
| Provider setup | 5ms | 10ms |
| TTFT (dominant) | **1500ms** | **5000ms** |
| Streaming (per-token overhead) | 10-50ms/token | 60ms/token |
| **Total to first visible text** | **~1.6s** | **~5.4s** |

---

## Optimization Recommendations

### Tier 1: High Impact (each saves 100ms-2s)

#### 1. Batch Tauri IPC on auto-mode model selection

**Problem**: When auto mode is enabled, `sendMessage()` makes two additional IPC calls (`setModel` + `setThinkingLevel`) before `cmd_agent_prompt`. Each is an independent round trip.

**Fix**: Add a single `cmd_agent_auto_configure` command that accepts `{ model, thinkingLevel, text, images }` and atomically sets the model, thinking level, AND starts the prompt in one IPC round trip.

**Gain**: **10-40ms** per auto-mode message. Small but eliminates two sequential IPC calls.

#### 2. Eliminate `serde_json::to_string` full serialization for safety check

**Problem**: The `MAX_REQUEST_BYTES` safety net serializes the ENTIRE LLM context to a JSON string just to check its byte length. For a session with 200k tokens, this can take 50-200ms of CPU time on every turn.

**Fix**: Use an approximate byte counter instead of full serialization. Count total string-UTF8 bytes of all message content + estimated overhead from JSON framing. This is O(n) but avoids allocating the full serialized string.

**Gain**: **50-200ms** per turn, more for large sessions.

#### 3. Cache tool definitions between turns

**Problem**: `get_agent_tool_definitions()` builds the full tool schema list from scratch on every single LLM call. Tools rarely change mid-session.

**Fix**: Cache tool definitions in an `Arc<RwLock<Option<Vec<...>>>>` on the `AgentContext`. Invalidate when workspace, MCP servers, or permission rules change. In the common case, return the cached version instantly.

**Gain**: **5-50ms** per turn. Small per-turn, but compounds over 5-15 turns per run.

#### 4. Batch streaming token events (instead of per-token `emit`)

**Problem**: Each streaming token goes through: mpsc channel → Tauri `emit()` → JSON serialize → IPC → frontend → React `setMessages`. For a 500-token reply, that's 500 Tauri event emits and 500 React state updates. Each `emit()` costs 1-10ms in serialization/IPC.

**Fix**: Batch tokens on the Rust side: collect tokens for 50-100ms windows, then emit a single `MessageUpdate` with the accumulated text. This reduces Tauri event frequency by 10-20x.

**Gain**: Reduces per-token overhead from 7-60ms to effectively amortized cost. For fast-streaming models (e.g., Claude Haiku, GPT-4o-mini), the frontend currently can't keep up with per-token emits.

#### 5. Use `Channel` instead of `emit()` for streaming tokens

**Problem**: The bash/powershell streaming output already uses Tauri's `Channel` for low-latency streaming (noted in `AgentProvider.tsx`'s own comment: *"Tauri's own docs say the event system 'is not designed for low latency or high throughput situations'"*). But the LLM token streaming still uses the `emit()` event system.

**Fix**: Register a Tauri `Channel` for LLM token streaming in `call_provider_stream_and_await`, similar to how `cmd_coding_register_stream_channel` is used for bash output. The `Channel` API uses a binary protocol that's significantly faster than JSON event serialization.

**Gain**: **3-8ms** per token, which adds up to **1.5-4s** for a 500-token reply. One of the biggest wins.

#### 6. Skip post-turn IPC when nothing changed

**Problem**: On `turn_end`, the frontend always calls `loadMessages()` + `refreshSessionStats()` + `refreshSessionCost()` — three separate IPC calls. For turns with no tool calls (pure Q&A), none of this state changed. The full message reload especially is wasteful.

**Fix**: The backend could include a `state_changed: bool` flag in the `TurnEnd`/`AgentEnd` events. The frontend skips the three IPC calls when `state_changed` is false. Alternatively, include the new message count/stats inline in the event payload.

**Gain**: **20-150ms** saved on every tool-free turn.

#### 7. Stream MessageStart inline with first token

**Problem**: The current flow requires the first streaming token to arrive before `MessageStart` is emitted. But `MessageStart` doesn't carry any content — it just creates the placeholder. This means there's an extra event emit at the token 0 boundary.

**Fix**: Emit `MessageStart` immediately before calling the provider (synchronously), not after waiting for the first token. The user sees the assistant bubble appear instantly, and tokens stream into it.

**Gain**: **~0ms** for latency (cosmetic), but improves perceived responsiveness significantly — the assistant card appears before the provider even responds.

### Tier 2: Medium Impact (each saves 10-100ms)

#### 8. Optimize `setMessages` in streaming handler

**Problem**: The `message_update` handler does:
```js
setMessages(prev => {
  const last = [...prev];  // full array clone
  const idx = last.findIndex(m => m.isStreaming);  // linear scan
  last[idx] = { ...last[idx], content: last[idx].content + (chunk || "") };
  return last;
});
```
This clones the entire messages array and does a linear scan on EVERY chunk.

**Fix**: Use a ref-based approach with `requestAnimationFrame` batching:
- Store the streaming text in a `useRef<string>`
- Use `requestAnimationFrame` to batch updates to React state
- Only do the `setMessages` clone once per frame instead of per-token

**Gain**: **5-30ms** per token. Smooths out UI jank during fast streaming.

#### 9. Prefetch API key before context assembly

**Problem**: API key resolution happens during context assembly, serialized after message conversion. It could be started earlier (in parallel with context assembly).

**Fix**: Start `resolve_api_key()` in parallel with `convert_agent_messages_to_ai()` and tool definition building.

**Gain**: **2-30ms** — small but free win (adds concurrency).

#### 10. Skip `convert_agent_messages_to_ai` for unchanged message tails

**Problem**: This function iterates ALL messages to convert them. In a multi-turn run, only the last few messages changed. But the function reprocesses the entire history every turn.

**Fix**: Cache the converted message list and only convert delta (new) messages. Reuse the cached conversions for unchanged history.

**Gain**: **2-20ms** per turn, proportional to session length.

#### 11. Reduce `Content` clone overhead in `convert_content_blocks`

**Problem**: The content block conversion clones each message's content blocks. For messages with many tool calls or images, this allocation adds up.

**Fix**: Use `Arc<Vec<...>>` for content blocks to share ownership instead of deep cloning. Only clone when actually modifying.

**Gain**: **5-50ms** for sessions with many tool-result messages.

### Tier 3: Lower Impact (under 10ms each)

#### 12. Pre-warm TLS connections
If the app can establish TCP/TLS connections to the provider's API endpoint before the first prompt, it saves 25-300ms of handshake time. Could be done during app startup or voice model preloading.

#### 13. Move tool definitions to compile-time or lazy-static
Instead of building tool schemas from configuration files every call, include them as compile-time constants. Only dynamic (MCP) tools need runtime construction.

#### 14. Batch `loadMessages` + `refreshSessionStats` into one IPC call
Create a `cmd_agent_turn_complete` command that returns messages + stats + cost in a single response. Saves two round trips per turn.

#### 15. User-facing: Show models sorted by TTFT
In the model picker, surface estimated speed (TTFT) alongside price. Users who optimize for latency can pick faster models for voice interactions.

---

## Summary: Critical Path

```
Current: sendMessage → [2x IPC auto-config] → invoke prompt → IPC → context assembly → API key → provider call → TTFT ~1500ms → tokens via emit()

Optimized: sendMessage → invoke prompt (with auto-config baked in) → IPC → context assembly (parallel API key, cached tools) → provider call → TTFT ~1500ms → tokens via Channel (10x fewer events)

Fixes applied:
  - Batch auto-config into prompt call (saves 2 IPC round trips)
  - Cache tool definitions (saves 5-50ms)
  - Replace safety serialization with byte counter (saves 50-200ms)
  - Replace emit() with Channel for tokens (saves 1.5-4s for long replies)
  - Skip post-turn IPC when nothing changed (saves 20-150ms)
  - Batch streaming tokens (saves 50-200ms per reply)
  - Inline MessageStart (improved perceived latency)
```

The biggest bottleneck is **TTFT** (provider network + inference latency) — typically 1.5-5s — which is outside the app's control. The next biggest is **per-token streaming overhead** from Tauri's event system (1.5-4s for a long reply). Fixing the streaming path (switching to `Channel` and batching tokens) would cut that overhead by ~90%.
