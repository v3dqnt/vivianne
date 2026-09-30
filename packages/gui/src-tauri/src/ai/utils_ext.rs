use crate::ai::types::{AssistantMessage, Context, Usage};
#[allow(unused_imports)]


// ============================================================================
// overflow.rs — context window overflow detection
// ============================================================================

use regex::Regex;
use std::sync::OnceLock;

fn overflow_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"(?i)prompt is too long").unwrap(),
            Regex::new(r"(?i)request_too_large").unwrap(),
            Regex::new(r"(?i)input is too long for requested model").unwrap(),
            Regex::new(r"(?i)exceeds the context window").unwrap(),
            Regex::new(r"(?i)exceeds (?:the )?(?:model'?s )?maximum context length(?: of [\d,]+ tokens?|\s*\([\d,]+\))").unwrap(),
            Regex::new(r"(?i)input token count.*exceeds the maximum").unwrap(),
            Regex::new(r"(?i)maximum prompt length is \d+").unwrap(),
            Regex::new(r"(?i)reduce the length of the messages").unwrap(),
            Regex::new(r"(?i)maximum context length is \d+ tokens").unwrap(),
            Regex::new(r"(?i)exceeds (?:the )?maximum allowed input length of [\d,]+ tokens?").unwrap(),
            Regex::new(r"(?i)input \(\d+ tokens\) is longer than the model'?s context length \(\d+ tokens\)").unwrap(),
            Regex::new(r"(?i)exceeds the limit of \d+").unwrap(),
            Regex::new(r"(?i)exceeds the available context size").unwrap(),
            Regex::new(r"(?i)greater than the context length").unwrap(),
            Regex::new(r"(?i)context window exceeds limit").unwrap(),
            Regex::new(r"(?i)exceeded model token limit").unwrap(),
            Regex::new(r"(?i)too large for model with \d+ maximum context length").unwrap(),
            Regex::new(r"(?i)model_context_window_exceeded").unwrap(),
            Regex::new(r"(?i)prompt too long; exceeded (?:max )?context length").unwrap(),
            Regex::new(r"(?i)context[_ ]length[_ ]exceeded").unwrap(),
            Regex::new(r"(?i)too many tokens").unwrap(),
            Regex::new(r"(?i)token limit exceeded").unwrap(),
            Regex::new(r"(?i)^4(?:00|13)\s*(?:status code)?\s*\(no body\)").unwrap(),
        ]
    })
}

fn non_overflow_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            Regex::new(r"(?i)^(Throttling error|Service unavailable):").unwrap(),
            Regex::new(r"(?i)rate limit").unwrap(),
            Regex::new(r"(?i)too many requests").unwrap(),
        ]
    })
}

pub fn is_context_overflow(message: &AssistantMessage, context_window: Option<u32>) -> bool {
    // Case 1: error message pattern matching
    if message.stop_reason == "error" {
        if let Some(ref err_msg) = message.error_message {
            let is_non_overflow = non_overflow_patterns().iter().any(|p| p.is_match(err_msg.as_str()));
            if !is_non_overflow && overflow_patterns().iter().any(|p| p.is_match(err_msg.as_str())) {
                return true;
            }
        }
    }

    // Case 2: silent overflow (z.ai style) — usage.input > context_window
    if let Some(cw) = context_window {
        if message.stop_reason == "stop" {
            let input_tokens = message.usage.input + message.usage.cache_read;
            if input_tokens > cw {
                return true;
            }
        }

        // Case 3: Xiaomi MiMo style — length stop with zero output filling the window
        if message.stop_reason == "length" && message.usage.output == 0 {
            let input_tokens = message.usage.input + message.usage.cache_read;
            if input_tokens as f64 >= cw as f64 * 0.99 {
                return true;
            }
        }
    }

    false
}

pub fn get_overflow_patterns() -> Vec<String> {
    overflow_patterns().iter().map(|r| r.as_str().to_string()).collect()
}

// ============================================================================
// estimate.rs — token count estimation
// ============================================================================

const CHARS_PER_TOKEN: usize = 4;

#[derive(Debug, Clone)]
pub struct ContextUsageEstimate {
    /// Estimated total context tokens
    pub tokens: u32,
    /// Tokens reported by the most recent assistant usage block
    pub usage_tokens: u32,
    /// Estimated tokens after the most recent assistant usage block
    pub trailing_tokens: u32,
    /// Index of the message providing usage, or None
    pub last_usage_index: Option<usize>,
}
pub fn calculate_context_tokens(usage: &Usage) -> u32 {
    if usage.total_tokens > 0 {
        usage.total_tokens
    } else {
        usage.input + usage.output + usage.cache_read + usage.cache_write
    }
}

pub fn estimate_text_tokens(text: &str) -> u32 {
    text.len().div_ceil(CHARS_PER_TOKEN) as u32
}

pub fn estimate_context_tokens(context: &Context) -> ContextUsageEstimate {
    let messages = &context.messages;

    // Find last assistant message with valid usage
    let usage_info = messages.iter().enumerate().rev().find_map(|(i, msg)| {
        if msg.role != "assistant" { return None; }
        if msg.stop_reason.as_deref() == Some("aborted") || msg.stop_reason.as_deref() == Some("error") {
            return None;
        }
        let tokens = calculate_context_tokens(&msg.usage.clone().unwrap_or_default());
        if tokens > 0 { Some((tokens, i)) } else { None }
    });

    if let Some((usage_tokens, idx)) = usage_info {
        let trailing_tokens: u32 = messages[idx + 1..].iter().map(|m| {
            estimate_text_tokens(&m.content_text())
        }).sum();
        return ContextUsageEstimate {
            tokens: usage_tokens + trailing_tokens,
            usage_tokens,
            trailing_tokens,
            last_usage_index: Some(idx),
        };
    }

    // No usage info: estimate everything
    let mut prefix_tokens = 0u32;
    if let Some(ref sp) = context.system_prompt {
        prefix_tokens += estimate_text_tokens(sp.as_str());
    }

    let msg_tokens: u32 = messages.iter().map(|m| estimate_text_tokens(&m.content_text())).sum();
    let total = msg_tokens + prefix_tokens;

    ContextUsageEstimate {
        tokens: total,
        usage_tokens: 0,
        trailing_tokens: total,
        last_usage_index: None,
    }
}

// ============================================================================
// sanitize_unicode.rs — strip unpaired surrogate chars
// ============================================================================

/// Removes unpaired Unicode surrogate characters from a string.
/// Rust strings are always valid UTF-8 so surrogates cannot appear in normal
/// &str. This function is a no-op in Rust and kept for API parity.
pub fn sanitize_surrogates(text: &str) -> &str {
    text
}

/// Owned version that filters any replacement characters from lossy-decoded text.
pub fn sanitize_surrogates_owned(text: String) -> String {
    // Rust strings are valid UTF-8 — surrogates can't exist here.
    // If you receive bytes from an external source, decode with from_utf8_lossy first.
    text
}

// ============================================================================
// abort_signals.rs — cancellation tokens
// ============================================================================

use tokio::sync::watch;

/// Rust equivalent of a combined AbortController.
/// Senders can call `cancel()`, receivers poll `is_cancelled()`.
#[derive(Clone)]
pub struct CancellationToken {
    tx: watch::Sender<bool>,
    rx: watch::Receiver<bool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self { tx, rx }
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow()
    }

    pub fn receiver(&self) -> watch::Receiver<bool> {
        self.rx.clone()
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Combine multiple cancellation tokens into one.
pub fn combine_cancellation_tokens(tokens: Vec<CancellationToken>) -> CancellationToken {
    let combined = CancellationToken::new();
    let combined_clone = combined.clone();
    tokio::spawn(async move {
        loop {
            if tokens.iter().any(|t| t.is_cancelled()) {
                combined_clone.cancel();
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
    });
    combined
}

// ============================================================================
// diagnostics.rs — diagnostic info structs
// ============================================================================

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticErrorInfo {
    pub name: Option<String>,
    pub message: String,
    pub stack: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessageDiagnostic {
    #[serde(rename = "type")]
    pub kind: String,
    pub timestamp: u64,
    pub error: Option<DiagnosticErrorInfo>,
    pub details: Option<serde_json::Value>,
}

pub fn format_thrown_value(err: &dyn std::fmt::Display) -> String {
    err.to_string()
}

pub fn extract_diagnostic_error(err: &(dyn std::error::Error + 'static)) -> DiagnosticErrorInfo {
    DiagnosticErrorInfo {
        name: Some(std::any::type_name_of_val(err).to_string()),
        message: err.to_string(),
        stack: None,
        code: None,
    }
}

pub fn create_assistant_message_diagnostic(
    kind: &str,
    message: &str,
    details: Option<serde_json::Value>,
) -> AssistantMessageDiagnostic {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    AssistantMessageDiagnostic {
        kind: kind.to_string(),
        timestamp: now,
        error: Some(DiagnosticErrorInfo {
            name: None,
            message: message.to_string(),
            stack: None,
            code: None,
        }),
        details,
    }
}

// ============================================================================
// provider_env.rs — env var resolution with scoped overrides
// ============================================================================

use crate::ai::auth::ProviderEnv;

/// Resolve a provider env value from scoped overrides or process environment.
pub fn get_provider_env_value(name: &str, env: Option<&ProviderEnv>) -> Option<String> {
    if let Some(map) = env {
        if let Some(val) = map.get(name) {
            return Some(val.clone());
        }
    }
    std::env::var(name).ok()
}

// ============================================================================
// http_proxy.rs — HTTP proxy resolution
// ============================================================================

const DEFAULT_PROXY_PORTS: &[(&str, u16)] = &[
    ("ftp", 21),
    ("gopher", 70),
    ("http", 80),
    ("https", 443),
    ("ws", 80),
    ("wss", 443),
];

fn default_port_for(protocol: &str) -> u16 {
    DEFAULT_PROXY_PORTS
        .iter()
        .find(|(p, _)| *p == protocol)
        .map(|(_, port)| *port)
        .unwrap_or(0)
}

fn get_proxy_env(key: &str, env: Option<&ProviderEnv>) -> String {
    let lower = key.to_lowercase();
    let upper = key.to_uppercase();
    if let Some(map) = env {
        if let Some(v) = map.get(&lower) { return v.clone(); }
        if let Some(v) = map.get(&upper) { return v.clone(); }
    }
    std::env::var(&lower)
        .or_else(|_| std::env::var(&upper))
        .unwrap_or_default()
}

fn should_proxy_hostname(hostname: &str, port: u16, env: Option<&ProviderEnv>) -> bool {
    let no_proxy = get_proxy_env("no_proxy", env).to_lowercase();
    if no_proxy.is_empty() { return true; }
    if no_proxy == "*" { return false; }

    no_proxy.split([',', ' ']).all(|entry| {
        let entry = entry.trim();
        if entry.is_empty() { return true; }

        // Parse host:port entry
        let (proxy_host, proxy_port) = if let Some(colon) = entry.rfind(':') {
            let port_str = &entry[colon + 1..];
            if let Ok(p) = port_str.parse::<u16>() {
                (&entry[..colon], p)
            } else {
                (entry, 0u16)
            }
        } else {
            (entry, 0u16)
        };

        if proxy_port != 0 && proxy_port != port { return true; }

        let proxy_host = proxy_host.trim_start_matches('*');
        if proxy_host.starts_with('.') || entry.starts_with('.') {
            !hostname.ends_with(proxy_host)
        } else {
            hostname != proxy_host
        }
    })
}

fn get_proxy_for_url(target_url: &str, env: Option<&ProviderEnv>) -> String {
    let parsed = match url::Url::parse(target_url) {
        Ok(u) => u,
        Err(_) => return String::new(),
    };

    let protocol = parsed.scheme().to_string();
    let hostname = parsed.host_str().unwrap_or("").to_string();
    let port = parsed.port().unwrap_or_else(|| default_port_for(&protocol));

    if !should_proxy_hostname(&hostname, port, env) {
        return String::new();
    }

    let proxy = get_proxy_env(&format!("{}_proxy", protocol), env);
    let proxy = if proxy.is_empty() { get_proxy_env("all_proxy", env) } else { proxy };

    if proxy.is_empty() {
        return String::new();
    }

    if !proxy.contains("://") {
        format!("{}://{}", protocol, proxy)
    } else {
        proxy
    }
}

pub const UNSUPPORTED_PROXY_PROTOCOL_MESSAGE: &str =
    "Unsupported proxy protocol. SOCKS and PAC proxy URLs are not supported; use an HTTP or HTTPS proxy URL.";

pub fn resolve_http_proxy_url(target_url: &str, env: Option<&ProviderEnv>) -> Result<Option<url::Url>, String> {
    let proxy = get_proxy_for_url(target_url, env);
    if proxy.is_empty() {
        return Ok(None);
    }

    let proxy_url = url::Url::parse(&proxy)
        .map_err(|e| format!("Invalid proxy URL {:?}: {}", proxy, e))?;

    if proxy_url.scheme() != "http" && proxy_url.scheme() != "https" {
        return Err(format!(
            "{} Got {}:",
            UNSUPPORTED_PROXY_PROTOCOL_MESSAGE,
            proxy_url.scheme()
        ));
    }

    Ok(Some(proxy_url))
}
