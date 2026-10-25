//! Updates, changelog, and announcements.
//!
//! Three related things the app previously had no working path for:
//!
//! - **Auto-update.** `tauri-plugin-updater` was already registered and
//!   fully configured (signing key + GitHub release endpoint in
//!   `tauri.conf.json`), but nothing ever called it — no command, no UI, no
//!   startup check. A correctly configured updater that is never invoked
//!   ships exactly as many updates as no updater at all.
//! - **Changelog.** `CHANGELOG.md` is embedded at compile time rather than
//!   read from disk or fetched, so the notes shown can never describe a
//!   different build than the one running.
//! - **Announcements.** A small remote JSON feed, so something can be said
//!   to users between releases without shipping a build.
//!
//! Read state (which version's notes have been seen, which announcements
//! were dismissed) lives in `~/.viv/agent/updates.json`, deliberately its own
//! file: `cmd_save_settings` rewrites `settings.json` wholesale from a
//! frontend snapshot, so anything written out-of-band there gets clobbered.

use serde::{Deserialize, Serialize};

/// Compiled in, not read at runtime: notes must always describe the binary
/// they ship inside. `../../../CHANGELOG.md` is the repo root's.
const CHANGELOG_MD: &str = include_str!("../../../../../CHANGELOG.md");

/// Where announcements are fetched from. Overridable via the
/// `announcementsUrl` setting for anyone self-hosting a fork's feed.
const DEFAULT_ANNOUNCEMENTS_URL: &str =
    "https://raw.githubusercontent.com/v3dqnt/vivianne/main/announcements.json";

// ── Changelog ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogSection {
    /// "Added" / "Fixed" / "Changed" — the `###` heading.
    pub title: String,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogEntry {
    pub version: String,
    /// Whatever followed the version on the heading line; not parsed into a
    /// real date because the file's separator has varied (`—`, `-`) and a
    /// display string is all the UI needs.
    pub date: Option<String>,
    pub sections: Vec<ChangelogSection>,
}

/// Parse the embedded changelog.
///
/// Deliberately a small hand-rolled parser rather than a Markdown crate: the
/// only structure that matters is `## [version] — date` → `### Section` →
/// `- item`, and a full CommonMark parse would be a dependency plus an AST
/// walk to recover exactly that.
pub fn parse_changelog(md: &str) -> Vec<ChangelogEntry> {
    let mut entries: Vec<ChangelogEntry> = Vec::new();

    for raw in md.lines() {
        let line = raw.trim_end();

        if let Some(rest) = line.strip_prefix("## ") {
            let rest = rest.trim();
            // `## [1.7.2] — 2026-08-25`, or a bare `## 1.7.2`.
            let (version, date) = match (rest.find('['), rest.find(']')) {
                (Some(open), Some(close)) if close > open => {
                    let version = rest[open + 1..close].trim().to_string();
                    // The separator between version and date has varied
                    // across entries (em dash, en dash, hyphen), so strip
                    // whichever is there rather than matching one.
                    let date = rest[close + 1..].trim().trim_start_matches(['—', '–', '-']).trim();
                    (version, (!date.is_empty()).then(|| date.to_string()))
                }
                _ => (rest.to_string(), None),
            };
            entries.push(ChangelogEntry { version, date, sections: Vec::new() });
            continue;
        }

        // Everything below only makes sense inside an entry; a `###` or `-`
        // before the first `##` is preamble.
        let Some(entry) = entries.last_mut() else { continue };

        if let Some(title) = line.strip_prefix("### ") {
            entry.sections.push(ChangelogSection { title: title.trim().to_string(), items: Vec::new() });
            continue;
        }

        if let Some(item) = line.strip_prefix("- ") {
            // A release whose bullets aren't under any `###` still needs
            // somewhere to put them.
            if entry.sections.is_empty() {
                entry.sections.push(ChangelogSection { title: "Changes".to_string(), items: Vec::new() });
            }
            if let Some(section) = entry.sections.last_mut() {
                section.items.push(item.trim().to_string());
            }
        }
    }

    entries
}

#[tauri::command]
pub fn cmd_get_changelog() -> Vec<ChangelogEntry> {
    parse_changelog(CHANGELOG_MD)
}

#[tauri::command]
pub fn cmd_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ── Seen/dismissed state ──────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    /// The version whose release notes the user has already been shown, so
    /// "what's new" fires once per upgrade rather than on every launch.
    #[serde(default)]
    pub last_seen_version: Option<String>,
    #[serde(default)]
    pub dismissed_announcements: Vec<String>,
}

fn state_path() -> Result<std::path::PathBuf, String> {
    Ok(crate::agent::system::paths::viv_agent_dir()?.join("updates.json"))
}

fn read_state() -> UpdateState {
    state_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default()
}

fn write_state(state: &UpdateState) {
    let (Ok(path), Ok(json)) = (state_path(), serde_json::to_string_pretty(state)) else { return };
    let _ = std::fs::write(path, json);
}

#[tauri::command]
pub fn cmd_get_update_state() -> UpdateState {
    read_state()
}

/// Records that the running version's notes have been shown.
#[tauri::command]
pub fn cmd_mark_version_seen() {
    let mut state = read_state();
    state.last_seen_version = Some(env!("CARGO_PKG_VERSION").to_string());
    write_state(&state);
}

#[tauri::command]
pub fn cmd_dismiss_announcement(id: String) {
    let mut state = read_state();
    if !state.dismissed_announcements.contains(&id) {
        state.dismissed_announcements.push(id);
        // Unbounded growth would be silly for a list of dismissals; the feed
        // is small and old ids stop appearing in it.
        if state.dismissed_announcements.len() > 200 {
            let excess = state.dismissed_announcements.len() - 200;
            state.dismissed_announcements.drain(..excess);
        }
        write_state(&state);
    }
}

// ── Announcements ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Announcement {
    pub id: String,
    pub title: String,
    pub body: String,
    /// "info" | "warning" | "success" — free-form; the UI falls back to
    /// "info" for anything it doesn't recognise.
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// Only show on this exact app version, when set — lets an announcement
    /// target the build it's actually about.
    #[serde(default)]
    pub app_version: Option<String>,
}

/// Fetch the announcement feed, minus anything already dismissed or aimed at
/// a different version.
///
/// Network failure returns an empty list rather than an error: an
/// announcements feed being unreachable is not something to interrupt the
/// user about, and the UI treats "none" and "couldn't fetch" identically.
#[tauri::command]
pub async fn cmd_get_announcements() -> Vec<Announcement> {
    let url = crate::agent::tools::exec::read_settings_json()
        .await
        .get("announcementsUrl")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_ANNOUNCEMENTS_URL.to_string());

    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let Ok(resp) = client.get(&url).send().await else { return Vec::new() };
    if !resp.status().is_success() {
        return Vec::new();
    }
    let Ok(all) = resp.json::<Vec<Announcement>>().await else { return Vec::new() };

    let state = read_state();
    let version = env!("CARGO_PKG_VERSION");
    all.into_iter()
        .filter(|a| !state.dismissed_announcements.contains(&a.id))
        .filter(|a| a.app_version.as_deref().is_none_or(|v| v == version))
        .collect()
}

// ── Auto-update ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

/// Ask the configured endpoint whether a newer build exists. `None` means
/// "already current" — not an error.
#[tauri::command]
pub async fn cmd_check_for_update(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;

    // A dev build isn't an installed release and can't be updated in place;
    // asking the release feed only produces errors.
    if cfg!(debug_assertions) {
        return Ok(None);
    }
    let updater = app.updater().map_err(|e| format!("Updater unavailable: {e}"))?;
    match updater.check().await {
        Ok(Some(update)) => Ok(Some(UpdateInfo {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            notes: update.body.clone(),
            date: update.date.map(|d| d.to_string()),
        })),
        Ok(None) => Ok(None),
        Err(e) => Err(format!("Couldn't check for updates: {e}")),
    }
}

/// Download and install the available update, emitting progress, then
/// restart into it.
///
/// Re-checks rather than taking an `Update` handle from the caller: the
/// plugin's `Update` isn't serializable across the IPC boundary, so the
/// frontend can only ever hand back a version string, and trusting that
/// would mean installing whatever the endpoint serves at install time
/// regardless of what was actually reviewed.
#[tauri::command]
pub async fn cmd_install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    use tauri_plugin_updater::UpdaterExt;

    let updater = app.updater().map_err(|e| format!("Updater unavailable: {e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("Couldn't check for updates: {e}"))?
        .ok_or_else(|| "No update available.".to_string())?;

    let mut downloaded: usize = 0;
    let app_for_progress = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk;
                let _ = app_for_progress.emit(
                    "update-progress",
                    serde_json::json!({ "downloaded": downloaded, "total": total }),
                );
            },
            || {},
        )
        .await
        .map_err(|e| format!("Update failed to install: {e}"))?;

    // The installer has replaced the binary on disk; the running process is
    // still the old one, so a restart is what actually completes the update.
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_real_shipped_changelog() {
        let entries = parse_changelog(CHANGELOG_MD);
        assert!(!entries.is_empty(), "the embedded CHANGELOG.md produced no entries");
        let first = &entries[0];
        assert!(!first.version.is_empty());
        assert!(
            first.sections.iter().any(|s| !s.items.is_empty()),
            "the newest entry parsed with no items — heading/bullet format likely changed"
        );
    }

    #[test]
    fn parses_version_and_date_from_a_heading() {
        let entries = parse_changelog("## [1.2.3] — 2026-01-02\n\n### Fixed\n\n- A thing\n- Another\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].version, "1.2.3");
        assert_eq!(entries[0].date.as_deref(), Some("2026-01-02"));
        assert_eq!(entries[0].sections.len(), 1);
        assert_eq!(entries[0].sections[0].title, "Fixed");
        assert_eq!(entries[0].sections[0].items.len(), 2);
    }

    /// Bullets before any `###` still have to survive — an older entry in
    /// this very file is written that way.
    #[test]
    fn bullets_without_a_section_get_one() {
        let entries = parse_changelog("## [0.1.0]\n\n- Did a thing\n");
        assert_eq!(entries[0].sections.len(), 1);
        assert_eq!(entries[0].sections[0].items, vec!["Did a thing"]);
    }
}
