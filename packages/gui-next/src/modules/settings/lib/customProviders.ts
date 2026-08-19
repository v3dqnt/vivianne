/**
 * User-defined providers: an arbitrary base URL + API key, speaking one of
 * Vivianne's existing wire formats (`universalProvider.ts`'s `apiKind`
 * switch already handles all four — a custom provider is just a
 * user-supplied `{baseUrl, apiKind}` pointed at someone else's server).
 *
 * Models are entered by hand, not discovered live — unlike the built-in
 * cloud providers, which fetch `/models` from each provider on every app
 * launch (`ai::model_discovery`). A custom endpoint might not even expose
 * that endpoint, and skipping the live call means adding one is instant and
 * offline-safe.
 *
 * Persisted as plain JSON in settings.json's `customProviders` array (via
 * `cmd_get_settings`/`cmd_save_settings`, which round-trip an opaque blob —
 * no Rust-side schema to keep in sync). The API key is deliberately NOT
 * part of that blob; it goes through the same OS-keychain path every other
 * provider's key uses (`auth_store_api_key`, keyed by this provider's own
 * `id`), which `resolve_api_key` on the Rust side already resolves for any
 * non-"local" provider string with zero backend changes needed.
 */

export type CustomApiFormat = 'anthropicMessages' | 'openAiChat' | 'openAiResponses' | 'googleGenerative'

export const API_FORMAT_OPTIONS: { value: CustomApiFormat; label: string }[] = [
  { value: 'anthropicMessages', label: 'Anthropic messages (/v1/messages)' },
  { value: 'openAiChat', label: 'OpenAI chat completions (/v1/chat/completions)' },
  { value: 'openAiResponses', label: 'OpenAI responses (/v1/responses)' },
  { value: 'googleGenerative', label: 'Google generative AI (/v1beta)' },
]

export function apiFormatLabel(format: CustomApiFormat): string {
  return API_FORMAT_OPTIONS.find((o) => o.value === format)?.label ?? format
}

export interface CustomModel {
  id: string
  contextWindow: number
  maxOutputTokens: number | null
}

/** The persisted shape — everything except whether a key is set. */
export interface CustomProviderConfig {
  id: string
  name: string
  baseUrl: string
  apiFormat: CustomApiFormat
  models: CustomModel[]
}

export interface CustomProvider extends CustomProviderConfig {
  hasKey: boolean
}
