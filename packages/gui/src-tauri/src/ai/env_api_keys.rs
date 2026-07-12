use std::env;
use crate::ai::auth::ProviderEnv;


fn get_api_key_env_vars(provider: &str) -> Option<Vec<&'static str>> {
    match provider {
        "github-copilot" => Some(vec!["COPILOT_GITHUB_TOKEN"]),
        "anthropic" => Some(vec!["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"]),
        "ant-ling" => Some(vec!["ANT_LING_API_KEY"]),
        "openai" => Some(vec!["OPENAI_API_KEY"]),
        "azure-openai-responses" => Some(vec!["AZURE_OPENAI_API_KEY"]),
        "nvidia" => Some(vec!["NVIDIA_API_KEY"]),
    "nvidia-nim" => Some(vec!["NVIDIA_NIM_API_KEY", "NVIDIA_API_KEY"]),
        "deepseek" => Some(vec!["DEEPSEEK_API_KEY"]),
        "google" => Some(vec!["GEMINI_API_KEY"]),
        "google-vertex" => Some(vec!["GOOGLE_CLOUD_API_KEY"]),
        "groq" => Some(vec!["GROQ_API_KEY"]),
        "cerebras" => Some(vec!["CEREBRAS_API_KEY"]),
        "xai" => Some(vec!["XAI_API_KEY"]),
        "openrouter" => Some(vec!["OPENROUTER_API_KEY"]),
        "vercel-ai-gateway" => Some(vec!["AI_GATEWAY_API_KEY"]),
        "zai" => Some(vec!["ZAI_API_KEY"]),
        "zai-coding-cn" => Some(vec!["ZAI_CODING_CN_API_KEY"]),
        "mistral" => Some(vec!["MISTRAL_API_KEY"]),
        "minimax" => Some(vec!["MINIMAX_API_KEY"]),
        "minimax-cn" => Some(vec!["MINIMAX_CN_API_KEY"]),
        "moonshotai" | "moonshotai-cn" => Some(vec!["MOONSHOT_API_KEY"]),
        "huggingface" => Some(vec!["HF_TOKEN"]),
        "fireworks" => Some(vec!["FIREWORKS_API_KEY"]),
        "together" => Some(vec!["TOGETHER_API_KEY"]),
        "opencode" | "opencode-go" => Some(vec!["OPENCODE_API_KEY"]),
        "kimi-coding" => Some(vec!["KIMI_API_KEY"]),
        "cloudflare-workers-ai" | "cloudflare-ai-gateway" => Some(vec!["CLOUDFLARE_API_KEY"]),
        "xiaomi" => Some(vec!["XIAOMI_API_KEY"]),
        "xiaomi-token-plan-cn" => Some(vec!["XIAOMI_TOKEN_PLAN_CN_API_KEY"]),
        "xiaomi-token-plan-ams" => Some(vec!["XIAOMI_TOKEN_PLAN_AMS_API_KEY"]),
        "xiaomi-token-plan-sgp" => Some(vec!["XIAOMI_TOKEN_PLAN_SGP_API_KEY"]),
        _ => None,
    }
}

fn get_provider_env_value(key: &str, provider_env: Option<&ProviderEnv>) -> Option<String> {
    if let Some(env_map) = provider_env {
        if let Some(val) = env_map.get(key) {
            return Some(val.clone());
        }
    }
    env::var(key).ok()
}

pub fn find_env_keys(provider: &str, env_map: Option<&ProviderEnv>) -> Option<Vec<String>> {
    let env_vars = get_api_key_env_vars(provider)?;
    let found: Vec<String> = env_vars.into_iter()
        .filter(|&var| get_provider_env_value(var, env_map).is_some())
        .map(|s| s.to_string())
        .collect();
    
    if found.is_empty() { None } else { Some(found) }
}

pub fn has_vertex_adc_credentials(env_map: Option<&ProviderEnv>) -> bool {
    if let Some(path) = get_provider_env_value("GOOGLE_APPLICATION_CREDENTIALS", env_map) {
        return std::path::Path::new(&path).exists();
    }
    if let Some(mut home) = dirs::home_dir() {
        home.push(".config");
        home.push("gcloud");
        home.push("application_default_credentials.json");
        return home.exists();
    }
    false
}

pub fn get_env_api_key(provider: &str, env_map: Option<&ProviderEnv>) -> Option<String> {
    if let Some(keys) = find_env_keys(provider, env_map) {
        if let Some(first_key) = keys.first() {
            return get_provider_env_value(first_key, env_map);
        }
    }

    if provider == "google-vertex" {
        let has_creds = has_vertex_adc_credentials(env_map);
        let has_project = get_provider_env_value("GOOGLE_CLOUD_PROJECT", env_map).is_some() 
            || get_provider_env_value("GCLOUD_PROJECT", env_map).is_some();
        let has_location = get_provider_env_value("GOOGLE_CLOUD_LOCATION", env_map).is_some();
        
        if has_creds && has_project && has_location {
            return Some("adc".to_string());
        }
    }

    None
}
