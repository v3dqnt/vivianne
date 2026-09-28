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

            if next_char == 'u'
                && index + 5 < chars.len() {
                    let unicode_digits: String = chars[index + 2..=index + 5].iter().collect();
                    if unicode_digits.chars().all(|c| c.is_ascii_hexdigit()) {
                        repaired.push_str(&format!("\\u{}", unicode_digits));
                        index += 6;
                        continue;
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

/// Strip a tool-call-shaped JSON object a model sometimes leaks into its
/// plain-text answer instead of emitting a real `tool_calls` delta — seen
/// with weaker-tool-calling models on some OpenAI-compatible hosts, e.g.
/// `We'll call task twice.{"type":"toolCall","id":"...","name":"task",...}`.
/// Finds the first `"type":"toolCall"` marker, walks back to the enclosing
/// `{`, then forward tracking brace depth (ignoring braces inside strings)
/// to the matching `}`, and removes that whole span. Leaves the text
/// untouched if no such marker is present.
pub fn strip_leaked_tool_call_json(text: &str) -> String {
    let marker = text
        .find("\"type\":\"toolCall\"")
        .or_else(|| text.find("\"type\": \"toolCall\""));
    let Some(marker_pos) = marker else {
        return text.to_string();
    };
    let Some(open_brace) = text[..marker_pos].rfind('{') else {
        return text.to_string();
    };

    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut end = None;
    for (i, &b) in bytes.iter().enumerate().skip(open_brace) {
        if escape {
            escape = false;
            continue;
        }
        match b {
            b'\\' if in_string => escape = true,
            b'"' => in_string = !in_string,
            b'{' if !in_string => depth += 1,
            b'}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    match end {
        Some(end_pos) => {
            let mut result = text[..open_brace].to_string();
            result.push_str(&text[end_pos + 1..]);
            result.trim().to_string()
        }
        None => text.to_string(),
    }
}

/// Splits `<think>...</think>` chain-of-thought out of a live OpenAI-
/// compatible content stream. Every provider that speaks this wire format
/// (Groq, xAI, DeepSeek, Together, Fireworks, OpenRouter, Cerebras, NVIDIA
/// NIM, and any self-hosted vLLM/Ollama-style endpoint) shares one streaming
/// path — `openai_completions.rs` — but not every reasoning-model deployment
/// on it implements the `reasoning_content`/`reasoning` extended field
/// that path already handled. Many instead emit the whole chain-of-thought
/// as literal text wrapped in `<think>` tags inside the *normal* content
/// stream, which used to leak straight into the visible answer as raw tag
/// text instead of routing to the thinking/reasoning UI.
///
/// Stateful across chunks on purpose: a tag can land split across two SSE
/// chunks (one ending `...<th`, the next starting `ink>...`), so a
/// per-chunk-only check would miss it. `feed` buffers the longest suffix of
/// the current chunk that's still a valid partial match for whichever tag
/// it's waiting on, and resolves it against the next chunk instead of
/// guessing early.
#[derive(Default)]
pub struct ThinkTagParser {
    in_thinking: bool,
    pending: String,
}

impl ThinkTagParser {
    const OPEN: &'static str = "<think>";
    const CLOSE: &'static str = "</think>";

    /// Feed the next raw content delta. Returns classified segments in
    /// order — `(is_thinking, text)` — ready to emit/accumulate as-is; the
    /// tag markers themselves are consumed, never included in the output.
    pub fn feed(&mut self, chunk: &str) -> Vec<(bool, String)> {
        let mut buf = std::mem::take(&mut self.pending);
        buf.push_str(chunk);

        let mut out = Vec::new();
        let mut cursor = 0usize;

        loop {
            let tag = if self.in_thinking { Self::CLOSE } else { Self::OPEN };
            match buf[cursor..].find(tag) {
                Some(rel_pos) => {
                    let pos = cursor + rel_pos;
                    if pos > cursor {
                        out.push((self.in_thinking, buf[cursor..pos].to_string()));
                    }
                    cursor = pos + tag.len();
                    self.in_thinking = !self.in_thinking;
                }
                None => {
                    let remainder = &buf[cursor..];
                    // Longest suffix of `remainder` that's still a valid
                    // prefix of `tag` — held back in case the rest of the
                    // tag arrives in the next chunk. Bounded to tag.len()-1
                    // (a full match would've been caught above) and gated
                    // on char-boundary safety: `tag` is pure ASCII, so a
                    // real match can only ever start on a char boundary,
                    // but a *candidate* slice into arbitrary UTF-8 text
                    // must never cut a multi-byte sequence in half.
                    let mut hold_len = 0usize;
                    for n in 1..tag.len() {
                        if n > remainder.len() {
                            break;
                        }
                        let start = remainder.len() - n;
                        if !remainder.is_char_boundary(start) {
                            continue;
                        }
                        if tag.starts_with(&remainder[start..]) {
                            hold_len = n;
                        }
                    }
                    if hold_len > 0 {
                        let split_at = remainder.len() - hold_len;
                        out.push((self.in_thinking, remainder[..split_at].to_string()));
                        self.pending = remainder[split_at..].to_string();
                    } else if !remainder.is_empty() {
                        out.push((self.in_thinking, remainder.to_string()));
                    }
                    break;
                }
            }
        }

        out.retain(|(_, s)| !s.is_empty());
        out
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

/// Coarse classification of a failed provider call, for the credential pool
/// to decide how long to cool down the key that just failed. Reuses the
/// same two regexes `is_retryable_assistant_error` already runs — one
/// classifier, not two drifting copies of "what does a quota error look
/// like."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialErrorClass {
    /// Looks like a transient/provider-wide issue (rate limit, 5xx, network
    /// blip) rather than something specific to this one key.
    Retryable,
    /// Looks like this specific key is out of quota/billing — a longer
    /// cooldown makes sense than a generic retryable error.
    QuotaOrBilling,
    /// Doesn't match either known shape (e.g. an invalid/revoked key error) —
    /// still worth rotating away from, just without strong signal on why.
    Other,
}

pub fn classify_provider_error(error_message: &str) -> CredentialErrorClass {
    if non_retryable_provider_limit_error_pattern().is_match(error_message) {
        CredentialErrorClass::QuotaOrBilling
    } else if retryable_provider_error_pattern().is_match(error_message) {
        CredentialErrorClass::Retryable
    } else {
        CredentialErrorClass::Other
    }
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

#[cfg(test)]
mod strip_leaked_tool_call_json_tests {
    use super::strip_leaked_tool_call_json;

    #[test]
    fn strips_a_leaked_tool_call_after_prose() {
        let text = r#"We'll call task twice.{"type":"toolCall","id":"chatcmpl-tool-9d4ad8db8449fc0d","name":"task","arguments":{"description":"x","prompt":"y"}}"#;
        assert_eq!(strip_leaked_tool_call_json(text), "We'll call task twice.");
    }

    #[test]
    fn strips_a_leaked_tool_call_with_nested_objects_and_braces_in_strings() {
        let text = r#"Sure.{"type":"toolCall","id":"1","name":"task","arguments":{"note":"a { weird } string","nested":{"a":1}}} Done."#;
        assert_eq!(strip_leaked_tool_call_json(text), "Sure. Done.");
    }

    #[test]
    fn leaves_ordinary_text_untouched() {
        let text = "This is a completely normal answer with no tool call JSON in it.";
        assert_eq!(strip_leaked_tool_call_json(text), text);
    }

    #[test]
    fn leaves_unrelated_json_untouched() {
        let text = r#"Here's an example config: {"type":"config","enabled":true}"#;
        assert_eq!(strip_leaked_tool_call_json(text), text);
    }

    #[test]
    fn handles_spaced_colon_variant() {
        let text = r#"Calling it now. {"type": "toolCall", "id": "1", "name": "task"}"#;
        assert_eq!(strip_leaked_tool_call_json(text), "Calling it now.");
    }
}

#[cfg(test)]
mod think_tag_parser_tests {
    use super::ThinkTagParser;

    #[test]
    fn plain_text_with_no_tags_passes_through_as_non_thinking() {
        let mut p = ThinkTagParser::default();
        let out = p.feed("just a normal answer");
        assert_eq!(out, vec![(false, "just a normal answer".to_string())]);
    }

    #[test]
    fn a_full_tag_pair_in_one_chunk_is_classified_correctly() {
        let mut p = ThinkTagParser::default();
        let out = p.feed("before<think>reasoning here</think>after");
        assert_eq!(
            out,
            vec![
                (false, "before".to_string()),
                (true, "reasoning here".to_string()),
                (false, "after".to_string()),
            ]
        );
    }

    #[test]
    fn a_tag_split_across_two_chunks_is_still_detected() {
        let mut p = ThinkTagParser::default();
        let mut out = p.feed("before<th");
        out.extend(p.feed("ink>reasoning</think>after"));
        assert_eq!(
            out,
            vec![
                (false, "before".to_string()),
                (true, "reasoning".to_string()),
                (false, "after".to_string()),
            ]
        );
    }

    #[test]
    fn thinking_content_itself_split_across_many_chunks_stays_classified_as_thinking() {
        let mut p = ThinkTagParser::default();
        let mut out = p.feed("<think>step one, ");
        out.extend(p.feed("step two, "));
        out.extend(p.feed("step three</think>final answer"));
        assert_eq!(out[0], (true, "step one, ".to_string()));
        assert_eq!(out[1], (true, "step two, ".to_string()));
        assert_eq!(out[2], (true, "step three".to_string()));
        assert_eq!(out[3], (false, "final answer".to_string()));
    }

    #[test]
    fn a_lone_angle_bracket_that_never_becomes_a_tag_is_not_dropped() {
        // "a < b" is ordinary text, not a truncated tag — must not be
        // silently swallowed by the partial-match buffer.
        let mut p = ThinkTagParser::default();
        let mut out = p.feed("a < b and that's all");
        out.extend(p.feed("")); // flush: nothing more arrives
        let joined: String = out.into_iter().map(|(_, s)| s).collect();
        assert_eq!(joined, "a < b and that's all");
    }

    #[test]
    fn multibyte_utf8_straddling_a_chunk_boundary_does_not_panic_or_corrupt() {
        // "café" — 'é' is 2 bytes in UTF-8; split the chunk right through it.
        let mut p = ThinkTagParser::default();
        let mut out = p.feed("<think>caf");
        out.extend(p.feed("é reasoning</think>done"));
        let thinking: String = out.iter().filter(|(t, _)| *t).map(|(_, s)| s.clone()).collect();
        assert_eq!(thinking, "café reasoning");
    }

    #[test]
    fn unclosed_thinking_block_keeps_everything_classified_as_thinking() {
        // Stream ends mid-thought (e.g. truncated) — no </think> ever
        // arrives. Nothing should be misclassified as a normal answer.
        let mut p = ThinkTagParser::default();
        let out = p.feed("<think>never finishes");
        assert_eq!(out, vec![(true, "never finishes".to_string())]);
    }
}
