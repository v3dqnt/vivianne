/**
 * Cloud, key-based providers only — matches the ids in
 * `ai::provider_registry::provider_spec`. Deliberately excludes providers
 * that need more than a single API key to work (`bedrock`/`azure-openai-responses`
 * need account-specific config, `local`/`ssm-moe` are unauthenticated
 * localhost servers) — those get their own setup flow later rather than a
 * text field that can't actually make them work.
 */
export interface ProviderMeta {
  id: string
  label: string
}

export const CLOUD_PROVIDERS: ProviderMeta[] = [
  { id: 'anthropic', label: 'Anthropic' },
  { id: 'openai', label: 'OpenAI' },
  { id: 'google', label: 'Google' },
  { id: 'groq', label: 'Groq' },
  { id: 'xai', label: 'xAI' },
  { id: 'deepseek', label: 'DeepSeek' },
  { id: 'mistral', label: 'Mistral' },
  { id: 'together', label: 'Together AI' },
  { id: 'fireworks', label: 'Fireworks AI' },
  { id: 'openrouter', label: 'OpenRouter' },
  { id: 'cerebras', label: 'Cerebras' },
  { id: 'nvidia-nim', label: 'NVIDIA NIM' },
  { id: 'huggingface', label: 'Hugging Face' },
  { id: 'cloudflare-workers-ai', label: 'Cloudflare Workers AI' },
]
