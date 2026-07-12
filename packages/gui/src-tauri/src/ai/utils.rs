use reqwest::header::HeaderMap;
use std::collections::HashMap;
use regex::Regex;
use std::sync::OnceLock;
use serde::de::DeserializeOwned;
use tokio::sync::{mpsc, oneshot};
use std::pin::Pin;
use std::task::{Context, Poll};
use futures::stream::Stream;

// --- headers.rs translation ---

pub fn headers_to_record(headers: &HeaderMap) -> HashMap<String, String> {
    let mut result = HashMap::new();
    for (key, value) in headers.iter() {
        if let Ok(v) = value.to_str() {
            result.insert(key.as_str().to_string(), v.to_string());
        }
    }
    result
}

pub fn provider_headers_to_record(headers: Option<&HashMap<String, Option<String>>>) -> Option<HashMap<String, String>> {
    let headers = headers?;
    let mut result = HashMap::new();
    for (k, v) in headers {
        if let Some(val) = v {
            result.insert(k.clone(), val.clone());
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

// --- hash.rs translation ---

/// Fast deterministic hash to shorten long strings
pub fn short_hash(str: &str) -> String {
    let mut h1: u32 = 0xdeadbeef;
    let mut h2: u32 = 0x41c6ce57;
    
    for ch in str.encode_utf16() {
        h1 = (h1 ^ (ch as u32)).wrapping_mul(2654435761);
        h2 = (h2 ^ (ch as u32)).wrapping_mul(1597334677);
    }
    
    h1 = (h1 ^ (h1 >> 16))
        .wrapping_mul(2246822507)
        ^ (h2 ^ (h2 >> 13)).wrapping_mul(3266489909);
    h2 = (h2 ^ (h2 >> 16))
        .wrapping_mul(2246822507)
        ^ (h1 ^ (h1 >> 13)).wrapping_mul(3266489909);
        
    format!("{}{}", radix_36(h2), radix_36(h1))
}

fn radix_36(mut n: u32) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let mut result = String::new();
    while n > 0 {
        let rem = n % 36;
        let ch = if rem < 10 {
            (b'0' + rem as u8) as char
        } else {
            (b'a' + (rem - 10) as u8) as char
        };
        result.push(ch);
        n /= 36;
    }
    result.chars().rev().collect::<String>()
}

// --- json-parse.rs translation ---

/// Repairs malformed JSON string literals by:
/// - escaping raw control characters inside strings
/// - doubling backslashes before invalid escape characters
pub fn repair_json(json: &str) -> String {
    let mut repaired = String::with_capacity(json.len());
    let mut in_string = false;
    let chars: Vec<char> = json.chars().collect();
    let mut index = 0;

    while index < chars.len() {
        let char = chars[index];

        if !in_string {
            repaired.push(char);
            if char == '"' {
                in_string = true;
            }
            index += 1;
            continue;
        }

        if char == '"' {
            repaired.push(char);
            in_string = false;
            index += 1;
            continue;
        }

        if char == '\\' {
            if index + 1 >= chars.len() {
                repaired.push_str("\\\\");
                index += 1;
                continue;
            }
            let next_char = chars[index + 1];

            if next_char == 'u' {
                if index + 5 < chars.len() {
                    let unicode_digits: String = chars[index + 2..=index + 5].iter().collect();
                    if unicode_digits.chars().all(|c| c.is_ascii_hexdigit()) {
                        repaired.push_str(&format!("\\u{}", unicode_digits));
                        index += 6;
                        continue;
                    }
                }
            }

            let valid_escapes = ['"', '\\', '/', 'b', 'f', 'n', 'r', 't', 'u'];
            if valid_escapes.contains(&next_char) {
                repaired.push('\\');
                repaired.push(next_char);
                index += 2;
                continue;
            }

            repaired.push_str("\\\\");
            index += 1;
            continue;
        }

        if char.is_control() {
            match char {
                '\x08' => repaired.push_str("\\b"),
                '\x0C' => repaired.push_str("\\f"),
                '\n' => repaired.push_str("\\n"),
                '\r' => repaired.push_str("\\r"),
                '\t' => repaired.push_str("\\t"),
                _ => repaired.push_str(&format!("\\u{:04x}", char as u32)),
            }
        } else {
            repaired.push(char);
        }
        index += 1;
    }

    repaired
}

pub fn parse_json_with_repair<T: DeserializeOwned>(json: &str) -> Result<T, serde_json::Error> {
    match serde_json::from_str(json) {
        Ok(v) => Ok(v),
        Err(e) => {
            let repaired = repair_json(json);
            if repaired != json {
                serde_json::from_str(&repaired)
            } else {
                Err(e)
            }
        }
    }
}

pub fn parse_streaming_json<T: DeserializeOwned + Default>(partial_json: Option<&str>) -> T {
    let json = match partial_json {
        Some(j) if !j.trim().is_empty() => j,
        _ => return T::default(),
    };

    match parse_json_with_repair(json) {
        Ok(parsed) => parsed,
        Err(_) => T::default(),
    }
}

// --- error-body.rs translation ---

pub const MAX_PROVIDER_ERROR_BODY_CHARS: usize = 4000;

#[derive(Debug, Clone)]
pub struct NormalizedProviderError {
    pub status: Option<u16>,
    pub body: Option<String>,
    pub message: String,
    pub message_carries_body: bool,
}

pub fn normalize_provider_error(
    status: Option<u16>,
    body: Option<String>,
    message: String,
) -> NormalizedProviderError {
    let body_trimmed = body.as_ref().map(|b| b.trim().to_string()).filter(|b| !b.is_empty());
    let body_final = body_trimmed.map(|b| truncate_error_text(&b, MAX_PROVIDER_ERROR_BODY_CHARS));
    
    let message_carries_body = match &body_final {
        Some(b) => message.contains(b),
        None => true,
    };
    
    NormalizedProviderError {
        status,
        body: body_final,
        message,
        message_carries_body,
    }
}

pub fn format_provider_error(norm: &NormalizedProviderError, prefix: Option<&str>) -> String {
    if norm.message_carries_body || norm.status.is_none() || norm.body.is_none() {
        if let (Some(p), Some(s)) = (prefix, norm.status) {
            return format!("{} ({}): {}", p, s, norm.message);
        }
        return norm.message.clone();
    }
    
    // Both are guaranteed Some by the early return above, but bind them
    // explicitly with a fallback rather than unwrap — so error-formatting
    // code can never itself panic while trying to report a *different* error,
    // even if the guard above is later changed.
    let (Some(status), Some(body)) = (norm.status, norm.body.as_ref()) else {
        return norm.message.clone();
    };
    if let Some(p) = prefix {
        format!("{} ({}): {}", p, status, body)
    } else {
        format!("{}: {}", status, body)
    }
}

pub fn truncate_error_text(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        format!("{}... [truncated {} chars]", &text[..max_chars], text.len() - max_chars)
    }
}

// --- retry.rs translation ---

fn build_provider_error_pattern(patterns: &[&str]) -> Regex {
    let combined = patterns.join("|");
    Regex::new(&format!("(?i){}", combined)).unwrap()
}

fn non_retryable_provider_limit_error_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        build_provider_error_pattern(&[
            "GoUsageLimitError",
            "FreeUsageLimitError",
            "Monthly usage limit reached",
            "available balance",
            "insufficient_quota",
            "out of budget",
            "quota exceeded",
            "billing",
        ])
    })
}

fn retryable_provider_error_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        build_provider_error_pattern(&[
            "overloaded",
            r"rate.?limit",
            "too many requests",
            "429",
            "500",
            "502",
            "503",
            "504",
            r"service.?unavailable",
            r"server.?error",
            r"internal.?error",
            r"provider.?returned.?error",
            r"network.?error",
            r"connection.?error",
            r"connection.?refused",
            r"connection.?lost",
            "other side closed",
            "fetch failed",
            r"upstream.?connect",
            "reset before headers",
            "socket hang up",
            r"timed? out",
            "timeout",
            "terminated",
            r"websocket.?closed",
            r"websocket.?error",
            "ended without",
            "stream ended before message_stop",
            "http2 request did not get a response",
            "retry delay",
            "you can retry your request",
            "try your request again",
            "please retry your request",
        ])
    })
}

pub fn is_retryable_assistant_error(stop_reason: Option<&str>, error_message: Option<&str>) -> bool {
    if stop_reason != Some("error") {
        return false;
    }
    
    let err_msg = match error_message {
        Some(msg) => msg,
        None => return false,
    };
    
    if non_retryable_provider_limit_error_pattern().is_match(err_msg) {
        return false;
    }
    
    retryable_provider_error_pattern().is_match(err_msg)
}

// --- event-stream.rs translation ---

pub struct EventStream<T, R> {
    receiver: mpsc::UnboundedReceiver<T>,
    final_result_rx: oneshot::Receiver<R>,
}

impl<T, R> EventStream<T, R> {
    pub fn new<F, G>(
        is_complete: F,
        extract_result: G,
    ) -> (Self, EventStreamSender<T, R>)
    where
        F: FnMut(&T) -> bool + Send + 'static,
        G: FnMut(&T) -> R + Send + 'static,
    {
        let (tx, rx) = mpsc::unbounded_channel();
        let (result_tx, result_rx) = oneshot::channel();
        
        let sender = EventStreamSender {
            tx,
            result_tx: Some(result_tx),
            is_complete: Box::new(is_complete),
            extract_result: Box::new(extract_result),
        };
        
        (
            Self {
                receiver: rx,
                final_result_rx: result_rx,
            },
            sender
        )
    }
    
    pub async fn result(self) -> Result<R, oneshot::error::RecvError> {
        self.final_result_rx.await
    }
}

impl<T, R> Stream for EventStream<T, R> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

pub struct EventStreamSender<T, R> {
    tx: mpsc::UnboundedSender<T>,
    result_tx: Option<oneshot::Sender<R>>,
    is_complete: Box<dyn FnMut(&T) -> bool + Send>,
    extract_result: Box<dyn FnMut(&T) -> R + Send>,
}

impl<T, R> EventStreamSender<T, R> {
    pub fn push(&mut self, event: T) {
        if (self.is_complete)(&event) {
            let result = (self.extract_result)(&event);
            if let Some(tx) = self.result_tx.take() {
                let _ = tx.send(result);
            }
        }
        let _ = self.tx.send(event);
    }
    
    pub fn end(&mut self, result: Option<R>) {
        if let Some(r) = result {
            if let Some(tx) = self.result_tx.take() {
                let _ = tx.send(r);
            }
        }
    }
}
