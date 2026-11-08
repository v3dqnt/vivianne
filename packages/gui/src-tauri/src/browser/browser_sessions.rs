//! Signed-in browsing sessions — how Vivianne gets access to a site that
//! requires an account, and how that access survives a restart.
//!
//! ## What already worked, and what did not
//!
//! camofox-browser persists cookies and localStorage per `userId` to
//! `~/.camofox/profiles/<hash>/storage_state.json`, and Vivianne always browses
//! as one fixed `USER_ID`. So a sign-in is durable the moment it exists — it
//! survives the browser restarting, the app restarting, and a reboot. The gap
//! was never persistence. It was that there was no way to *establish* a
//! sign-in in the first place.
//!
//! ## Why importing cookies, and not typing a password
//!
//! There is no interactive route on Windows. The server's VNC plugin — the
//! documented "log in visually" path — needs Xvfb and x11vnc, which are X11 and
//! therefore Linux-only, and the browser otherwise runs headless with no window
//! for a human to type into.
//!
//! That constraint pushes toward the design that is correct anyway. The user
//! signs in inside their own browser, where their password manager and 2FA
//! already live, exports the resulting cookies, and hands over the session
//! only. Vivianne never sees, types, or stores a password, so there is no
//! credential for this app to leak. A "let the agent sign in for you" flow would
//! mean this process holding the user's password in memory to type it into a
//! form, which is strictly worse for a capability that buys nothing extra.
//!
//! For the same reason there is deliberately no automated sign-*up*. An account
//! created unattended belongs to nobody the site can verify, cannot be
//! recovered by the person who ends up depending on it, and usually breaches
//! the terms of the service being signed up to. A user who wants a separate
//! identity for the agent creates that account themselves and imports its
//! cookies here like any other.

use std::path::PathBuf;

use crate::camoufox::{base_url, client, ensure_running, not_running_err, USER_ID};

/// Where the user drops exported cookie files. Matches camofox's own default
/// (`CAMOFOX_COOKIES_DIR`) so a file placed here is visible to both sides.
pub fn cookies_dir() -> Result<PathBuf, String> {
    let dir = dirs::home_dir().ok_or("no home directory")?.join(".camofox").join("cookies");
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// One cookie in Playwright's shape, which is what `POST /sessions/:id/cookies`
/// expects.
#[derive(Debug, serde::Serialize)]
struct PwCookie {
    name: String,
    value: String,
    domain: String,
    path: String,
    expires: f64,
    #[serde(rename = "httpOnly")]
    http_only: bool,
    secure: bool,
}

/// Parse a Netscape `cookies.txt` file — the format every browser cookie-export
/// extension and `curl --cookie-jar` emits.
///
/// Seven tab-separated fields: domain, includeSubdomains, path, secure, expiry,
/// name, value. Lines starting with `#` are comments, with one exception that
/// matters enormously: `#HttpOnly_` is a real prefix on the domain field, not a
/// comment. Treating it as one drops precisely the cookies that carry the
/// sign-in, because session cookies are almost always HttpOnly — the import
/// would appear to succeed and the site would still show a login page.
fn parse_netscape_cookies(raw: &str) -> Vec<PwCookie> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let (line, http_only) = match line.strip_prefix("#HttpOnly_") {
            Some(rest) => (rest, true),
            None if line.starts_with('#') => continue,
            None => (line, false),
        };
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 7 {
            continue;
        }
        out.push(PwCookie {
            domain: f[0].to_string(),
            path: f[2].to_string(),
            secure: f[3].eq_ignore_ascii_case("TRUE"),
            // -1 is Playwright's "session cookie". A malformed expiry is far
            // safer as a session cookie than as a timestamp in the past, which
            // the browser would discard on arrival.
            expires: f[4].parse::<f64>().unwrap_or(-1.0),
            name: f[5].to_string(),
            value: f[6].to_string(),
            http_only,
        });
    }
    out
}

/// Import a Netscape cookie file into the browsing session.
///
/// `file` is a name inside `~/.camofox/cookies/`, or an absolute path.
pub async fn import_cookies(file: &str) -> Result<String, String> {
    ensure_running().await?;
    let dir = cookies_dir()?;
    let path = {
        let p = std::path::Path::new(file);
        if p.is_absolute() { p.to_path_buf() } else { dir.join(p) }
    };
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "couldn't read {} ({e}). Sign in using your own browser, export its cookies for that site \
             in Netscape/cookies.txt format, save the file into {}, then import it by name.",
            path.display(),
            dir.display()
        )
    })?;
    let cookies = parse_netscape_cookies(&raw);
    if cookies.is_empty() {
        return Err(format!(
            "{} contained no cookies in Netscape format (seven tab-separated fields per line). \
             Check the export was 'cookies.txt'/Netscape rather than JSON.",
            path.display()
        ));
    }
    let domains: std::collections::BTreeSet<&str> =
        cookies.iter().map(|c| c.domain.trim_start_matches('.')).collect();
    let count = cookies.len();
    let domain_list = domains.into_iter().collect::<Vec<_>>().join(", ");

    let base = base_url().await;
    let resp = client()
        .post(format!("{base}/sessions/{USER_ID}/cookies"))
        .json(&serde_json::json!({ "cookies": cookies }))
        .send()
        .await
        .map_err(|e| not_running_err(&base, e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("camofox rejected the cookies (HTTP {status}): {body}"));
    }
    Ok(format!(
        "Imported {count} cookie(s) for {domain_list}. camofox persists browser storage, so this \
         sign-in survives restarts until it expires or is reset."
    ))
}

/// Which sites the browsing session currently holds a sign-in for.
pub async fn session_status() -> Result<serde_json::Value, String> {
    ensure_running().await?;
    let base = base_url().await;
    let resp = client()
        .get(format!("{base}/sessions/{USER_ID}/storage_state"))
        .send()
        .await
        .map_err(|e| not_running_err(&base, e))?;
    if !resp.status().is_success() {
        // The export endpoint is registered by the VNC plugin, which ships
        // disabled. Its absence means the state cannot be *listed*, not that
        // nothing is stored — reporting "signed out" here would be a lie.
        return Ok(serde_json::json!({
            "available": false,
            "reason": "This build of camofox doesn't expose storage export — that endpoint comes from the VNC plugin, which is disabled by default. Sign-ins still persist; they just can't be listed."
        }));
    }
    let state: serde_json::Value =
        resp.json().await.map_err(|e| format!("unreadable storage state: {e}"))?;
    let domains: std::collections::BTreeSet<String> = state
        .get("cookies")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c.get("domain").and_then(|d| d.as_str()))
                .map(|d| d.trim_start_matches('.').to_string())
                .collect()
        })
        .unwrap_or_default();
    Ok(serde_json::json!({
        "available": true,
        "domains": domains.into_iter().collect::<Vec<_>>(),
        "cookieCount": state.get("cookies").and_then(|c| c.as_array()).map(|a| a.len()).unwrap_or(0),
    }))
}

/// Forget every stored sign-in — the browsing equivalent of signing out
/// everywhere, and irreversible. Only ever reached from an explicit user action
/// or an explicit tool call, never as cleanup.
pub async fn reset_session() -> Result<String, String> {
    ensure_running().await?;
    let base = base_url().await;
    let resp = client()
        .delete(format!("{base}/sessions/{USER_ID}/storage_state"))
        .send()
        .await
        .map_err(|e| not_running_err(&base, e))?;
    if !resp.status().is_success() {
        return Err(format!("couldn't reset the session (HTTP {})", resp.status()));
    }
    Ok("Signed out of every site in the browsing session and deleted the persisted state.".to_string())
}

// ── Tauri commands (Settings → Browser sessions) ─────────────────────────────

#[tauri::command]
pub async fn cmd_browser_session_status() -> Result<serde_json::Value, String> {
    session_status().await
}

#[tauri::command]
pub async fn cmd_browser_import_cookies(file: String) -> Result<String, String> {
    import_cookies(&file).await
}

#[tauri::command]
pub async fn cmd_browser_reset_session() -> Result<String, String> {
    reset_session().await
}

/// The directory to drop cookie exports into, so the UI can show it and open it.
#[tauri::command]
pub async fn cmd_browser_cookies_dir() -> Result<String, String> {
    Ok(cookies_dir()?.display().to_string())
}

/// Cookie files already sitting in that directory, ready to import.
#[tauri::command]
pub async fn cmd_browser_list_cookie_files() -> Result<Vec<String>, String> {
    let dir = cookies_dir()?;
    let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(Vec::new()) };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_netscape_file() {
        let raw = "# Netscape HTTP Cookie File\nexample.com\tTRUE\t/\tFALSE\t1800000000\tsid\tabc123\n";
        let c = parse_netscape_cookies(raw);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "sid");
        assert_eq!(c[0].value, "abc123");
        assert_eq!(c[0].domain, "example.com");
        assert!(!c[0].http_only);
    }

    #[test]
    fn httponly_prefix_is_a_domain_marker_not_a_comment() {
        // The bug this guards against is silent: skip these and the import
        // "succeeds" while dropping exactly the cookies that carry the login.
        let raw = "#HttpOnly_example.com\tTRUE\t/\tTRUE\t1800000000\tsession\tsecret\n";
        let c = parse_netscape_cookies(raw);
        assert_eq!(c.len(), 1);
        assert!(c[0].http_only);
        assert!(c[0].secure);
        assert_eq!(c[0].name, "session");
    }

    #[test]
    fn real_comments_and_short_lines_are_skipped() {
        let raw = "# a comment\n\nexample.com\tTRUE\t/\n";
        assert!(parse_netscape_cookies(raw).is_empty());
    }

    #[test]
    fn a_bad_expiry_becomes_a_session_cookie_not_an_expired_one() {
        let raw = "example.com\tTRUE\t/\tFALSE\tnot-a-number\tsid\tv\n";
        let c = parse_netscape_cookies(raw);
        assert_eq!(c[0].expires, -1.0);
    }

    #[test]
    fn leading_dot_domains_are_kept_verbatim_for_playwright() {
        // Playwright treats a leading dot as "include subdomains"; stripping it
        // here would narrow the cookie to the apex only.
        let raw = ".example.com\tTRUE\t/\tFALSE\t1800000000\tsid\tv\n";
        assert_eq!(parse_netscape_cookies(raw)[0].domain, ".example.com");
    }
}

// ── Explicit startup ──────────────────────────────────────────────────────

/// Bring the browser up and report what state it is in.
///
/// Every browser entry point already calls `ensure_running`, so this is not
/// what makes browsing work — it is what makes a slow or failed start legible.
/// A cold start can mean an `npm install`, a native-module rebuild, and a
/// ~300MB engine download before the first page ever loads. Folded invisibly
/// into a `web_preview` call, that reads as a hang, and the model's usual
/// response to a hang is to retry — which starts the queue again.
///
/// Doing it as its own step means the wait is attributable, the failure is
/// reported once with the server's own reason attached, and the model learns
/// the session's signed-in state before it plans around a login wall.
pub async fn start_and_report() -> Result<String, String> {
    let already = crate::camoufox::health().await;
    ensure_running().await?;

    let mut lines = Vec::new();
    lines.push(if already {
        "Browser already running.".to_string()
    } else {
        "Browser started.".to_string()
    });

    // Signed-in state is the part that changes what the model should do next,
    // so it is reported here rather than left for a second call.
    match session_status().await {
        Ok(v) => {
            let domains = v
                .get("domains")
                .and_then(|d| d.as_array())
                .map(|a| a.iter().filter_map(|d| d.as_str()).collect::<Vec<_>>())
                .unwrap_or_default();
            if v.get("available").and_then(|a| a.as_bool()) == Some(false) {
                lines.push(
                    "Signed-in sites: can't be listed on this build, but any previous sign-in is still active."
                        .to_string(),
                );
            } else if domains.is_empty() {
                lines.push("Signed-in sites: none.".to_string());
            } else {
                lines.push(format!("Signed in to: {}.", domains.join(", ")));
            }
        }
        // A status probe failing is not a reason to report the browser as
        // unusable — it is running, we just could not enumerate its cookies.
        Err(e) => lines.push(format!("(couldn't read signed-in state: {e})")),
    }
    Ok(lines.join("\n"))
}
