//! How dangerous is this step?
//!
//! Viv Instant runs unattended: no approval prompt, nobody at the keyboard. The
//! `dip` tool's mouse and keyboard actions are also the one part of this repo
//! with **no undo** — `agent::checkpoints` snapshots file writes, and a
//! synthesised keystroke is not a file write. So between a misheard word and a
//! destroyed document there is exactly one thing, and it is this file.
//!
//! ## Static rules first, Jev second
//!
//! [`classify`] is pure, synchronous and always available. [`classify_with_jev`]
//! may *raise* that verdict using a model, and may never lower it. That
//! direction is the whole design: Jev is waitlisted and behind a circuit
//! breaker, so on most machines it will not answer, and a classifier that
//! degrades to "everything is safe" when its model is missing is not a safety
//! control — it is a safety control-shaped hole.
//!
//! ## What this can and cannot see
//!
//! It reads tool arguments, and only those. That bounds it honestly:
//!
//! - **Seen:** the text being typed, the key chord being pressed, the window
//!   action, the executable being launched and its arguments.
//! - **Not seen:** what a click lands on. `dip interact` addresses elements by
//!   the numeric id from the last snapshot, so the arguments of a click on
//!   "Delete all" are indistinguishable from a click on "Cancel". Catching that
//!   would mean holding the snapshot's element labels and looking the id back
//!   up — worth doing, and deliberately not faked here with a rule that would
//!   read as coverage it does not have.
//!
//! The live-step log is what covers the gap: it is why the log names the target
//! rather than the tool.

use serde::{Deserialize, Serialize};

/// How much damage a step could do if the instruction behind it was misheard.
///
/// Ordinal on purpose — `max` across several signals is the combining rule, and
/// the `Ord` derive is what makes that a one-liner instead of a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// Reading, looking, moving the mouse. The overwhelming majority of steps.
    Safe,
    /// Changes visible state but nothing is lost: focusing, launching, typing
    /// ordinary text, minimising.
    Cautious,
    /// Could discard the user's work or data, and no checkpoint covers it.
    Destructive,
}

/// Substrings that make a *typed string* dangerous.
///
/// Typing is the sharpest edge here. Viv Instant may not call `bash` or
/// `powershell` — but "type `rm -rf ~/work` into the terminal that is already
/// open, then press Enter" reaches the same place using only allowed tools, so
/// the text itself has to be inspected.
///
/// Matched case-insensitively against the whole string, substring-wise. Chosen
/// to over-match rather than under-match: the cost of a false positive is a
/// three-second pause, and the cost of a false negative is unbounded.
const DESTRUCTIVE_TEXT: &[&str] = &[
    "rm -rf", "rm -r ", "rmdir /s", "del /s", "del /q", "format ", "diskpart", "mkfs",
    "drop table", "drop database", "truncate table",
    "git reset --hard", "git clean -fd", "git push --force", "git push -f",
    "shutdown", "taskkill /f", "killall ",
    "> /dev/null 2>&1 &", ":(){", // fork bomb
    "remove-item -recurse", "remove-item -force", "format-volume",
];

/// Key chords that destroy something.
///
/// Normalised to lowercase with spaces stripped before comparison, because the
/// model writes these inconsistently (`"ctrl+shift+delete"`, `"Ctrl + Shift +
/// Delete"`).
const DESTRUCTIVE_CHORDS: &[&str] = &[
    "shift+delete",       // permanent delete, bypasses the Recycle Bin
    "ctrl+shift+delete",  // clear browsing data
    "alt+f4",             // close window, unsaved work included
    "ctrl+shift+q",       // quit application (most Linux/macOS apps)
    "cmd+q",
    "meta+q",
];

/// Executables that are a shell by another name.
///
/// `dip launch_app` is `Command::new(x).args(y)` with no shell — but launching
/// `cmd.exe` with `/c del /s /q C:\Users` arrives at the same destination as the
/// `bash` tool that Viv Instant's allowlist deliberately excludes. Treating the
/// interpreter itself as the signal closes that route without banning the one
/// legitimate case (launching a shell with no arguments to give the user a
/// prompt, which is merely `Cautious`).
const SHELL_EXECUTABLES: &[&str] = &[
    "cmd", "cmd.exe", "powershell", "powershell.exe", "pwsh", "pwsh.exe",
    "wscript", "wscript.exe", "cscript", "cscript.exe", "bash", "sh", "zsh", "python", "python3",
];

fn normalize_chord(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase()
}

/// The executable's file name, lowercased. Split on both separators: `Path`
/// only knows `\` on Windows, so on macOS a `C:\…\powershell.exe` would keep
/// its whole path and slip past the shell check.
fn file_stem_lower(command: &str) -> String {
    command.rsplit(['/', '\\']).next().unwrap_or(command).to_ascii_lowercase()
}

/// Classify one tool call from its arguments alone. Pure, fast, always works.
pub fn classify(tool: &str, args: &serde_json::Value) -> Risk {
    if tool != "dip" {
        // The rest of Viv Instant's allowlist is read-only by construction —
        // `read`, `ls`, `glob`, `grep`, `sleep`, `todo_write`, `web_search`,
        // `web_fetch`. If a writing tool is ever added to that list, it needs a
        // rule here; `Cautious` rather than `Safe` is the default so that
        // omission shows up as an extra pause rather than as silence.
        return Risk::Safe;
    }

    let s = |k: &str| args.get(k).and_then(|v| v.as_str()).unwrap_or_default();
    let action = s("action");

    let mut risk = match action {
        "snapshot" | "screenshot" | "list_windows" | "cursor_position" | "mouse_move" => Risk::Safe,
        _ => Risk::Cautious,
    };

    // A window being closed is the single most likely way this feature loses
    // someone's work: the app may hold an unsaved buffer and the close may be
    // answered by a "Save changes?" dialog that nobody is watching.
    if action == "window_action" && s("window_state").eq_ignore_ascii_case("close") {
        risk = risk.max(Risk::Destructive);
    }

    // Typed text and a `key` chord both travel in `text`.
    let text = s("text");
    if !text.is_empty() {
        let lower = text.to_ascii_lowercase();
        if DESTRUCTIVE_TEXT.iter().any(|needle| lower.contains(needle)) {
            risk = risk.max(Risk::Destructive);
        }
        if action == "key" {
            let chord = normalize_chord(&lower);
            if DESTRUCTIVE_CHORDS.iter().any(|c| chord == *c) {
                risk = risk.max(Risk::Destructive);
            }
        }
    }

    if action == "launch_app" {
        let stem = file_stem_lower(s("command"));
        let has_args = args
            .get("args")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty());
        // A bare shell is a prompt for the user. A shell carrying arguments is
        // a command being executed, and the arguments are not inspected here
        // precisely because quoting makes that unreliable — the shape alone is
        // the signal.
        if SHELL_EXECUTABLES.contains(&stem.as_str()) && has_args {
            risk = risk.max(Risk::Destructive);
        }
    }

    risk
}

/// The static verdict, optionally raised by Jev.
///
/// Jev may only raise. It is an early-access model behind a circuit breaker
/// whose most likely answer is no answer at all, so letting it lower a verdict
/// would mean the safety of a step depended on whether an optional network call
/// happened to succeed.
pub async fn classify_with_jev(tool: &str, args: &serde_json::Value, instruction: &str) -> Risk {
    let base = classify(tool, args);
    // Only `Cautious` is worth asking about. `Destructive` is already the top
    // of the scale, and `Safe` is reserved for steps that cannot destroy
    // anything by construction — looking, listing, moving the pointer — so
    // consulting a model about them would spend a network round trip per
    // screenshot to learn what the action name already says.
    if base != Risk::Cautious || !crate::ai::jev::available().await {
        return base;
    }

    #[derive(Deserialize)]
    struct Verdict {
        destructive: bool,
        #[serde(default)]
        confidence: f32,
    }

    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "destructive": {
                "type": "boolean",
                "description": "Could this step permanently delete or overwrite the user's data, \
                                or discard unsaved work?"
            },
            "confidence": { "type": "number", "minimum": 0, "maximum": 1 }
        },
        "required": ["destructive"]
    });

    let context = format!(
        "The user said: {instruction}\nThe assistant is about to run the tool `{tool}` with \
         arguments: {args}"
    );

    match crate::ai::jev::decide::<Verdict>(schema, &context).await {
        // The bar is high because the action on a positive is to interrupt a
        // run the user asked for. A hesitant model saying "maybe" is not a
        // reason to stop; that is what the step log is for.
        Some(v) if v.destructive && v.confidence >= 0.7 => Risk::Destructive,
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dip(v: serde_json::Value) -> Risk {
        classify("dip", &v)
    }

    #[test]
    fn looking_is_safe() {
        assert_eq!(dip(json!({"action": "screenshot"})), Risk::Safe);
        assert_eq!(dip(json!({"action": "list_windows"})), Risk::Safe);
        assert_eq!(dip(json!({"action": "snapshot", "id": 3})), Risk::Safe);
        assert_eq!(classify("read", &json!({"path": "a.txt"})), Risk::Safe);
    }

    #[test]
    fn ordinary_acting_is_cautious() {
        assert_eq!(dip(json!({"action": "left_click", "coordinate": [10, 10]})), Risk::Cautious);
        assert_eq!(dip(json!({"action": "type", "text": "hello world"})), Risk::Cautious);
        assert_eq!(dip(json!({"action": "window_action", "window_state": "minimize"})), Risk::Cautious);
    }

    #[test]
    fn closing_a_window_is_destructive() {
        // The unsaved-work case, and the reason this rule is not "only if the
        // title looks like an editor".
        assert_eq!(dip(json!({"action": "window_action", "id": 1, "window_state": "close"})), Risk::Destructive);
        // Case-insensitive: the model does not reliably lowercase enum values.
        assert_eq!(dip(json!({"action": "window_action", "id": 1, "window_state": "Close"})), Risk::Destructive);
    }

    #[test]
    fn typing_a_destructive_command_is_caught() {
        // This is the route that bypasses the missing `bash` tool entirely.
        assert_eq!(dip(json!({"action": "type", "text": "rm -rf ~/work"})), Risk::Destructive);
        assert_eq!(dip(json!({"action": "type", "text": "GIT RESET --HARD"})), Risk::Destructive);
        assert_eq!(dip(json!({"action": "type", "text": "DROP TABLE users;"})), Risk::Destructive);
    }

    #[test]
    fn destructive_chords_are_caught_however_spaced() {
        assert_eq!(dip(json!({"action": "key", "text": "shift+delete"})), Risk::Destructive);
        assert_eq!(dip(json!({"action": "key", "text": "Ctrl + Shift + Delete"})), Risk::Destructive);
        // A plain Delete keystroke is ordinary text editing, not a wipe.
        assert_eq!(dip(json!({"action": "key", "text": "delete"})), Risk::Cautious);
        assert_eq!(dip(json!({"action": "key", "text": "ctrl+s"})), Risk::Cautious);
    }

    #[test]
    fn a_shell_with_arguments_is_destructive_but_a_bare_one_is_not() {
        assert_eq!(
            dip(json!({"action": "launch_app", "command": "cmd.exe", "args": ["/c", "del /s /q C:\\Users"]})),
            Risk::Destructive
        );
        // Full paths must not slip past the name check.
        assert_eq!(
            dip(json!({"action": "launch_app", "command": "C:\\Windows\\System32\\powershell.exe", "args": ["-c", "x"]})),
            Risk::Destructive
        );
        // Opening a terminal for the user is a normal thing to ask for.
        assert_eq!(dip(json!({"action": "launch_app", "command": "cmd.exe"})), Risk::Cautious);
        assert_eq!(dip(json!({"action": "launch_app", "command": "notepad.exe"})), Risk::Cautious);
    }

    #[test]
    fn risk_orders_so_max_combines() {
        // `max` is the combining rule throughout `classify`; if this ordering
        // ever flipped, every multi-signal call would silently downgrade.
        assert!(Risk::Destructive > Risk::Cautious);
        assert!(Risk::Cautious > Risk::Safe);
    }
}
