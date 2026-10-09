//! DIP's structured mode on macOS — the counterpart of `super::dip` (Windows
//! UI Automation), built on the Accessibility API (`super::mac`).
//!
//! Same surface, same output: a snapshot lists a window's controls as
//! `[id] Role "Name" = "value"` lines, `interact` acts on an id, and windows
//! are listed, focused, arranged and launched by id. So the agent's prompts,
//! skills and habits carry over between the two platforms unchanged.
//!
//! Acting prefers the element's own AX actions (`AXPress`, setting
//! `AXValue`, `AXShowMenu`, `AXIncrement`…) and falls back to a real click at
//! the element's centre with its app in front. Every AX call has a 2 s
//! messaging timeout (`mac::Element::application`), so a hung app can't wedge
//! `dip`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::json;

use super::mac::{self, cf_to_string, Element, Window};
use super::tool_exec::ToolResult;

fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult { content: format!("Error: {}", msg.into()), is_error: true, details: None }
}

fn ok(text: impl Into<String>, details: Option<serde_json::Value>) -> ToolResult {
    ToolResult { content: text.into(), is_error: false, details }
}

/// Elements and windows are listed at most this many.
const MAX_LISTED: usize = 300;
const MAX_DEPTH: usize = 30;

struct Found {
    element: Element,
    role: String,
    name: String,
    center: (f64, f64),
}

#[derive(Default)]
struct Ctx {
    elements: Vec<Found>,
    window: Option<Window>,
    windows: Vec<Window>,
}

static CTX: Mutex<Option<Ctx>> = Mutex::new(None);

fn with_ctx<R>(f: impl FnOnce(&mut Ctx) -> R) -> R {
    let mut g = CTX.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Ctx::default))
}

/// Run AX work off the async runtime, bounded in time.
async fn blocking<R: Send + 'static>(job: impl FnOnce() -> R + Send + 'static) -> Result<R, String> {
    match tokio::time::timeout(Duration::from_secs(20), tokio::task::spawn_blocking(job)).await {
        Ok(Ok(r)) => Ok(r),
        Ok(Err(e)) => Err(format!("dip failed: {e}")),
        Err(_) => Err("the app didn't answer within 20s — it may be busy or hung".into()),
    }
}

fn require_trust() -> Result<(), String> {
    if mac::trusted() {
        return Ok(());
    }
    // Shows macOS's own prompt, which opens the right pane of System Settings.
    let _ = mac::request_trust();
    Err(mac::NOT_TRUSTED.into())
}

fn short_role(role: &str) -> String {
    role.strip_prefix("AX").unwrap_or(role).to_string()
}

fn is_interactive(role: &str) -> bool {
    matches!(
        role,
        "AXButton"
            | "AXCheckBox"
            | "AXRadioButton"
            | "AXPopUpButton"
            | "AXMenuButton"
            | "AXComboBox"
            | "AXTextField"
            | "AXTextArea"
            | "AXSearchField"
            | "AXLink"
            | "AXMenuItem"
            | "AXMenuBarItem"
            | "AXSlider"
            | "AXIncrementor"
            | "AXDisclosureTriangle"
            | "AXRow"
            | "AXCell"
            | "AXTab"
            | "AXWebArea"
            | "AXColorWell"
            | "AXDateField"
    )
}

fn clip(s: &str, max: usize) -> String {
    let s = s.replace(['\r', '\n'], " ");
    if s.chars().count() <= max {
        return s;
    }
    format!("{}…", s.chars().take(max).collect::<String>())
}

fn line(id: usize, depth: usize, role: &str, name: &str, value: &str, enabled: bool, focused: bool) -> String {
    let mut out = format!("{}[{id}] {role}", "  ".repeat(depth));
    if !name.trim().is_empty() {
        out.push_str(&format!(" \"{}\"", clip(name.trim(), 80)));
    }
    if !value.trim().is_empty() && value.trim() != name.trim() {
        out.push_str(&format!(" = \"{}\"", clip(value.trim(), 120)));
    }
    if !enabled {
        out.push_str(" (disabled)");
    }
    if focused {
        out.push_str(" *focused");
    }
    out
}

struct Snapshot {
    title: String,
    lines: Vec<String>,
    truncated: bool,
    elements: Vec<Found>,
}

const ATTRS: [&str; 10] = ["AXRole", "AXTitle", "AXDescription", "AXValue", "AXPosition", "AXSize", "AXEnabled", "AXFocused", "AXSubrole", "AXChildren"];

fn read_window(w: &Window) -> Snapshot {
    let mut elements: Vec<Found> = Vec::new();
    let mut lines = Vec::new();
    let mut truncated = false;
    // Depth-first; `depth` counts listed ancestors, so skipped containers
    // don't indent.
    let mut stack: Vec<(Element, usize, usize)> = w.element.elements("AXChildren").into_iter().rev().map(|e| (e, 0, 0)).collect();
    while let Some((el, depth, walk)) = stack.pop() {
        let v = el.attrs(&ATTRS);
        let s = |i: usize| v.get(i).and_then(|x| x.as_ref()).map(cf_to_string).unwrap_or_default();
        let role = s(0);
        let subrole = s(8);
        let title = s(1);
        let name = if title.trim().is_empty() { s(2) } else { title };
        // A password field's value is never read out.
        let value = if subrole == "AXSecureTextField" { String::new() } else { s(3) };
        let pos = v.get(4).and_then(|x| x.as_ref()).and_then(mac::ax_point).unwrap_or_default();
        let size = v.get(5).and_then(|x| x.as_ref()).and_then(mac::ax_size).unwrap_or_default();
        let enabled = s(6) != "false";
        let focused = s(7) == "true";
        let visible = size.width > 0.0 && size.height > 0.0;
        let mut child_depth = depth;
        let listed = visible && (is_interactive(&role) || !name.trim().is_empty() || !value.trim().is_empty() || focused);
        if listed {
            if elements.len() >= MAX_LISTED {
                truncated = true;
            } else {
                let id = elements.len();
                let shown_role = if subrole == "AXSearchField" { "SearchField".to_string() } else { short_role(&role) };
                lines.push(line(id, depth, &shown_role, &name, &value, enabled, focused));
                elements.push(Found {
                    element: el.clone(),
                    role: role.clone(),
                    name: name.clone(),
                    center: (pos.x + size.width / 2.0, pos.y + size.height / 2.0),
                });
                child_depth = depth + 1;
            }
        }
        if walk < MAX_DEPTH {
            for c in el.elements("AXChildren").into_iter().rev() {
                stack.push((c, child_depth, walk + 1));
            }
        }
    }
    Snapshot { title: format!("{} — {}", w.title, w.app), lines, truncated, elements }
}

/// The window a snapshot reads: the one named by id, else the target, else
/// the front app's front window — never Vivianne's own.
fn resolve_window(args: &serde_json::Value) -> Result<Window, String> {
    if let Some(id) = args.get("id").and_then(|v| v.as_u64()) {
        return with_ctx(|c| c.windows.get(id as usize).cloned()).ok_or_else(|| format!("no window with id {id} — call list_windows again"));
    }
    let all = mac::windows();
    if let Some((pid, title)) = mac::target() {
        if let Some(w) = all.iter().find(|w| w.pid == pid && w.title == title).or_else(|| all.iter().find(|w| w.pid == pid)) {
            return Ok(w.clone());
        }
    }
    let me = std::process::id() as i32;
    let front = mac::frontmost_pid().filter(|p| *p != me);
    front
        .and_then(|p| all.iter().find(|w| w.pid == p).cloned())
        .ok_or_else(|| "no target window yet — call list_windows and pass the window's id (or launch_app / focus_window first)".into())
}

pub(crate) async fn execute_dip_snapshot_tool(app: &tauri::AppHandle, args: &serde_json::Value) -> ToolResult {
    if let Err(e) = require_trust() {
        return err(e);
    }
    let job_args = args.clone();
    let started = Instant::now();
    let result = blocking(move || -> Result<(Snapshot, Window), String> {
        let w = resolve_window(&job_args)?;
        mac::set_target(w.pid, &w.title);
        // In front first: acting later may mean clicking its elements.
        mac::focus(&w);
        Ok((read_window(&w), w))
    })
    .await;
    let (snap, w) = match result {
        Ok(Ok(s)) => s,
        Ok(Err(e)) | Err(e) => return err(e),
    };
    let ms = started.elapsed().as_millis();
    if args.get("highlight").and_then(|v| v.as_bool()).unwrap_or(false) {
        let (x, y, wd, ht) = w.frame;
        crate::instant::pointer::highlight(app, x, y, wd, ht, Some(snap.title.clone())).ok();
    }
    let count = snap.lines.len();
    let (title, lines, truncated) = (snap.title.clone(), snap.lines.clone(), snap.truncated);
    with_ctx(|c| {
        c.elements = snap.elements;
        c.window = Some(w);
    });
    if lines.is_empty() {
        return ok(format!("'{title}' exposes no UI elements (it may draw its own UI, like a game or canvas) — use screenshot and pixel actions instead."), None);
    }
    let mut text = format!("'{title}' — {count} element(s), read in {ms} ms. Act on one with interact{{id, element_action}}; ids last until the next snapshot or find.\n");
    text.push_str(&lines.join("\n"));
    if truncated {
        text.push_str(&format!("\n… more elements not listed (over {MAX_LISTED}). Use find{{query}} to locate a specific one."));
    }
    ok(text, Some(json!({ "window": title, "count": count, "ms": ms })))
}

pub(crate) async fn execute_dip_find_tool(args: &serde_json::Value) -> ToolResult {
    let Some(query) = args.get("query").or_else(|| args.get("text")).and_then(|v| v.as_str()).map(|q| q.trim().to_lowercase()).filter(|q| !q.is_empty()) else {
        return err("'query' is required — part of the element's name, text, or type");
    };
    if let Err(e) = require_trust() {
        return err(e);
    }
    let args = args.clone();
    let result = blocking(move || -> Result<(Snapshot, Window), String> {
        let w = resolve_window(&args)?;
        mac::set_target(w.pid, &w.title);
        Ok((read_window(&w), w))
    })
    .await;
    let (snap, w) = match result {
        Ok(Ok(s)) => s,
        Ok(Err(e)) | Err(e) => return err(e),
    };
    let title = snap.title.clone();
    let hits: Vec<String> = snap.lines.iter().filter(|l| l.to_lowercase().contains(&query)).map(|l| l.trim_start().to_string()).collect();
    with_ctx(|c| {
        c.elements = snap.elements;
        c.window = Some(w);
    });
    if hits.is_empty() {
        return ok(format!("Nothing in '{title}' matches \"{query}\". snapshot lists everything."), None);
    }
    ok(format!("{} match(es) in '{title}':\n{}", hits.len(), hits.join("\n")), None)
}

// ── Acting ──

fn physical_click(pid: i32, at: (f64, f64), button: enigo::Button, count: u32) -> Result<(), String> {
    use enigo::{Coordinate, Direction, Enigo, Mouse, Settings};
    mac::activate(pid);
    let mut e = Enigo::new(&Settings::default()).map_err(|e| format!("input unavailable: {e}"))?;
    e.move_mouse(at.0.round() as i32, at.1.round() as i32, Coordinate::Abs).map_err(|e| e.to_string())?;
    for _ in 0..count.max(1) {
        e.button(button, Direction::Click).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn type_text(text: &str) -> Result<(), String> {
    use enigo::{Enigo, Keyboard, Settings};
    let mut e = Enigo::new(&Settings::default()).map_err(|e| format!("input unavailable: {e}"))?;
    e.text(text).map_err(|e| e.to_string())
}

fn select_all() -> Result<(), String> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut e = Enigo::new(&Settings::default()).map_err(|e| format!("input unavailable: {e}"))?;
    e.key(Key::Meta, Direction::Press).map_err(|e| e.to_string())?;
    let r = e.key(Key::Unicode('a'), Direction::Click).map_err(|e| e.to_string());
    let _ = e.key(Key::Meta, Direction::Release);
    r
}

fn interact(id: usize, action: &str, value: Option<String>) -> Result<(String, (f64, f64), Option<Window>), String> {
    let (element, role, name, center, window) = with_ctx(|c| {
        c.elements.get(id).map(|f| (f.element.clone(), f.role.clone(), f.name.clone(), f.center, c.window.clone()))
    })
    .ok_or_else(|| format!("no element [{id}] — call snapshot (or find) again"))?;
    // A stale element (the window changed) fails here instead of acting on
    // whatever now sits where it was.
    if element.attr("AXRole").is_none() {
        return Err(format!("element [{id}] is gone — the window changed; snapshot again"));
    }
    let label = if name.trim().is_empty() { format!("[{id}] {}", short_role(&role)) } else { format!("'{}'", clip(name.trim(), 60)) };
    let pid = element.pid().or_else(|| window.as_ref().map(|w| w.pid)).unwrap_or_default();
    let actions = element.actions();
    let has = |a: &str| actions.iter().any(|x| x == a);

    let done = match action {
        "click" | "invoke" => {
            if has("AXPress") && element.perform("AXPress") {
                format!("{} {label}.", if action == "click" { "Clicked" } else { "Invoked" })
            } else if action == "invoke" {
                return Err(format!("{label} can't be invoked — use click"));
            } else {
                physical_click(pid, center, enigo::Button::Left, 1)?;
                format!("Clicked {label}.")
            }
        }
        "double_click" => {
            physical_click(pid, center, enigo::Button::Left, 2)?;
            format!("Double-clicked {label}.")
        }
        "right_click" => {
            if has("AXShowMenu") && element.perform("AXShowMenu") {
                format!("Opened the menu of {label}.")
            } else {
                physical_click(pid, center, enigo::Button::Right, 1)?;
                format!("Right-clicked {label}.")
            }
        }
        "focus" => {
            mac::activate(pid);
            if !element.set_bool("AXFocused", true) {
                return Err(format!("couldn't focus {label}"));
            }
            format!("Focused {label}.")
        }
        "set_value" => {
            let value = value.ok_or("'value' is required for set_value")?;
            // Setting AXValue replaces the text outright; read it back, since
            // some controls accept the call and ignore it.
            let direct = element.set_string("AXValue", &value) && element.string("AXValue") == value;
            if !direct {
                mac::activate(pid);
                if !element.set_bool("AXFocused", true) {
                    physical_click(pid, center, enigo::Button::Left, 1)?;
                }
                std::thread::sleep(Duration::from_millis(60));
                select_all()?;
                type_text(&value)?;
            }
            format!("Set {label} to {:?}.", clip(&value, 80))
        }
        "toggle" => {
            if !(has("AXPress") && element.perform("AXPress")) {
                return Err(format!("{label} isn't a toggle — use click"));
            }
            format!("Toggled {label} (now {}).", element.string("AXValue"))
        }
        "select" => {
            if element.set_bool("AXSelected", true) || (has("AXPick") && element.perform("AXPick")) || (has("AXPress") && element.perform("AXPress")) {
                format!("Selected {label}.")
            } else {
                return Err(format!("{label} isn't selectable — use click"));
            }
        }
        "expand" | "collapse" => {
            let want = action == "expand";
            let ok = element.set_bool("AXExpanded", want)
                || element.set_bool("AXDisclosing", want)
                || (want && has("AXShowMenu") && element.perform("AXShowMenu"))
                || (has("AXPress") && element.perform("AXPress"));
            if !ok {
                return Err(format!("{label} doesn't expand — use click"));
            }
            format!("{} {label}.", if want { "Expanded" } else { "Collapsed" })
        }
        "scroll_into_view" => {
            if !(has("AXScrollToVisible") && element.perform("AXScrollToVisible")) {
                return Err(format!("{label} can't be scrolled to"));
            }
            format!("Scrolled {label} into view.")
        }
        "increment" | "decrement" => {
            let a = if action == "increment" { "AXIncrement" } else { "AXDecrement" };
            if !element.perform(a) {
                return Err(format!("{label} can't be stepped"));
            }
            format!("{} {label} (now {}).", if action == "increment" { "Incremented" } else { "Decremented" }, element.string("AXValue"))
        }
        other => {
            return Err(format!(
                "unknown element_action '{other}' — use click, double_click, right_click, invoke, focus, set_value, toggle, select, expand, collapse, increment, decrement, or scroll_into_view"
            ))
        }
    };
    Ok((done, center, window))
}

pub(crate) async fn execute_dip_interact_tool(app: &tauri::AppHandle, args: &serde_json::Value) -> ToolResult {
    let Some(id) = args.get("id").and_then(|v| v.as_u64()).map(|v| v as usize) else {
        return err("'id' is required (an element id from the last snapshot or find).");
    };
    if let Err(e) = require_trust() {
        return err(e);
    }
    let action = args.get("element_action").and_then(|v| v.as_str()).unwrap_or("click").to_string();
    let value = args.get("value").or_else(|| args.get("text")).and_then(|v| v.as_str()).map(str::to_string);
    match blocking(move || interact(id, &action, value)).await {
        Ok(Ok((msg, at, window))) => {
            if let Some(w) = window {
                mac::set_target(w.pid, &w.title);
            }
            // The live pointer: where the agent acted. Drawing only.
            let _ = crate::instant::pointer::point(app, at.0, at.1, None);
            ok(msg, None)
        }
        Ok(Err(e)) | Err(e) => err(e),
    }
}

/// The text of whatever has keyboard focus, for checking typing landed.
pub(crate) async fn focused_text() -> Option<String> {
    if !mac::trusted() {
        return None;
    }
    blocking(|| mac::focused_element().map(|e| e.string("AXValue")).filter(|t| !t.is_empty())).await.ok().flatten()
}

pub(crate) async fn execute_dip_read_text_tool(args: &serde_json::Value) -> ToolResult {
    if let Err(e) = require_trust() {
        return err(e);
    }
    let id = args.get("id").and_then(|v| v.as_u64()).map(|v| v as usize);
    let result = blocking(move || -> Result<String, String> {
        let element = match id {
            Some(i) => with_ctx(|c| c.elements.get(i).map(|f| f.element.clone())).ok_or_else(|| format!("no element [{i}] — snapshot again"))?,
            None => with_ctx(|c| c.window.as_ref().map(|w| w.element.clone())).ok_or("no window read yet — snapshot first")?,
        };
        let own = element.string("AXValue");
        if !own.trim().is_empty() {
            return Ok(own);
        }
        // A window holds its text in a text area or document inside it; read
        // the fullest one. Otherwise, every label under it, in order.
        let mut best = String::new();
        let mut names = Vec::new();
        let mut stack = vec![(element, 0usize)];
        let mut seen = 0;
        while let Some((el, depth)) = stack.pop() {
            seen += 1;
            if seen > 4000 {
                break;
            }
            let v = el.attrs(&["AXRole", "AXValue", "AXTitle", "AXDescription"]);
            let s = |i: usize| v.get(i).and_then(|x| x.as_ref()).map(cf_to_string).unwrap_or_default();
            let (role, value) = (s(0), s(1));
            if matches!(role.as_str(), "AXTextArea" | "AXTextField" | "AXStaticText") && value.len() > best.len() && role != "AXStaticText" {
                best = value.clone();
            }
            for t in [s(2), s(3), if role == "AXStaticText" { value } else { String::new() }] {
                if !t.trim().is_empty() {
                    names.push(t);
                }
            }
            if depth < MAX_DEPTH {
                for c in el.elements("AXChildren").into_iter().rev() {
                    stack.push((c, depth + 1));
                }
            }
        }
        Ok(if best.trim().is_empty() { names.join("\n") } else { best })
    })
    .await;
    match result {
        Ok(Ok(t)) if t.trim().is_empty() => ok("It has no readable text — try a screenshot.", None),
        Ok(Ok(t)) => {
            const MAX: usize = 60_000;
            if t.len() > MAX {
                let mut end = MAX;
                while !t.is_char_boundary(end) {
                    end -= 1;
                }
                ok(format!("{}\n… ({} more characters)", &t[..end], t.len() - MAX), None)
            } else {
                ok(t, None)
            }
        }
        Ok(Err(e)) | Err(e) => err(e),
    }
}

// ── Windows ──

pub(crate) async fn execute_dip_list_windows_tool() -> ToolResult {
    if let Err(e) = require_trust() {
        return err(e);
    }
    let windows = match blocking(mac::windows).await {
        Ok(w) => w,
        Err(e) => return err(e),
    };
    if windows.is_empty() {
        return ok("No open windows found.", None);
    }
    let target = mac::target();
    let lines: Vec<String> = windows
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let title = if w.title.is_empty() { "(untitled)" } else { &w.title };
            let mut l = format!("[{i}] {} — {}", clip(title, 90), w.app);
            if w.minimized {
                l.push_str(" (minimized)");
            }
            if target.as_ref().is_some_and(|(p, t)| *p == w.pid && *t == w.title) {
                l.push_str(" *target");
            }
            l
        })
        .collect();
    let list: Vec<serde_json::Value> = windows.iter().enumerate().map(|(i, w)| json!({ "id": i, "title": w.title, "process": w.app })).collect();
    let count = windows.len();
    with_ctx(|c| c.windows = windows);
    ok(format!("{count} open window(s), front app first:\n{}", lines.join("\n")), Some(json!({ "windows": list })))
}

fn window_by_id(args: &serde_json::Value) -> Result<Window, String> {
    let id = args.get("id").and_then(|v| v.as_u64()).ok_or("'id' is required (a window id from list_windows).")?;
    with_ctx(|c| c.windows.get(id as usize).cloned()).ok_or_else(|| format!("no window with id {id} — call list_windows again"))
}

pub(crate) async fn execute_dip_focus_window_tool(args: &serde_json::Value) -> ToolResult {
    if let Err(e) = require_trust() {
        return err(e);
    }
    let w = match window_by_id(args) {
        Ok(w) => w,
        Err(e) => return err(e),
    };
    let w2 = w.clone();
    match blocking(move || mac::focus(&w2)).await {
        Ok(true) => {
            mac::set_target(w.pid, &w.title);
            ok(format!("'{}' ({}) is in front and is now the target for typing and keys.", w.title, w.app), None)
        }
        Ok(false) => err(format!("couldn't bring '{}' to the front", w.title)),
        Err(e) => err(e),
    }
}

pub(crate) async fn execute_dip_window_action_tool(args: &serde_json::Value) -> ToolResult {
    if let Err(e) = require_trust() {
        return err(e);
    }
    let Some(state) = args.get("window_state").and_then(|v| v.as_str()).map(str::to_string) else {
        return err("'window_state' is required (minimize, maximize, restore, or close).");
    };
    let w = match window_by_id(args) {
        Ok(w) => w,
        Err(e) => return err(e),
    };
    let title = w.title.clone();
    let result = blocking(move || -> Result<String, String> {
        let el = &w.element;
        let ok = match state.as_str() {
            "minimize" => el.set_bool("AXMinimized", true),
            "restore" => el.set_bool("AXMinimized", false) | mac::focus(&w),
            // The green button's zoom (fills the screen without a new Space).
            "maximize" => el.element("AXZoomButton").is_some_and(|b| b.perform("AXPress")),
            "close" => el.element("AXCloseButton").is_some_and(|b| b.perform("AXPress")),
            other => return Err(format!("unknown window_state '{other}' — minimize, maximize, restore, or close")),
        };
        if !ok {
            return Err(format!("couldn't {state} '{}'", w.title));
        }
        Ok(state)
    })
    .await;
    match result {
        Ok(Ok(s)) if s == "close" => ok(format!("Asked '{title}' to close (it may show a save prompt — snapshot to check)."), None),
        Ok(Ok(s)) => ok(format!("{}d '{title}'.", s), None),
        Ok(Err(e)) | Err(e) => err(e),
    }
}

/// `launch_app`: open an app by name (or a file, folder or URL), wait for its
/// window, and make it the target.
pub(crate) async fn execute_dip_launch_app_tool(args: &serde_json::Value) -> ToolResult {
    let Some(command) = args.get("command").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string) else {
        return err("'command' is required — an app name (Safari, TextEdit), a path, a document, a folder, or a URL.");
    };
    let extra: Vec<String> = args.get("args").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let result = blocking(move || -> Result<String, String> {
        let before: std::collections::HashSet<i32> = mac::running_apps().into_iter().map(|a| a.pid).collect();
        let mut cmd = std::process::Command::new("open");
        let looks_like_path = command.contains('/') || command.contains("://") || command.ends_with(".app");
        if looks_like_path {
            cmd.arg(&command);
        } else {
            cmd.args(["-a", &command]);
        }
        if !extra.is_empty() {
            cmd.arg("--args").args(&extra);
        }
        let out = cmd.output().map_err(|e| format!("couldn't run open: {e}"))?;
        if !out.status.success() {
            return Err(format!("couldn't open '{command}': {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let wanted = command.trim_end_matches(".app").rsplit('/').next().unwrap_or(&command).to_lowercase();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
            let windows = mac::windows();
            let fresh = windows
                .iter()
                .find(|w| w.app.to_lowercase() == wanted)
                .or_else(|| windows.iter().find(|w| !before.contains(&w.pid)));
            if let Some(w) = fresh {
                mac::set_target(w.pid, &w.title);
                mac::focus(w);
                return Ok(format!("Opened '{command}'. Its window '{}' is in front and is the target now.", w.title));
            }
        }
        Ok(format!("Opened '{command}', but no window appeared within 10s — it may still be loading (call list_windows)."))
    })
    .await;
    match result {
        Ok(Ok(m)) => ok(m, None),
        Ok(Err(e)) | Err(e) => err(e),
    }
}
