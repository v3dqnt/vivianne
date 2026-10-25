//! The `vivmod` protocol: serves a running mod's files to its sandboxed
//! frames — `vivmod://localhost/<key>/<path>` (on Windows WebView2,
//! `http://vivmod.localhost/<key>/<path>`) — and the SDK at `/_sdk/v1.js`.
//!
//! Only files inside the mod's own folder, only for mods that are on, and
//! every response carries the mod's CSP: scripts, styles and fetches limited
//! to the mod's own files plus the `network:` origins it was granted, and the
//! page sandboxed even if something loads it outside a frame.

use std::path::{Path, PathBuf};
use tauri::http;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use super::{ModInfo, Status};

pub const SCHEME: &str = "vivmod";

/// The SDK every mod page loads (`<script src="/_sdk/v1.js">`).
const SDK_V1: &str = include_str!("../../../../gui-next/src/modules/mods/sdk/v1.js");

/// How a frame's URL starts on this platform, the origin of every mod page.
pub fn origins() -> &'static [&'static str] {
    &["http://vivmod.localhost", "https://vivmod.localhost", "vivmod://localhost"]
}

/// Mod listings, reused for a moment: one page load fetches several files.
static CACHE: Mutex<Option<(Instant, Vec<ModInfo>)>> = Mutex::new(None);

fn running(key: &str) -> Option<ModInfo> {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let fresh = g.as_ref().is_some_and(|(t, _)| t.elapsed() < Duration::from_secs(2));
    if !fresh {
        *g = Some((Instant::now(), super::list()));
    }
    g.as_ref().and_then(|(_, mods)| mods.iter().find(|m| m.key == key && m.status == Status::On).cloned())
}

/// Forget the cached listing (after a mod is switched on or off).
pub fn invalidate() {
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The mod's CSP: everything from its own folder (and the SDK), fetches to
/// its granted origins, nothing else.
pub fn csp_for(key: &str, network: &[String]) -> String {
    let own: Vec<String> = origins().iter().map(|o| format!("{o}/{key}/ {o}/_sdk/")).collect();
    let own = own.join(" ");
    let mut connect: Vec<String> = origins().iter().map(|o| format!("{o}/{key}/")).collect();
    connect.extend(network.iter().cloned());
    let connect = connect.join(" ");
    format!(
        "default-src 'none'; script-src {own} 'unsafe-inline' 'unsafe-eval' blob:; style-src {own} 'unsafe-inline'; \
         img-src {own} data: blob:; font-src {own} data:; media-src {own} data: blob:; connect-src {connect}; \
         worker-src blob:; frame-src 'none'; form-action 'none'; base-uri 'none'; sandbox allow-scripts allow-forms"
    )
}

/// Resolve `/<key>/<path>` to a file inside the mod: no `..`, no escaping
/// through links.
pub fn resolve(mod_dir: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    if !super::manifest::is_inner_path(rel) {
        return None;
    }
    let root = mod_dir.canonicalize().ok()?;
    let file = root.join(rel).canonicalize().ok()?;
    (file.starts_with(&root) && file.is_file()).then_some(file)
}

fn respond(status: u16, ctype: &str, csp: Option<&str>, body: Vec<u8>) -> http::Response<Vec<u8>> {
    let mut b = http::Response::builder()
        .status(status)
        .header("Content-Type", ctype)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-store")
        // Module scripts and fonts are fetched in CORS mode from the frame's
        // opaque origin.
        .header("Access-Control-Allow-Origin", "*");
    if let Some(csp) = csp {
        b = b.header("Content-Security-Policy", csp);
    }
    b.body(body).unwrap_or_else(|_| http::Response::new(Vec::new()))
}

fn not_found(why: &str) -> http::Response<Vec<u8>> {
    respond(404, "text/plain; charset=utf-8", Some("default-src 'none'"), why.as_bytes().to_vec())
}

/// Mini-apps from chat (```widget fences), by id, kept for the session.
static WIDGETS: LazyLock<Mutex<HashMap<String, std::sync::Arc<str>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// What every widget page starts with: the theme as CSS variables, sizing to
/// its content, and `vivianne.followUp(text)` to ask in chat. It talks to the
/// app only by `postMessage`, which the app checks came from this frame.
const WIDGET_BOOT: &str = r#"<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<style>:root{color-scheme:light dark}html,body{margin:0;background:transparent;color:var(--viv-foreground,#111);font:14px/1.5 var(--viv-font,system-ui,sans-serif)}*{box-sizing:border-box}</style>
<script>(function(){var p=parent;window.vivianne={followUp:function(t){p.postMessage({viv:'followUp',text:String(t)},'*')}};
addEventListener('message',function(e){var d=e.data;if(!d||d.viv!=='theme')return;var r=document.documentElement;for(var k in d.tokens)r.style.setProperty('--viv-'+k.replace(/[A-Z]/g,function(c){return'-'+c.toLowerCase()}),d.tokens[k]);r.style.setProperty('--viv-font',d.font);r.style.colorScheme=d.mode;});
function size(){p.postMessage({viv:'resize',height:Math.ceil(document.documentElement.scrollHeight)},'*')}
addEventListener('load',function(){size();new ResizeObserver(size).observe(document.documentElement)});p.postMessage({viv:'ready'},'*')})();</script>
"#;

/// Keep a widget's page; returns the id it's served under (`/_widget/<id>`).
#[tauri::command]
pub async fn cmd_widget_put(html: String) -> Result<String, String> {
    if html.len() > 2_000_000 {
        return Err("That widget is too large to show.".into());
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    html.hash(&mut h);
    let id = format!("{:016x}", h.finish());
    let mut w = WIDGETS.lock().unwrap_or_else(|e| e.into_inner());
    // ponytail: unbounded only up to a few hundred widgets per session; an LRU if chats ever hold thousands.
    if w.len() > 500 {
        w.clear();
    }
    w.insert(id.clone(), html.into());
    Ok(id)
}

/// A widget gets scripts and styles inline, images from data: — and no
/// network, no frames, no forms that leave.
const WIDGET_CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' blob:; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; connect-src 'none'; worker-src blob:; frame-src 'none'; form-action 'none'; base-uri 'none'; sandbox allow-scripts";

/// Handle one request.
pub fn handle(request: &http::Request<Vec<u8>>) -> http::Response<Vec<u8>> {
    let path = request.uri().path();
    let path = percent_decode(path);
    let mut parts = path.trim_start_matches('/').splitn(2, '/');
    let key = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");
    if key == "_widget" {
        let html = WIDGETS.lock().unwrap_or_else(|e| e.into_inner()).get(rest).cloned();
        return match html {
            Some(body) => respond(200, "text/html; charset=utf-8", Some(WIDGET_CSP), format!("<!doctype html>{WIDGET_BOOT}{}", &*body).into_bytes()),
            None => not_found("this widget is gone; reload the chat"),
        };
    }
    if key == "_sdk" {
        return if rest == "v1.js" {
            respond(200, "text/javascript; charset=utf-8", None, SDK_V1.as_bytes().to_vec())
        } else {
            not_found("no such SDK")
        };
    }
    let Some(info) = running(key) else { return not_found("this mod isn't running") };
    let Some(file) = resolve(Path::new(&info.dir), rest) else { return not_found("no such file in this mod") };
    let network: Vec<String> = info
        .manifest
        .as_ref()
        .map(|m| m.permissions.iter().filter_map(|p| p.strip_prefix("network:")).map(String::from).collect())
        .unwrap_or_default();
    match std::fs::read(&file) {
        Ok(body) => respond(200, mime(rest), Some(&csp_for(key, &network)), body),
        Err(_) => not_found("couldn't read that file"),
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_resolve_only_inside_the_mod() {
        let root = std::env::temp_dir().join(format!("viv-modproto-{}", uuid::Uuid::new_v4()));
        let a = root.join("a");
        std::fs::create_dir_all(a.join("ui")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        std::fs::write(a.join("ui/p.html"), "x").unwrap();
        std::fs::write(a.join("index.html"), "x").unwrap();
        std::fs::write(root.join("b/secret.txt"), "x").unwrap();
        std::fs::write(root.join("outside.txt"), "x").unwrap();

        assert!(resolve(&a, "/ui/p.html").is_some());
        assert!(resolve(&a, "").is_some(), "the folder serves index.html");
        for bad in ["../b/secret.txt", "../outside.txt", "ui/../../outside.txt", "/../outside.txt", "C:/Windows/win.ini", "ui\\..\\..\\outside.txt"] {
            assert!(resolve(&a, bad).is_none(), "{bad}");
        }
        assert!(resolve(&a, "missing.html").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn percent_escapes_cant_smuggle_dot_dot() {
        assert_eq!(percent_decode("/u.a/%2e%2e/x"), "/u.a/../x");
        let root = std::env::temp_dir().join(format!("viv-modproto2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::write(root.join("x"), "x").unwrap();
        assert!(resolve(&root.join("a"), &percent_decode("%2e%2e/x")).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_csp_scopes_a_mod_to_its_own_files_and_grants() {
        let csp = csp_for("u.notes", &[]);
        assert!(csp.contains("http://vivmod.localhost/u.notes/"));
        assert!(!csp.contains("https: ") && !csp.contains("connect-src *"));
        assert!(csp.contains("sandbox allow-scripts"));
        let csp = csp_for("u.weather", &["https://api.weather.example".into()]);
        assert!(csp.contains("connect-src") && csp.contains("https://api.weather.example"));
    }
}
