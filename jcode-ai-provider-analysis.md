# AI Inference Provider Infrastructure — Deep Analysis

> A side-by-side analysis of the AI provider abstraction layer, model routing,
> authentication, and streaming architecture in **jcode** vs **Vivianne**.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Provider Trait & Abstraction Layer](#2-provider-trait--abstraction-layer)
3. [Wire Protocol Dispatch](#3-wire-protocol-dispatch)
4. [Authentication & Credential Management](#4-authentication--credential-management)
5. [Model Routing & Selection](#5-model-routing--selection)
6. [Streaming Architecture](#6-streaming-architecture)
7. [Failover & Retry](#7-failover--retry)
8. [Provider Catalog & Discovery](#8-provider-catalog--discovery)
9. [Side-by-Side Comparison](#9-side-by-side-comparison)
10. [Key Takeaways for Vivianne](#10-key-takeaways-for-vivianne)

---

## 1. Architecture Overview

### JCode

```
jcode-provider-core/          ← Trait definitions, schemas, routing primitives
├── Provider trait            ← Async trait with complete(), streaming, etc.
├── transport.rs              ← Transient error classification (shared across providers)
├── auth_mode.rs              ← Dual-auth (OAuth vs API key) canonical definitions
├── failover.rs               ← Failover decisions, classification
├── fallback_pick.rs          ← Route selection after failure
├── retry_after.rs            ← Rate-limit retry timing
├── attempt_tracker.rs        ← Retry attempt counting
├── model_id.rs               ← Model ID parsing (1m suffix, slash base)
├── models.rs                 ← Static model list + capability/context lookups
├── openai_schema.rs          ← Shared OpenAI request/response schemas
├── reasoning.rs              ← Reasoning effort definitions
├── selection.rs              ← Provider selection, model routing
├── pricing.rs                ← Cost breakdown structures
├── catalog_refresh.rs        ← Model catalog refresh summaries
├── fingerprint.rs            ← Stable hashing for provider IDs
└── anthropic.rs              ← Anthropic-specific constants/mapping

jcode-provider-anthropic/     ← Anthropic-specific types & schemas
jcode-provider-anthropic-runtime/  ← Anthropic streaming implementation

jcode-provider-openai/        ← OpenAI-specific types & request builders
jcode-provider-openai-runtime/    ← OpenAI streaming (with Chat Completions & Responses API support)

jcode-provider-gemini/        ← Gemini-specific types
jcode-provider-gemini-runtime/    ← Gemini streaming implementation

jcode-provider-bedrock/       ← AWS Bedrock (SigV4 signed requests)

jcode-provider-antigravity/   ← Antigravity provider
jcode-provider-antigravity-runtime/

jcode-provider-copilot/       ← GitHub Copilot
jcode-provider-copilot-runtime/

jcode-provider-cursor/        ← Cursor IDE
jcode-provider-cursor-runtime/

jcode-provider-openrouter/    ← OpenRouter routing
jcode-provider-openrouter-runtime/

jcode-provider-metadata/      ← Provider registry & CLI metadata
jcode-provider-doctor/        ← Provider health probes & diagnostics
jcode-provider-env/           ← Environment-based provider configuration
```

**Design principle**: **A crate per provider** with a **-runtime** variant to separate types/schemas from streaming implementations. The core crate is dependency-free of any provider-specific code.

### Vivianne

```
crates/gui/src-tauri/src/ai/
├── mod.rs                    ← Module declarations
├── types.rs                  ← Message, Usage, StreamTokenEvent, roles
├── models.rs                 ← ModelInfo (id, name, provider, max_context)
├── auth.rs                   ← AuthState, Credential enum, keychain integration
├── env_api_keys.rs           ← Environment variable API key lookup
├── keychain_store.rs         ← OS keychain storage
├── oauth.rs                  ← OAuth flow
├── utils.rs / utils_ext.rs   ← Utility functions
├── completion.rs             ← One-shot non-streaming completion
├── embedding.rs              ← Embedding backend
├── provider_registry.rs      ← Single source of truth: provider → ApiKind + base URL
├── model_discovery.rs        ← Live model catalog from provider /models endpoints
└── api/
    ├── mod.rs                ← Module declarations
    ├── anthropic.rs          ← Anthropic streaming
    ├── openai_completions.rs ← OpenAI Chat Completions streaming
    ├── openai_responses.rs   ← OpenAI Responses API streaming
    ├── google.rs             ← Google Gemini streaming
    ├── mistral.rs            ← Mistral streaming
    ├── bedrock.rs            ← AWS Bedrock
    ├── azure_openai.rs       ← Azure OpenAI
    ├── cloudflare.rs         ← Cloudflare Workers AI
    ├── transform_messages.rs ← Message format conversion
    ├── simple_options.rs     ← Simple option parsing
    ├── github_copilot_headers.rs ← Copilot auth headers
    ├── openai_prompt_cache.rs← Prompt caching for OpenAI
    ├── openai_shared.rs      ← Shared OpenAI helper functions
    └── google_shared.rs      ← Shared Google helper functions
```

**Design principle**: **Single module tree** with `provider_registry.rs` as the dispatch table. One file per wire format (not per provider), since several providers share the same wire format.

---

## 2. Provider Trait & Abstraction Layer

### JCode — Formal `Provider` Trait

```rust
// jcode-provider-core/src/lib.rs
#[async_trait]
pub trait Provider: Send + Sync {
    /// Core streaming method
    async fn complete(&self, messages: &[Message], tools: &[ToolDefinition],
        system: &str, resume_session_id: Option<&str>) -> Result<EventStream>;

    /// Split system (static + dynamic) for better prompt caching
    async fn complete_split(&self, messages: &[Message], tools: &[ToolDefinition],
        system_static: &str, system_dynamic: &str,
        resume_session_id: Option<&str>) -> Result<EventStream> { ... }

    fn name(&self) -> &str;                  // stable machine id ("openrouter")
    fn display_name(&self) -> String;        // human label (overridable)
    fn model(&self) -> String;
    fn active_auth_method_label(&self) -> Option<&'static str>;
    fn active_resolved_credential(&self) -> Option<ResolvedCredential>;
    fn supports_image_input(&self) -> bool;
    fn set_model(&self, _model: &str) -> Result<()>;
    fn set_route_selection(&self, selection: &RouteSelection) -> Result<()>;
    fn available_models(&self) -> Vec<&'static str>;
    fn cycle_model(&self, direction: CycleDirection) -> String;
    fn flush(&self);                         // force-close connections
    fn context_window(&self) -> Option<usize>;
    // + token counting, retry tracking, etc.
}
```

Key features:
- **Single `EventStream` return type** — all providers produce the same stream type
- **`complete_split`** — separates static system prompt (cacheable) from dynamic (per-turn)
- **`active_resolved_credential()`** — answers "subscription or metered?" at the trait level
- **`set_route_selection()`** — supports provider orchestrators (OpenRouter multiplexing)
- **`cycle_model()`** — for cycling through available models

### Vivianne — Dispatch via `ApiKind` Enum

```rust
// provider_registry.rs
pub enum ApiKind {
    AnthropicMessages,
    OpenAiChat,            // OpenAI Chat Completions (and all compatible hosts)
    OpenAiResponses,       // OpenAI Responses API (direct OpenAI only)
    GoogleGenerative,
    MistralChat,
    Bedrock,
    AzureOpenAiResponses,
}
```

Vivianne has **no formal `Provider` trait**. Instead, the API layer is organized by `ApiKind` (wire format), and the streaming functions in `api/` are called directly from the agent loop with the provider-specific parameters. The `provider_registry.rs` resolves a provider name to its `ApiKind` and base URL.

```rust
pub fn provider_spec(provider: &str) -> Option<ProviderSpec> {
    Some(match provider {
        "anthropic" => ProviderSpec { default_base_url: Some("https://api.anthropic.com"), api_kind: AnthropicMessages },
        "openai" => ProviderSpec { default_base_url: Some("https://api.openai.com/v1"), api_kind: OpenAiResponses },
        "groq" => ProviderSpec { default_base_url: Some("https://api.groq.com/openai/v1"), api_kind: OpenAiChat },
        "xai" => ProviderSpec { ... },
        // ... 14+ providers
        _ => return None,
    })
}
```

---

## 3. Wire Protocol Dispatch

### JCode — EventStream + Provider trait

Each provider crate implements the `Provider` trait with its own `complete()`:

```
AnthropicProvider::complete()
  → POST /v1/messages (streaming: true)
  → parse SSE events → StreamEvent::Message/StreamEvent::ContentBlockDelta/...

OpenAIProvider::complete()
  → POST /v1/responses (stream: true)   [direct OpenAI]
  → POST /v1/chat/completions (stream: true)  [all compatible]

GeminiProvider::complete()
  → POST /v1/models/{model}:streamGenerateContent
```

The stream type is:
```rust
pub type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>;
```

A unified `StreamEvent` enum covers all providers:
```rust
pub enum StreamEvent {
    MessageStart,
    ContentBlockDelta { index: usize, delta: ContentBlockDelta },
    ContentBlockStop { index: usize },
    MessageStop(Message),
    Ping,
    Error(String),
}
```

### Vivianne — Direct Function Dispatch

Each file in `api/` exports a streaming function:

```
api/anthropic.rs    → anthropic_stream(messages, ...) → Vec<StreamTokenEvent>
api/openai_completions.rs → openai_stream(...) → Vec<StreamTokenEvent>
api/openai_responses.rs  → openai_responses_stream(...) → Vec<StreamTokenEvent>
api/google.rs       → google_stream(...) → Vec<StreamTokenEvent>
```

No unified return type — each function returns events differently. The agent loop (`loop_core.rs`) has a `match model.provider.as_str()` that calls the right function.

---

## 4. Authentication & Credential Management

### JCode — Dual-Auth with Canonical Vocabulary

```rust
// auth_mode.rs — single source of truth for OAuth vs API key
pub enum AuthMode { Oauth, ApiKey }

pub struct AuthRoute { provider: DualAuthProvider, mode: AuthMode }

// Unified parser: accepts any alias from ANY vocabulary
pub fn AuthRoute::parse(token: &str) -> Option<Self> {
    match token.to_lowercase().as_str() {
        "claude" | "anthropic" | "claude-oauth" | "anthropic-oauth"
            => Some(AuthRoute::anthropic(AuthMode::Oauth)),
        "claude-api" | "anthropic-api-key" | "api-key"
            => Some(AuthRoute::anthropic(AuthMode::ApiKey)),
        "openai" | "openai-oauth"
            => Some(AuthRoute::openai(AuthMode::Oauth)),
        "openai-api" | "openai-api-key"
            => Some(AuthRoute::openai(AuthMode::ApiKey)),
        _ => None,
    }
}
```

Key innovations:
- **Single `AuthRoute` struct** — one parse function for all vocabularies (env vars, CLI args, stable IDs, model prefixes)
- **`pinned_mode_for()`** — runtime-env override (e.g., `JCODE_RUNTIME_PROVIDER=claude-api`)
- **`runtime_env_auth_route()`** — read the pin from `JCODE_RUNTIME_PROVIDER`
- **`ResolvedCredential`** — `Oauth` vs `ApiKey` as a flyweight enum for billing surfaces
- Every surface (header tag, info-widget, model-switch) reads from the same authoritative source

### Vivianne — Simpler Credential Store

```rust
// auth.rs
pub enum Credential {
    ApiKey(ApiKeyCredential),
    OAuth(OAuthCredential),
}

pub struct AuthState {
    pub store: KeychainStore,
    pub auth_context: AuthContext,
}
```

- Credentials stored in OS keychain (`keychain_store.rs`)
- API keys also read from environment variables (`env_api_keys.rs`)
- OAuth flow in `oauth.rs`
- **No dual-auth vocabulary** — the provider name directly maps to the credential lookup
- **Nvidia sibling fallback** — `model_discovery.rs` tries "nvidia" and "nvidia-nim" as siblings when resolving keys

---

## 5. Model Routing & Selection

### JCode — Multi-Layered Routing

```rust
// selection.rs
pub enum ActiveProvider {
    Claude,
    ClaudeApi,
    OpenAI,
    OpenAIApi,
    OpenRouter,
    Copilot,
    Gemini,
    Cursor,
    Antigravity,
}
```

jcode's routing has **multiple layers**:

1. **Provider hint → ActiveProvider** — `parse_provider_hint(hint)` resolves a string like `"claude-api"` to `ActiveProvider::ClaudeApi`
2. **Model name → provider** — `model_name_for_provider(model)` determines which provider a model string belongs to
3. **Route selection** — `set_route_selection()` on a provider orchestrator activates the right runtime
4. **Fallback sequence** — if a provider fails, `fallback_sequence(provider)` returns the ordered fallback list
5. **Dedup** — `dedupe_model_routes()` removes duplicate model entries across providers
6. **Cycle model** — `cycle_model()` supports up/down cycling through available models

**Context window resolution**: The `context_limit_for_model_with_provider()` function maps model names to known context sizes, with per-provider overrides. Falls back to `DEFAULT_CONTEXT_LIMIT` (200K) for unknown models.

**Provider-specific catalogs**: The `models_catalog.rs` supports runtime-loaded catalogs (e.g., OpenRouter's API returns the full model list dynamically). These are merged with the static model lists.

### Vivianne — Live Discovery from Provider APIs

```rust
// model_discovery.rs
pub async fn discover_all(auth: &AuthState) -> Vec<DiscoveredModel> {
    try_fetch!("anthropic", |key| fetch_anthropic(&key));
    try_fetch!("openai", |key| fetch_openai_style("https://api.openai.com/v1", ...));
    try_fetch!("google", |key| fetch_google(&key));
    try_fetch!("groq", |key| fetch_openai_style("https://api.groq.com/openai/v1", ...));
    // ... 12+ providers
}
```

Vivianne takes a **live discovery** approach:
- Each provider's `/models` (or equivalent) endpoint is called at startup
- Providers without resolvable credentials are silently skipped
- **OpenRouter returns per-model pricing** (parsed from decimal strings)
- Results are tagged with capability labels (`is_image_model`, `supports_vision`) via pattern matching
- Frontend receives the live catalog and displays it in the model selector

**Frontend model router** (`lib/modelRouter.ts`): A rule-based heuristic that selects the best model from the discovered catalog based on prompt complexity and capability tags. Takes ~microseconds and requires zero API calls.

---

## 6. Streaming Architecture

### JCode — Unified EventStream

```
Provider::complete()
  ↓
EventStream (futures::Stream<Item = Result<StreamEvent>>)
  ↓
Caller processes StreamEvent::ContentBlockDelta { delta: ContentBlockDelta }
```

- All providers produce the **same stream type**
- `StreamEvent` is a tagged union covering message start/stop, content deltas, pings, errors
- `ContentBlockDelta` covers text, tool use, thinking, and other content types uniformly
- **Split system prompt** — `complete_split()` separates static from dynamic system context for better prompt caching
- Streaming is built on `futures::Stream`, so it's compatible with tokio select loops

### Vivianne — Tauri Event-Driven

```
Agent loop calls provider's stream function
  ↓
Provider pushes StreamTokenEvent via app.emit("stream-token", ...)
  ↓
Frontend receives Tauri events → updates message state
  ↓
agent_end / turn_end events signal completion
```

Vivianne's streaming is **Tauri event-driven**:
- Each chunk is emitted as a Tauri event to the frontend
- No unified stream type — each provider writes its own loop that calls `app.emit()`
- `StreamTokenEvent` has `delta`, `done`, and `is_thinking` fields
- The frontend reconstructs messages from the event stream
- Tool execution events (`tool_execution_start/update/end`) are interleaved with message events

---

## 7. Failover & Retry

### JCode — Sophisticated Multi-Layer

```rust
// failover.rs — classifies errors into decisions
pub enum FailoverDecision {
    None,
    RetryNextProvider,
    RetryAndMarkUnavailable,
}

pub fn classify_failover_error_message(message: &str) -> FailoverDecision {
    // Detects: context-length, rate-limit, quota, auth, overload, server-error, timeout, ...
}
```

JCode's failover has **four layers**:

1. **Transport-level** (`transport.rs`): Classifies connection errors (reset, DNS, TLS, HTTP/2 GOAWAY) as transient — retry on fresh connection
2. **Provider-level** (`failover.rs`): Classifies API errors into quotas, rate limits, auth failures, context length overflows
3. **Fallback route** (`fallback_pick.rs`): Picks the next provider in the fallback chain with credential probing
4. **User-facing prompt** (`ProviderFailoverPrompt`): Shows the user a summary ("Claude unavailable → switching to GPT-5 would resend ~5K tokens")

**Retry tracking**: `attempt_tracker.rs` tracks attempts per request, `retry_after.rs` parses `Retry-After` headers and rate-limit responses.

### Vivianne — Simple Retry in Agent Loop

Vivianne's `loop_core.rs` has a simpler retry mechanism:
- **`autoRetryEnabled`** frontend flag enables/disables retry
- **`retry_status`** event informs the frontend of retry attempts
- **`model_fallback`** event fires when the agent switches to a configured fallback model
- No fine-grained error classification — the loop retries on any failure up to `max_retries`

---

## 8. Provider Catalog & Discovery

### JCode — Hybrid (Static + Dynamic)

JCode maintains **static model lists** in `models.rs`:

```rust
pub const ALL_CLAUDE_MODELS: &[&str] = &[
    "claude-fable-5",
    "claude-opus-4-8",
    "claude-opus-4-6",
    "claude-opus-4-6[1m]",
    "claude-sonnet-5",
    // ...
];

pub const ALL_OPENAI_MODELS: &[&str] = &[
    "gpt-5.6-sol",
    "gpt-5.5-pro",
    "gpt-5.5",
    "gpt-5.1-codex-mini",
    // ...
];
```

Plus **runtime-loaded catalogs** for providers like OpenRouter that expose a `/models` endpoint with dynamic pricing. The two are merged.

### Vivianne — Fully Dynamic (Live API Calls Only)

Vivianne calls each provider's `/models` endpoint at startup. **No hardcoded model lists**. The discovery function:

1. Resolves credentials for each provider
2. Calls the `/models` endpoint (OpenAI-compatible, Anthropic, or Google)
3. Parses response, opportunistically reads `context_window`, `pricing`, etc.
4. Tags capability labels via pattern matching
5. Returns the unified list to the frontend

This means Vivianne **always has up-to-date model lists** but requires live API access to discover them.

---

## 9. Side-by-Side Comparison

| Aspect | **JCode** | **Vivianne** |
|---|---|---|
| **Trait abstraction** | Formal `Provider` trait with `complete()` + 15+ methods | No trait — dispatch via `ApiKind` enum + direct function calls |
| **Stream type** | Unified `EventStream = Pin<Box<dyn Stream<Result<StreamEvent>>>>` | Provider-specific; events emitted via Tauri IPC |
| **Provider separation** | **Crate per provider** (12 crates) with `-runtime` variant | **One module tree** (one file per wire format) |
| **Dual auth** | Formal `AuthMode` + `AuthRoute` + canonical vocabulary | Simple `Credential` enum (ApiKey / OAuth) |
| **Model lists** | **Hybrid** — static curated lists + runtime catalog | **Fully dynamic** — live `/models` calls only |
| **Model routing** | Multi-layer: provider hint → ActiveProvider → route selection → fallback chain | Frontend heuristic router (`modelRouter.ts`) |
| **Context window** | Per-model mapping with provider overrides (200K default) | From provider API (None when unavailable) |
| **Pricing** | Static per-model pricing | OpenRouter returns per-model pricing; others use pattern matching |
| **Failover** | 4-layer: transport → API error → fallback route → user prompt | Simple retry + fallback model config |
| **Prompt caching** | `complete_split()` separates static/dynamic system | `openai_prompt_cache.rs` for OpenAI, prompt cache headers for Anthropic |
| **Reasoning** | Formal `canonical_reasoning_effort()` per provider | `ThinkingLevel` enum with `simple_options.rs` |
| **Capability detection** | Static `ModelCapabilities` struct | Pattern-matched `classify_capabilities()` |
| **Provider count** | Claude, OpenAI, Gemini, Copilot, Cursor, OpenRouter, Bedrock, Antigravity | Anthropic, OpenAI, Groq, xAI, DeepSeek, Together, Fireworks, OpenRouter, Cerebras, NVIDIA, Google, Mistral, Bedrock, Azure, Cloudflare, ssm-moe |
| **Diagnostics** | `jcode-provider-doctor` crate with live probes + E2E tests | No dedicated diagnostic tool |
| **Synthetic providers** | `Antigravity` (test/tuning provider) | `ssm-moe` (local MoE engine) |

---

## 10. Key Takeaways for Vivianne

### Strengths of Vivianne's Approach

1. **Simple dispatch** — The `provider_registry.rs` + `ApiKind` enum is clean and easy to understand. No trait abstraction overhead.
2. **Live model discovery** — Always has up-to-date model lists, no manual curation needed.
3. **Broad provider support** — 15+ providers, including emerging ones like Groq, xAI, Cerebras, and NVIDIA NIM.
4. **Frontend model router** — The `modelRouter.ts` heuristic runs client-side in microseconds, zero API cost.
5. **Tauri IPC streaming** — Simple event-driven model works well for a desktop app.

### Gaps vs JCode

1. **No formal Provider trait** — Adding a new wire format requires modifying every call site in `loop_core.rs` that dispatches on `provider`. A trait would encapsulate this.
2. **No unified error classification** — `transport.rs` in jcode is a single file shared by all providers. Vivianne's error handling is ad-hoc per provider.
3. **No systematic failover** — jcode's 4-layer failover (transport → API → fallback → user prompt) is more robust. Vivianne relies on simple retry + a configured fallback model.
4. **No dual-auth vocabulary** — As more providers adopt OAuth (e.g., Anthropic's Claude subscription), jcode's `AuthMode` + `AuthRoute` system prevents credential routing bugs.
5. **No `EventStream` abstraction** — Each provider's streaming code duplicates the event loop + error handling pattern.
6. **No streaming subagent support** — jcode's `EventStream` + `Provider` trait enables clean multiplexing for multi-agent orchestration.
7. **No provider diagnostics** — jcode has a `jcode-provider-doctor` crate that runs live health probes and E2E tests. Vivianne has no equivalent.

### Recommended Upgrades

| Priority | Change | Effort | Impact |
|---|---|---|---|
| **1** | Add `transport.rs`-style transient error classifier shared by all providers | Low | Medium |
| **2** | Extract a `Provider` trait with unified `EventStream` return type | Medium | High |
| **3** | Add `AuthMode`/`AuthRoute` dual-auth vocabulary for Anthropic/OpenAI OAuth | Medium | Medium |
| **4** | Add systematic failover (classify error → fallback chain → user prompt) | High | High |
| **5** | Add provider diagnostics (live probe endpoint + E2E test suite) | Medium | Low |
| **6** | Extract provider-specific crates (if multi-agent orchestration is needed) | High | Medium |

---

*Analysis performed July 2026. JCode repository: [github.com/1jehuang/jcode](https://github.com/1jehuang/jcode)*
