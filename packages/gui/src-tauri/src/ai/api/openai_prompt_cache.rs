/// OpenAI prompt cache key helpers — from api/openai-prompt-cache.ts

pub const OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH: usize = 64;

/// Clamp an OpenAI prompt cache key to the maximum allowed length (64 chars).
/// Returns None if the input is None.
pub fn clamp_openai_prompt_cache_key(key: Option<&str>) -> Option<String> {
    let k = key?;
    let chars: Vec<char> = k.chars().collect();
    if chars.len() <= OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH {
        Some(k.to_string())
    } else {
        Some(chars[..OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH].iter().collect())
    }
}
