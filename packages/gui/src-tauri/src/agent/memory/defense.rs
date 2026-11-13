//! Memory defense: secrets and personal numbers are replaced with `[REDACTED:type]` before anything is written to memory.
//!
//! The patterns and the card checks are ported from Hindsight's `memory_defense.py` (vectorize-io/hindsight, MIT licence,
//! Copyright (c) Vectorize, Inc.), with these differences:
//! - Rust's `regex` has no look-around, so the "ASCII token boundary" Hindsight builds with look-behind and look-ahead is checked by hand
//!   on the characters either side of a match. The rule is the same: a secret butted against non-ASCII text (CJK prose) still matches, a
//!   piece of a longer ASCII token does not.
//! - Vivianne's own `key = value` rule (`password: ...`, `api_key=...`) stays, as `credential_assignment`.
//! - A match's text never leaves this module: callers get the type names and a count, so a log or a status line cannot leak what was found.
//!
//! Scope is high-confidence patterns with an unambiguous prefix. A secret with no recognisable shape (a password typed in prose) is not found;
//! a context-aware scanner would be the next step if leaks show up.

use regex::Regex;
use std::sync::OnceLock;

/// What to do with a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Replace each match with `[REDACTED:type]` and keep the rest.
    Redact,
    /// Refuse to remember the text at all when anything matched.
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Screened {
    pub text: String,
    /// The pattern names that matched, once each, in the order they were first found.
    pub types: Vec<&'static str>,
    /// How many matches were replaced.
    pub count: usize,
}

fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// A pattern: its name, whether its ends must sit on an ASCII token boundary, and the expression.
struct Rule {
    name: &'static str,
    bounded: bool,
    re: Regex,
}

fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        // Order matters: the more specific first, so `sk-ant-` is named before the generic `sk-`.
        let raw: &[(&str, bool, &str)] = &[
            // AI and LLM providers
            ("anthropic_key", true, r"sk-ant-[A-Za-z0-9_-]{20,}"),
            ("openai_project_key", true, r"sk-proj-[A-Za-z0-9_-]{48,}"),
            ("openai_admin_key", true, r"sk-admin-[A-Za-z0-9_-]{40,}"),
            ("openai_key", true, r"sk-[A-Za-z0-9_-]{20,}"),
            ("google_api_key", true, r"AIza[0-9A-Za-z_-]{35}"),
            ("google_oauth_token", true, r"ya29\.[0-9A-Za-z_-]{20,}"),
            ("xai_key", true, r"xai-[A-Za-z0-9]{40,}"),
            ("groq_key", true, r"gsk_[A-Za-z0-9]{20,}"),
            ("huggingface_token", true, r"hf_[A-Za-z0-9]{30,}"),
            ("replicate_token", true, r"r8_[A-Za-z0-9]{30,}"),
            ("perplexity_key", true, r"pplx-[A-Za-z0-9]{40,}"),
            ("databricks_token", true, r"dapi[A-Za-z0-9]{32}"),
            // Cloud providers
            ("aws_access_key", true, r"AKIA[0-9A-Z]{16}"),
            ("aws_session_token", true, r"ASIA[0-9A-Z]{16}"),
            ("aws_secret_key", false, r#"(?i)aws(.{0,20})?(secret|private)?[\s_-]?access[\s_-]?key[\s_-]?[:=][\s"']*[A-Za-z0-9/+=]{40}"#),
            ("digitalocean_token", true, r"dop_v1_[a-f0-9]{64}"),
            // Source control and CI
            ("github_fg_pat", true, r"github_pat_[A-Za-z0-9_]{60,}"),
            ("github_token", true, r"gh[pousr]_[A-Za-z0-9]{36}"),
            ("gitlab_pat", true, r"glpat-[A-Za-z0-9_-]{20,}"),
            ("npm_token", true, r"npm_[A-Za-z0-9]{30,}"),
            ("pypi_token", true, r"pypi-AgEIcHlwaS5vcmc[A-Za-z0-9_-]{20,}"),
            // Payment processors
            ("stripe_secret", true, r"sk_(?:live|test)_[A-Za-z0-9]{20,}"),
            ("stripe_restricted", true, r"rk_(?:live|test)_[A-Za-z0-9]{20,}"),
            ("square_token", true, r"sq0[a-z]{3}-[A-Za-z0-9_-]{22,}"),
            ("braintree_token", true, r"access_token\$production\$[a-z0-9]{16}\$[a-f0-9]{32}"),
            // Communication
            ("slack_token", true, r"xox[abprs]-[0-9A-Za-z-]{10,}"),
            ("slack_webhook", false, r"https://hooks\.slack\.com/services/T[A-Za-z0-9_]{8,}/B[A-Za-z0-9_]{8,}/[A-Za-z0-9_]{20,}"),
            ("twilio_api_key", true, r"SK[0-9a-fA-F]{32}"),
            ("twilio_account_sid", true, r"AC[0-9a-fA-F]{32}"),
            ("sendgrid_key", true, r"SG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}"),
            ("mailgun_key", true, r"key-[A-Za-z0-9]{32}"),
            ("discord_bot", true, r"[MNO][A-Za-z0-9]{23}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{27}"),
            ("telegram_bot", true, r"[0-9]{8,10}:[A-Za-z0-9_-]{35}"),
            ("shopify_token", true, r"shpat_[a-fA-F0-9]{32}"),
            // Connection strings with a password in them
            ("db_url_postgres", false, r"postgres(?:ql)?://[^\s:/@]+:[^\s/@]+@\S+"),
            ("db_url_mysql", false, r"mysql://[^\s:/@]+:[^\s/@]+@\S+"),
            ("db_url_mongodb", false, r"mongodb(?:\+srv)?://[^\s:/@]+:[^\s/@]+@\S+"),
            // Private keys and generic credentials
            ("private_key_pem", false, r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP )?PRIVATE KEY(?: BLOCK)?-----"),
            ("jwt", true, r"eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}"),
            // Vivianne's own: `password: hunter2hunter2`, `api_key = ...`
            ("credential_assignment", false, r#"(?i)\b(?:api[_-]?key|secret|token|password)\s*[:=]\s*['"]?[A-Za-z0-9_\-./+]{12,}['"]?"#),
            // Personal numbers
            ("ssn_us", true, r"\d{3}-\d{2}-\d{4}"),
        ];
        raw.iter().map(|(name, bounded, p)| Rule { name, bounded: *bounded, re: Regex::new(p).expect("static defense pattern must compile") }).collect()
    })
}

fn card_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:\d{4}[ -]?){3}\d{1,4}").expect("static card pattern must compile"))
}

fn uuid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[0-9a-fA-F]{8}-(?:[0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}").expect("static uuid pattern must compile"))
}

/// Whether `[start, end)` of `text` is not part of a longer ASCII token.
fn on_boundary(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back().is_none_or(|c| !token_char(c));
    let after = text[end..].chars().next().is_none_or(|c| !token_char(c));
    before && after
}

/// Card candidates must pass the Luhn check: digit count alone redacted timestamps and port lists. A run of one repeated digit passes Luhn
/// and is a placeholder, so it is refused too.
fn is_luhn_card(value: &str) -> bool {
    let digits: Vec<u32> = value.chars().filter(|c| c.is_ascii_digit()).filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    if digits.iter().all(|d| *d == digits[0]) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| if i % 2 == 1 { let x = d * 2; if x > 9 { x - 9 } else { x } } else { *d })
        .sum();
    sum % 10 == 0
}

fn digit_before(text: &str, start: usize) -> bool {
    let head = &text[..start];
    head.chars().next_back().is_some_and(|c| c.is_ascii_digit()) || head.ends_with(|c: char| c == '.') && head[..head.len() - 1].chars().next_back().is_some_and(|c| c.is_ascii_digit())
}

fn digit_after(text: &str, end: usize) -> bool {
    let tail = &text[end..];
    tail.chars().next().is_some_and(|c| c.is_ascii_digit()) || tail.strip_prefix('.').and_then(|t| t.chars().next()).is_some_and(|c| c.is_ascii_digit())
}

/// Spans of `text` that are a card number: a 13 to 19 digit run that is a whole number, passes Luhn, and is not part of a UUID.
fn card_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut from = 0;
    while from <= text.len() {
        let Some(m) = card_re().find_at(text, from) else { break };
        // A rejected candidate must not hide a valid one that starts a little later ("2026 4111 1111 1111 1111").
        let next = text[m.start()..].chars().next().map_or(m.start() + 1, |c| m.start() + c.len_utf8());
        from = next;
        if !on_boundary(text, m.start(), m.end()) || digit_before(text, m.start()) || digit_after(text, m.end()) || !is_luhn_card(m.as_str()) {
            continue;
        }
        let mut lo = m.start().saturating_sub(35);
        while !text.is_char_boundary(lo) {
            lo -= 1;
        }
        let mut hi = (m.end() + 36).min(text.len());
        while !text.is_char_boundary(hi) {
            hi += 1;
        }
        if uuid_re().find_iter(&text[lo..hi]).any(|u| lo + u.start() < m.end() && lo + u.end() > m.start()) {
            continue;
        }
        spans.push((m.start(), m.end()));
        from = m.end();
    }
    spans
}

/// Replace every secret and card number in `text` with `[REDACTED:type]`.
pub fn apply_redaction(text: &str) -> Screened {
    let mut out = text.to_string();
    let mut types: Vec<&'static str> = Vec::new();
    let mut count = 0usize;
    for rule in rules() {
        let spans: Vec<(usize, usize)> = rule.re.find_iter(&out).filter(|m| !rule.bounded || on_boundary(&out, m.start(), m.end())).map(|m| (m.start(), m.end())).collect();
        if spans.is_empty() {
            continue;
        }
        out = replace_spans(&out, &spans, rule.name);
        count += spans.len();
        if !types.contains(&rule.name) {
            types.push(rule.name);
        }
    }
    let cards = card_spans(&out);
    if !cards.is_empty() {
        out = replace_spans(&out, &cards, "credit_card");
        count += cards.len();
        types.push("credit_card");
    }
    Screened { text: out, types, count }
}

fn replace_spans(text: &str, spans: &[(usize, usize)], name: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (s, e) in spans {
        out.push_str(&text[at..*s]);
        out.push_str(&format!("[REDACTED:{name}]"));
        at = *e;
    }
    out.push_str(&text[at..]);
    out
}

/// What to remember for `text` under `policy`: the text to store, or `None` when the policy refuses it.
pub fn screen(text: &str, policy: Policy) -> Option<Screened> {
    let s = apply_redaction(text);
    if policy == Policy::Block && s.count > 0 {
        return None;
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A token built at run time, so this file never holds a string a secret scanner would flag.
    fn tok(prefix: &str, n: usize) -> String {
        format!("{prefix}{}", "Ab3dE6gH9jK2mN5pQ8sT1vW4yZ7".chars().cycle().take(n).collect::<String>())
    }
    fn hex(prefix: &str, n: usize) -> String {
        format!("{prefix}{}", "0a1b2c3d4e5f6789".chars().cycle().take(n).collect::<String>())
    }

    #[test]
    fn each_provider_token_is_named_and_replaced() {
        let cases: Vec<(&str, String)> = vec![
            ("anthropic_key", tok("sk-ant-", 30)),
            ("openai_project_key", tok("sk-proj-", 52)),
            ("openai_key", tok("sk-", 30)),
            ("google_api_key", tok("AIza", 35)),
            ("groq_key", tok("gsk_", 30)),
            ("huggingface_token", tok("hf_", 34)),
            ("aws_access_key", tok("AKIA", 16).to_uppercase()),
            ("github_token", tok("ghp_", 36)),
            ("gitlab_pat", tok("glpat-", 24)),
            ("npm_token", tok("npm_", 36)),
            ("stripe_secret", tok("sk_live_", 24)),
            ("slack_token", tok("xoxb-", 20)),
            ("sendgrid_key", format!("SG.{}.{}", tok("", 22), tok("", 43))),
            ("shopify_token", hex("shpat_", 32)),
            ("twilio_account_sid", hex("AC", 32)),
            ("telegram_bot", format!("123456789:{}", tok("", 35))),
            ("jwt", format!("eyJ{}.eyJ{}.{}", tok("", 20), tok("", 20), tok("", 20))),
        ];
        for (name, secret) in cases {
            let r = apply_redaction(&format!("the key is {secret} for prod"));
            assert!(r.types.contains(&name), "{name} not found in {secret}: {:?}", r.types);
            assert!(!r.text.contains(&secret), "{name} left in the text");
            assert!(r.text.contains(&format!("[REDACTED:{name}]")) && r.text.starts_with("the key is ") && r.text.ends_with(" for prod"), "{}", r.text);
        }
    }

    #[test]
    fn a_connection_string_with_a_password_and_a_private_key_header() {
        let r = apply_redaction("use postgres://admin:hunter2@db.internal:5432/app and mongodb+srv://u:p4ss@cluster.example.net/x");
        assert_eq!(r.types, vec!["db_url_postgres", "db_url_mongodb"]);
        assert!(!r.text.contains("hunter2") && !r.text.contains("p4ss"));
        assert_eq!(apply_redaction("postgres://db.internal:5432/app").count, 0, "no credentials, nothing to hide");
        assert_eq!(apply_redaction("-----BEGIN OPENSSH PRIVATE KEY-----\nabc").types, vec!["private_key_pem"]);
    }

    #[test]
    fn viviannes_own_assignment_rule_still_works() {
        let r = apply_redaction("password: correcthorsebattery and api_key=\"abcdef0123456789\"");
        assert_eq!(r.count, 2);
        assert!(r.types.contains(&"credential_assignment") && !r.text.contains("correcthorse"));
        assert_eq!(apply_redaction("the password field is required").count, 0, "prose about passwords is not a password");
    }

    #[test]
    fn a_secret_next_to_chinese_text_is_found_but_a_piece_of_a_longer_token_is_not() {
        let key = tok("sk_live_", 24);
        assert_eq!(apply_redaction(&format!("凭证为{key}请保密")).types, vec!["stripe_secret"], "CJK is not a token character");
        assert_eq!(apply_redaction(&format!("x{key}")).count, 0, "glued to an ASCII letter it is a different token");
        assert_eq!(apply_redaction(&format!("{key}_suffix")).count, 0, "same on the right");
        assert_eq!(apply_redaction(&format!("({key}),")).count, 1, "punctuation is a boundary");
    }

    #[test]
    fn the_specific_pattern_wins_over_the_generic_one() {
        let r = apply_redaction(&format!("{} and {}", tok("sk-ant-", 30), tok("sk-", 30)));
        assert_eq!(r.types, vec!["anthropic_key", "openai_key"]);
        assert_eq!(r.count, 2);
    }

    #[test]
    fn cards_need_luhn_and_ssns_a_boundary() {
        assert_eq!(apply_redaction("card 4111 1111 1111 1111 expires").types, vec!["credit_card"]);
        assert_eq!(apply_redaction("card 4111-1111-1111-1111.").count, 1);
        assert_eq!(apply_redaction("order 1234 5678 9012 3456").count, 0, "fails Luhn");
        assert_eq!(apply_redaction("0000 0000 0000 0000").count, 0, "a placeholder");
        assert_eq!(apply_redaction("ts 1700000000123456789012").count, 0, "part of a longer number");
        assert_eq!(apply_redaction("version 3.4111 1111 1111 1111").count, 0, "part of a decimal");
        assert_eq!(apply_redaction("2026 4111 1111 1111 1111").types, vec!["credit_card"], "a rejected start does not hide a later one");
        assert_eq!(apply_redaction("id 550e8400-e29b-41d4-a716-446655440000").count, 0, "a UUID is not a card");
        assert_eq!(apply_redaction("ssn 123-45-6789.").types, vec!["ssn_us"]);
        assert_eq!(apply_redaction("ref 9123-45-67890").count, 0);
    }

    #[test]
    fn ordinary_text_passes_untouched() {
        let t = "We chose SQLite over Postgres; the build is on port 1420 and the key idea is caching. Ran at 2026-11-12 10:42:01.";
        assert_eq!(apply_redaction(t), Screened { text: t.to_string(), types: vec![], count: 0 });
    }

    #[test]
    fn block_refuses_what_redact_would_clean() {
        let t = format!("remember {}", tok("ghp_", 36));
        assert!(screen(&t, Policy::Block).is_none());
        let kept = screen(&t, Policy::Redact).unwrap();
        assert_eq!(kept.text, "remember [REDACTED:github_token]");
        assert!(screen("nothing secret", Policy::Block).is_some());
    }

    #[test]
    fn redacting_twice_changes_nothing() {
        let once = apply_redaction(&format!("a {} b", tok("sk-", 30)));
        assert_eq!(apply_redaction(&once.text).text, once.text);
    }
}
