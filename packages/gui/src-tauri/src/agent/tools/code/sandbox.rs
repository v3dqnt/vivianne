//! Sandbox — least-privilege guards applied to every shell command the agent
//! runs (bash + powershell). Ported from kilocode's `kilo-sandbox` package,
//! which does real OS-level jailing (bubblewrap on Linux, seatbelt on macOS).
//! Windows has no equivalent primitive available from a plain Rust process
//! spawn (no unprivileged namespace/AppContainer jail without a lot more
//! machinery), so this is the portable subset of "deny by default":
//!
//! 1. **Env allowlisting** — strip credentials (API keys, tokens, secrets)
//!    from the child process's environment so a shell command can't read or
//!    exfiltrate them, even though the command itself was approved.
//! 2. **Destructive-command denylist** — reject a short list of
//!    unambiguously catastrophic commands (wipe a drive, recursively delete
//!    a filesystem root) before they ever spawn.
//!
//! ponytail: this is a static string/regex guard, not a real jail — it stops
//! obvious catastrophic commands and credential leakage, not a determined
//! sandbox escape (e.g. a script that reads env vars from `/proc` indirectly,
//! or a command split across multiple tool calls). Upgrade path if that
//! matters: real OS sandboxing (Job Objects + restricted tokens on Windows,
//! bubblewrap/seatbelt elsewhere), same as kilocode.

use regex::Regex;
use std::collections::HashMap;

/// Env var names that are never handed to a spawned shell, even though the
/// agent process itself holds them (for provider API calls). Matches by
/// suffix so it catches every provider's convention (ANTHROPIC_API_KEY,
/// AWS_SECRET_ACCESS_KEY, GITHUB_TOKEN, ...) without enumerating each one.
const DENIED_ENV_SUFFIXES: &[&str] =
    &["_KEY", "_TOKEN", "_SECRET", "_PASSWORD", "_CREDENTIAL", "_CREDENTIALS"];

fn is_denied_env_var(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    DENIED_ENV_SUFFIXES.iter().any(|suf| upper.ends_with(suf))
}

/// Build the environment a spawned shell command should see: everything the
/// agent process has, minus anything that looks like a credential.
pub fn filtered_env() -> HashMap<String, String> {
    std::env::vars().filter(|(k, _)| !is_denied_env_var(k)).collect()
}

/// Patterns for commands that are never legitimate as agent-issued shell
/// commands — only catastrophic, whole-system-destructive ones. Deliberately
/// short: this is a last-resort backstop, not a policy engine, so it must
/// never false-positive on normal dev work (`rm -rf node_modules`, `rm -rf
/// ./build`, etc. must all still work).
fn denylist_patterns() -> &'static [Regex] {
    use std::sync::OnceLock;
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let raw = [
            // rm -rf / (or //, or any depth of /, but not a subpath)
            r"(?i)\brm\s+.*-[a-z]*r[a-z]*f[a-z]*\s+/+\s*($|[;&|])",
            r"(?i)\brm\s+.*-[a-z]*f[a-z]*r[a-z]*\s+/+\s*($|[;&|])",
            // Windows: delete a drive root or Windows dir outright
            r"(?i)\b(del|erase|rd|rmdir)\b.*\s[a-z]:\\\\?\s*($|[;&|])",
            r"(?i)\brmdir\s+/s\s+/q\s+[a-z]:\\\\?\s*($|[;&|])",
            r"(?i)\bformat\s+[a-z]:",
            r"(?i)\bdiskpart\b",
            r"(?i)\bmkfs(\.|$|\s)",
            // fork bombs / disk-fill wipes
            r":\(\)\s*\{\s*:\s*\|\s*:\s*&\s*\}\s*;\s*:",
            r"(?i)\bdd\s+.*of=/dev/(sd|nvme|disk)",
        ];
        raw.iter().map(|p| Regex::new(p).expect("static sandbox pattern must compile")).collect()
    })
}

/// Returns `Some(reason)` if `command` matches a known-catastrophic pattern
/// and should be refused without spawning a shell at all.
pub fn check_command(command: &str) -> Option<&'static str> {
    denylist_patterns()
        .iter()
        .any(|re| re.is_match(command))
        .then_some("blocked: this command matches a known-destructive pattern (wipes a drive or filesystem root)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_catastrophic_commands() {
        assert!(check_command("rm -rf /").is_some());
        assert!(check_command("format C:").is_some());
        assert!(check_command("rmdir /s /q C:\\").is_some());
        assert!(check_command("diskpart").is_some());
    }

    #[test]
    fn allows_normal_dev_commands() {
        assert!(check_command("rm -rf node_modules").is_none());
        assert!(check_command("rm -rf ./dist").is_none());
        assert!(check_command("git status").is_none());
        assert!(check_command("cargo build").is_none());
        assert!(check_command("del build\\output.exe").is_none());
    }

    #[test]
    fn strips_credential_env_vars_by_suffix() {
        assert!(is_denied_env_var("ANTHROPIC_API_KEY"));
        assert!(is_denied_env_var("AWS_SECRET_ACCESS_KEY"));
        assert!(is_denied_env_var("GITHUB_TOKEN"));
        assert!(is_denied_env_var("HF_TOKEN"));
        assert!(!is_denied_env_var("PATH"));
        assert!(!is_denied_env_var("HOME"));
        assert!(!is_denied_env_var("NODE_ENV"));
    }
}
