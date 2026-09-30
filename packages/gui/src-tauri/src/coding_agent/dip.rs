//! DIP (Direct Interaction Protocol) — the structured, Windows half of `dip`:
//! reading an app's real UI instead of guessing pixels, and acting on it.
//!
//! ## What makes it fast
//!
//! Everything UI Automation does is a cross-process COM call into the target
//! app, and the old implementation paid for that three times over: a fresh
//! `UIAutomation` per call, a tree walk followed by ~6 more calls *per
//! element* to read its name, type, rect and state, and a 220 ms pause before
//! every click so the cursor could be watched. A 200-element window took
//! seconds.
//!
//! Now one worker thread owns a single `UIAutomation` for the life of the
//! process (`worker`), and a snapshot is one `CacheRequest`: every property
//! of every element in the window, fetched in a single round trip. Actions
//! use the live elements that snapshot returned — they stay on the worker, so
//! COM's thread affinity is never an issue — instead of re-finding them by
//! screen position.
//!
//! ## What makes it work
//!
//! - Acting prefers the element's own patterns (Value to *replace* a field's
//!   text, Toggle, SelectionItem, ExpandCollapse) and falls back to a real
//!   click at the element's clickable point, in the target window brought to
//!   the front first — never into Vivianne.
//! - The window the agent is working in is remembered (`win::target`), so a
//!   snapshot without an id reads that window, not whichever one has focus.
//! - A worker stuck on an unresponsive app is abandoned after a timeout and a
//!   fresh one takes over, so one hung window cannot wedge `dip` for good.
//!
//! Window listing, focusing, state changes and launching go through Win32
//! directly (`super::win`) — faster than UI Automation, and a launched app's
//! window is found and made the target automatically.

use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::json;
use uiautomation::patterns::{
    UIExpandCollapsePattern, UIInvokePattern, UIScrollItemPattern, UISelectionItemPattern, UITextPattern, UITogglePattern,
    UIValuePattern,
};
use uiautomation::types::{ControlType, ElementMode, ExpandCollapseState, Handle, TreeScope, UIProperty};
use uiautomation::{UIAutomation, UIElement};

use super::tool_exec::ToolResult;
use super::win;

fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult { content: format!("Error: {}", msg.into()), is_error: true, details: None }
}

fn ok(text: impl Into<String>, details: Option<serde_json::Value>) -> ToolResult {
    ToolResult { content: text.into(), is_error: false, details }
}

// ── The UI Automation worker ────────────────────────────────────────────────

/// One element from the last snapshot: the live handle to act on, and what
/// the agent was told about it.
struct Found {
    element: UIElement,
    role: ControlType,
    name: String,
    center: (i32, i32),
}

/// State that lives on the worker thread only.
struct Ctx {
    automation: UIAutomation,
    /// The last snapshot's elements, by the ids the agent was given.
    elements: Vec<Found>,
    /// The window they belong to.
    window: isize,
}

type Job = Box<dyn FnOnce(&mut Ctx) + Send>;

static WORKER: Mutex<Option<mpsc::Sender<Job>>> = Mutex::new(None);

fn spawn_worker() -> Result<mpsc::Sender<Job>, String> {
    let (tx, rx) = mpsc::channel::<Job>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("dip-uia".into())
        .spawn(move || {
            let automation = match UIAutomation::new() {
                Ok(a) => a,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("UI Automation is unavailable: {e}")));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            let mut ctx = Ctx { automation, elements: Vec::new(), window: 0 };
            while let Ok(job) = rx.recv() {
                job(&mut ctx);
            }
        })
        .map_err(|e| format!("couldn't start the UI Automation thread: {e}"))?;
    ready_rx.recv().map_err(|_| "UI Automation thread died on start".to_string())??;
    Ok(tx)
}

/// How long a UI Automation job may take before the app it talks to is
/// considered hung. Generous: a large tree in a slow app is legitimately
/// slow, and giving up throws the worker (and its snapshot) away.
const JOB_TIMEOUT: Duration = Duration::from_secs(20);

/// Run `job` on the UI Automation worker and wait for its answer.
async fn on_worker<R: Send + 'static>(job: impl FnOnce(&mut Ctx) -> R + Send + 'static) -> Result<R, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let sender = {
        let mut guard = WORKER.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(spawn_worker()?);
        }
        guard.clone().expect("just set")
    };
    let boxed: Job = Box::new(move |ctx| {
        let _ = tx.send(job(ctx));
    });
    if sender.send(boxed).is_err() {
        // The thread is gone; start over next time.
        *WORKER.lock().unwrap_or_else(|e| e.into_inner()) = None;
        return Err("the UI Automation thread stopped — try again".into());
    }
    match tokio::time::timeout(JOB_TIMEOUT, rx).await {
        Ok(Ok(r)) => Ok(r),
        Ok(Err(_)) => {
            *WORKER.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Err("the UI Automation call failed inside the app — try again".into())
        }
        Err(_) => {
            // The worker is stuck inside a hung app. Leave it there and
            // start fresh on the next call.
            *WORKER.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Err("the app stopped responding to UI Automation (it may be busy or hung). Try a screenshot, or wait and snapshot again.".into())
        }
    }
}

// ── Snapshot ───────────────────────────────────────────────────────────────

/// Most elements a snapshot lists. Past this the list costs more of the
/// model's context than it saves in clicks; `find` narrows instead.
const MAX_LISTED: usize = 300;
/// How deep the tree is walked.
const MAX_DEPTH: usize = 30;

fn is_interactive(t: ControlType) -> bool {
    matches!(
        t,
        ControlType::Button
            | ControlType::CheckBox
            | ControlType::ComboBox
            | ControlType::Edit
            | ControlType::Hyperlink
            | ControlType::ListItem
            | ControlType::MenuItem
            | ControlType::RadioButton
            | ControlType::Slider
            | ControlType::Spinner
            | ControlType::SplitButton
            | ControlType::TabItem
            | ControlType::TreeItem
            | ControlType::DataItem
            | ControlType::Document
            | ControlType::MenuBar
            | ControlType::Menu
    )
}

/// Worth listing: something you can act on, or something with words on it.
/// Unnamed layout containers are skipped, their children kept.
fn is_listed(t: ControlType, name: &str, value: &str, focused: bool) -> bool {
    is_interactive(t) || !name.trim().is_empty() || !value.trim().is_empty() || focused
}

fn role_name(t: ControlType) -> String {
    format!("{t:?}")
}

fn clip(s: &str, max: usize) -> String {
    let s = s.replace(['\r', '\n'], " ");
    if s.chars().count() <= max {
        return s;
    }
    format!("{}…", s.chars().take(max).collect::<String>())
}

/// One snapshot line: `[12] Edit "File name" = "report.txt" *focused`.
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
    rect: (i32, i32, i32, i32),
}

/// Read a window's whole UI in one round trip and remember its elements.
fn snapshot(ctx: &mut Ctx, window: isize) -> Result<Snapshot, String> {
    let a = &ctx.automation;
    let cache = a.create_cache_request().map_err(|e| e.to_string())?;
    for p in [
        UIProperty::Name,
        UIProperty::ControlType,
        UIProperty::BoundingRectangle,
        UIProperty::IsEnabled,
        UIProperty::HasKeyboardFocus,
        UIProperty::IsOffscreen,
        UIProperty::ValueValue,
        UIProperty::IsPassword,
    ] {
        cache.add_property(p).map_err(|e| e.to_string())?;
    }
    cache.set_tree_scope(TreeScope::Subtree).map_err(|e| e.to_string())?;
    cache.set_tree_filter(a.get_control_view_condition().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    cache.set_element_mode(ElementMode::Full).map_err(|e| e.to_string())?;

    let root = a
        .element_from_handle_build_cache(Handle::from(window), &cache)
        .map_err(|e| format!("couldn't read that window ({e}) — it may have closed; call list_windows again"))?;

    let mut elements: Vec<Found> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut truncated = false;
    // Iterative depth-first walk over the cached tree; `depth` is the
    // indentation of listed ancestors, so skipped containers don't indent.
    let mut stack: Vec<(UIElement, usize, usize)> = vec![(root.clone(), 0, 0)];
    let mut first = true;
    while let Some((el, depth, walk_depth)) = stack.pop() {
        let role = el.get_cached_control_type().unwrap_or(ControlType::Custom);
        let name = el.get_cached_name().unwrap_or_default();
        let offscreen = el
            .get_cached_property_value(UIProperty::IsOffscreen)
            .ok()
            .and_then(|v| v.try_into().ok())
            .unwrap_or(false);
        let password = el
            .get_cached_property_value(UIProperty::IsPassword)
            .ok()
            .and_then(|v| v.try_into().ok())
            .unwrap_or(false);
        let value: String = if password {
            String::new()
        } else {
            el.get_cached_property_value(UIProperty::ValueValue).ok().and_then(|v| (&v).try_into().ok()).unwrap_or_default()
        };
        let enabled = el.get_cached_property_value(UIProperty::IsEnabled).ok().and_then(|v| v.try_into().ok()).unwrap_or(true);
        let focused = el
            .get_cached_property_value(UIProperty::HasKeyboardFocus)
            .ok()
            .and_then(|v| v.try_into().ok())
            .unwrap_or(false);
        let r = el.get_cached_bounding_rectangle().ok();
        let rect = r.map(|r| (r.get_left(), r.get_top(), r.get_width(), r.get_height())).unwrap_or_default();

        let mut child_depth = depth;
        // The window itself is the header, not a line.
        if !first && !offscreen && rect.2 > 0 && rect.3 > 0 && is_listed(role, &name, &value, focused) {
            if elements.len() >= MAX_LISTED {
                truncated = true;
            } else {
                let id = elements.len();
                lines.push(line(id, depth, &role_name(role), &name, &value, enabled, focused));
                elements.push(Found {
                    element: el.clone(),
                    role,
                    name: name.clone(),
                    center: (rect.0 + rect.2 / 2, rect.1 + rect.3 / 2),
                });
                child_depth = depth + 1;
            }
        }
        first = false;
        if walk_depth < MAX_DEPTH {
            if let Ok(children) = el.get_cached_children() {
                // Reversed so the first child is walked first.
                for c in children.into_iter().rev() {
                    stack.push((c, child_depth, walk_depth + 1));
                }
            }
        }
    }

    ctx.elements = elements;
    ctx.window = window;
    Ok(Snapshot { title: win::title(window), lines, truncated, rect: win::rect(window).unwrap_or_default() })
}

/// Which window a snapshot reads: the one named, else the agent's target,
/// else the app in front — but never Vivianne's own window by accident.
fn resolve_window(args: &serde_json::Value) -> Result<isize, String> {
    if let Some(id) = args.get("id").and_then(|v| v.as_u64()) {
        return window_by_id(id as usize);
    }
    if let Some(t) = win::target() {
        return Ok(t);
    }
    let fg = win::foreground();
    if fg != 0 && !win::is_own(fg) {
        return Ok(fg);
    }
    Err("no target window yet — call list_windows and pass the window's id (or launch_app / focus_window first)".into())
}

pub(crate) async fn execute_dip_snapshot_tool(app: &tauri::AppHandle, args: &serde_json::Value) -> ToolResult {
    let window = match resolve_window(args) {
        Ok(w) => w,
        Err(e) => return err(e),
    };
    win::set_target(window);
    // Front first: acting later means clicking its elements, and a window
    // behind others can't be clicked.
    let _ = tokio::task::spawn_blocking(move || win::force_foreground(window)).await;
    let started = Instant::now();
    let snap = match on_worker(move |ctx| snapshot(ctx, window)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) | Err(e) => return err(e),
    };
    let ms = started.elapsed().as_millis();
    if args.get("highlight").and_then(|v| v.as_bool()).unwrap_or(false) {
        crate::instant::pointer::highlight(app, snap.rect.0 as f64, snap.rect.1 as f64, snap.rect.2 as f64, snap.rect.3 as f64, Some(snap.title.clone())).ok();
    }
    if snap.lines.is_empty() {
        return ok(
            format!("'{}' exposes no UI elements (it may draw its own UI, like a game or canvas) — use screenshot and pixel actions instead.", snap.title),
            None,
        );
    }
    let mut text = format!(
        "'{}' — {} element(s), read in {ms} ms. Act on one with interact{{id, element_action}}; ids last until the next snapshot or find.\n",
        snap.title,
        snap.lines.len()
    );
    text.push_str(&snap.lines.join("\n"));
    if snap.truncated {
        text.push_str(&format!("\n… more elements not listed (over {MAX_LISTED}). Use find{{query}} to locate a specific one."));
    }
    ok(text, Some(json!({ "window": snap.title, "count": snap.lines.len(), "ms": ms })))
}

/// `find`: a snapshot filtered to what matches `query` (name, value or
/// type) — the fast way to one element in a big window.
pub(crate) async fn execute_dip_find_tool(args: &serde_json::Value) -> ToolResult {
    let Some(query) = args.get("query").or_else(|| args.get("text")).and_then(|v| v.as_str()).map(|q| q.trim().to_lowercase()).filter(|q| !q.is_empty())
    else {
        return err("'query' is required — part of the element's name, text, or type");
    };
    let window = match resolve_window(args) {
        Ok(w) => w,
        Err(e) => return err(e),
    };
    win::set_target(window);
    let snap = match on_worker(move |ctx| snapshot(ctx, window)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) | Err(e) => return err(e),
    };
    let hits: Vec<&String> = snap.lines.iter().filter(|l| l.to_lowercase().contains(&query)).collect();
    if hits.is_empty() {
        return ok(format!("Nothing in '{}' matches \"{query}\". snapshot lists everything.", snap.title), None);
    }
    let body: Vec<String> = hits.iter().map(|l| l.trim_start().to_string()).collect();
    ok(format!("{} match(es) in '{}':\n{}", body.len(), snap.title, body.join("\n")), None)
}

// ── Acting on an element ───────────────────────────────────────────────────

/// A real click at an element: its window in front, the cursor on the
/// element's clickable point (or centre), click. No pause for show.
fn physical_click(window: isize, at: (i32, i32), button: win::MouseButton, count: u32) -> Result<(), String> {
    if let Some(reason) = win::input_blocked_reason(window) {
        return Err(reason);
    }
    win::force_foreground(window)?;
    win::move_to(at.0, at.1)?;
    win::click(button, count)
}

fn clickable_point(f: &Found) -> (i32, i32) {
    f.element.get_clickable_point().ok().flatten().map(|p| (p.get_x(), p.get_y())).unwrap_or(f.center)
}

fn interact(ctx: &mut Ctx, id: usize, action: &str, value: Option<String>) -> Result<(String, (i32, i32)), String> {
    let window = ctx.window;
    let f = ctx.elements.get(id).ok_or_else(|| format!("no element [{id}] — call snapshot (or find) again"))?;
    // A stale element (the UI changed) fails here rather than acting on
    // whatever now sits where it was.
    f.element.get_bounding_rectangle().map_err(|_| format!("element [{id}] is gone — the window changed; snapshot again"))?;
    let label = if f.name.trim().is_empty() { format!("[{id}] {:?}", f.role) } else { format!("'{}'", clip(f.name.trim(), 60)) };
    let at = clickable_point(f);

    let done = match action {
        "click" => {
            physical_click(window, at, win::MouseButton::Left, 1)?;
            format!("Clicked {label}.")
        }
        "double_click" => {
            physical_click(window, at, win::MouseButton::Left, 2)?;
            format!("Double-clicked {label}.")
        }
        "right_click" => {
            physical_click(window, at, win::MouseButton::Right, 1)?;
            format!("Right-clicked {label}.")
        }
        "invoke" => {
            let p = f.element.get_pattern::<UIInvokePattern>().map_err(|_| format!("{label} can't be invoked — use click"))?;
            p.invoke().map_err(|e| e.to_string())?;
            format!("Invoked {label}.")
        }
        "focus" => {
            win::force_foreground(window)?;
            f.element.set_focus().map_err(|e| format!("couldn't focus {label}: {e}"))?;
            format!("Focused {label}.")
        }
        "set_value" => {
            let value = value.ok_or("'value' is required for set_value")?;
            // The Value pattern replaces the text outright. Checked by
            // reading it back: some controls accept the call and ignore it.
            let via_pattern = f
                .element
                .get_pattern::<UIValuePattern>()
                .ok()
                .filter(|p| !p.is_readonly().unwrap_or(true))
                .and_then(|p| p.set_value(&value).ok().map(|()| p))
                .map(|p| p.get_value().map(|v| v == value).unwrap_or(false))
                .unwrap_or(false);
            if !via_pattern {
                // Otherwise the way a person does it: focus, select all, type.
                if let Some(reason) = win::input_blocked_reason(window) {
                    return Err(reason);
                }
                win::force_foreground(window)?;
                if f.element.set_focus().is_err() {
                    win::move_to(at.0, at.1)?;
                    win::click(win::MouseButton::Left, 1)?;
                }
                let ctrl_a = win::parse_combo("ctrl+a")?;
                win::press(&ctrl_a, 1)?;
                win::type_text(&value)?;
            }
            format!("Set {label} to {:?}.", clip(&value, 80))
        }
        "toggle" => {
            let p = f.element.get_pattern::<UITogglePattern>().map_err(|_| format!("{label} isn't a toggle — use click"))?;
            p.toggle().map_err(|e| e.to_string())?;
            format!("Toggled {label} (now {:?}).", p.get_toggle_state().ok())
        }
        "select" => {
            let p = f.element.get_pattern::<UISelectionItemPattern>().map_err(|_| format!("{label} isn't selectable — use click"))?;
            p.select().map_err(|e| e.to_string())?;
            format!("Selected {label}.")
        }
        "expand" | "collapse" => {
            let p = f.element.get_pattern::<UIExpandCollapsePattern>().map_err(|_| format!("{label} doesn't expand — use click"))?;
            if action == "expand" { p.expand() } else { p.collapse() }.map_err(|e| e.to_string())?;
            let state = p.get_state().ok().map(|s| if s == ExpandCollapseState::Expanded { "expanded" } else { "collapsed" });
            format!("{} {label}{}.", if action == "expand" { "Expanded" } else { "Collapsed" }, state.map(|s| format!(" (now {s})")).unwrap_or_default())
        }
        "scroll_into_view" => {
            let p = f.element.get_pattern::<UIScrollItemPattern>().map_err(|_| format!("{label} can't be scrolled to"))?;
            p.scroll_into_view().map_err(|e| e.to_string())?;
            format!("Scrolled {label} into view.")
        }
        other => {
            return Err(format!(
                "unknown element_action '{other}' — use click, double_click, right_click, invoke, focus, set_value, toggle, select, expand, collapse, or scroll_into_view"
            ))
        }
    };
    Ok((done, at))
}

pub(crate) async fn execute_dip_interact_tool(app: &tauri::AppHandle, args: &serde_json::Value) -> ToolResult {
    let Some(id) = args.get("id").and_then(|v| v.as_u64()).map(|v| v as usize) else {
        return err("'id' is required (an element id from the last snapshot or find).");
    };
    let action = args.get("element_action").and_then(|v| v.as_str()).unwrap_or("click").to_string();
    let value = args.get("value").or_else(|| args.get("text")).and_then(|v| v.as_str()).map(str::to_string);
    let result = on_worker(move |ctx| {
        let window = ctx.window;
        interact(ctx, id, &action, value).map(|r| (r, window))
    })
    .await;
    match result {
        Ok(Ok(((msg, at), window))) => {
            win::set_target(window);
            // The live pointer: where the agent just acted, for whoever is
            // watching. Drawing only — it moves and clicks nothing.
            let _ = crate::instant::pointer::point(app, at.0 as f64, at.1 as f64, None);
            ok(msg, None)
        }
        Ok(Err(e)) | Err(e) => err(e),
    }
}

/// `read_text`: an element's full text — a document, a log, a field — or the
/// window's when no id is given.
pub(crate) async fn execute_dip_read_text_tool(args: &serde_json::Value) -> ToolResult {
    let id = args.get("id").and_then(|v| v.as_u64()).map(|v| v as usize);
    let result = on_worker(move |ctx| -> Result<String, String> {
        let element = match id {
            Some(i) => ctx.elements.get(i).map(|f| f.element.clone()).ok_or_else(|| format!("no element [{i}] — snapshot again"))?,
            None => {
                let w = if ctx.window != 0 { ctx.window } else { win::target().ok_or("no window read yet — snapshot first")? };
                ctx.automation.element_from_handle(Handle::from(w)).map_err(|e| e.to_string())?
            }
        };
        if let Ok(p) = element.get_pattern::<UITextPattern>() {
            if let Ok(t) = p.get_document_range().and_then(|r| r.get_text(200_000)) {
                if !t.trim().is_empty() {
                    return Ok(t);
                }
            }
        }
        if let Ok(p) = element.get_pattern::<UIValuePattern>() {
            if let Ok(v) = p.get_value() {
                if !v.trim().is_empty() {
                    return Ok(v);
                }
            }
        }
        let cond = ctx.automation.create_true_condition().map_err(|e| e.to_string())?;
        // A window has no text of its own: what it holds is in a document or
        // edit control inside it. Read the fullest one, so "what's in the
        // Notepad window" is the document, not the menu labels around it.
        if id.is_none() {
            let best = element
                .find_all(TreeScope::Descendants, &cond)
                .unwrap_or_default()
                .iter()
                .filter_map(|e| {
                    if let Ok(p) = e.get_pattern::<UITextPattern>() {
                        if let Ok(t) = p.get_document_range().and_then(|r| r.get_text(200_000)) {
                            return Some(t);
                        }
                    }
                    e.get_pattern::<UIValuePattern>().and_then(|p| p.get_value()).ok()
                })
                .filter(|t| !t.trim().is_empty())
                .max_by_key(|t| t.len());
            if let Some(t) = best {
                return Ok(t);
            }
        }
        // Last resort: every name under it, in order.
        let names: Vec<String> = element
            .find_all(TreeScope::Subtree, &cond)
            .unwrap_or_default()
            .iter()
            .filter_map(|e| e.get_name().ok())
            .filter(|n| !n.trim().is_empty())
            .collect();
        Ok(names.join("\n"))
    })
    .await;
    match result {
        Ok(Ok(t)) if t.trim().is_empty() => ok("It has no readable text — try a screenshot.", None),
        Ok(Ok(t)) => {
            const MAX: usize = 60_000;
            let shown = if t.len() > MAX {
                let mut end = MAX;
                while !t.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}\n… ({} more characters)", &t[..end], t.len() - MAX)
            } else {
                t
            };
            ok(shown, None)
        }
        Ok(Err(e)) | Err(e) => err(e),
    }
}

// ── Windows ─────────────────────────────────────────────────────────────────

static LAST_WINDOWS: Mutex<Vec<isize>> = Mutex::new(Vec::new());

fn window_by_id(id: usize) -> Result<isize, String> {
    LAST_WINDOWS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .copied()
        .ok_or_else(|| format!("no window with id {id} — call list_windows again"))
}

pub(crate) async fn execute_dip_list_windows_tool() -> ToolResult {
    let windows = match tokio::task::spawn_blocking(win::list_windows).await {
        Ok(w) => w,
        Err(e) => return err(format!("list_windows failed: {e}")),
    };
    if windows.is_empty() {
        return ok("No open windows found.", None);
    }
    let target = win::target();
    let lines: Vec<String> = windows
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let mut l = format!("[{i}] {} — {}", clip(&w.title, 90), if w.process.is_empty() { "?" } else { &w.process });
            if w.minimized {
                l.push_str(" (minimized)");
            }
            if win::is_own(w.hwnd) {
                l.push_str(" (Vivianne — not a target)");
            }
            if Some(w.hwnd) == target {
                l.push_str(" *target");
            }
            l
        })
        .collect();
    let list: Vec<serde_json::Value> =
        windows.iter().enumerate().map(|(i, w)| json!({ "id": i, "title": w.title, "process": w.process })).collect();
    *LAST_WINDOWS.lock().unwrap_or_else(|e| e.into_inner()) = windows.iter().map(|w| w.hwnd).collect();
    ok(format!("{} open window(s), front to back:\n{}", windows.len(), lines.join("\n")), Some(json!({ "windows": list })))
}

pub(crate) async fn execute_dip_focus_window_tool(args: &serde_json::Value) -> ToolResult {
    let Some(id) = args.get("id").and_then(|v| v.as_u64()) else {
        return err("'id' is required (a window id from list_windows).");
    };
    let h = match window_by_id(id as usize) {
        Ok(h) => h,
        Err(e) => return err(e),
    };
    if win::is_own(h) {
        return err("that is Vivianne's own window — pick the app you want to control");
    }
    match tokio::task::spawn_blocking(move || win::force_foreground(h)).await {
        Ok(Ok(())) => {
            win::set_target(h);
            let warning = win::input_blocked_reason(h).map(|r| format!(" Note: {r}")).unwrap_or_default();
            ok(format!("'{}' is in front and is now the target for typing and keys.{warning}", win::title(h)), None)
        }
        Ok(Err(e)) => err(format!("couldn't bring '{}' to the front: {e}", win::title(h))),
        Err(e) => err(e.to_string()),
    }
}

pub(crate) async fn execute_dip_window_action_tool(args: &serde_json::Value) -> ToolResult {
    let Some(id) = args.get("id").and_then(|v| v.as_u64()) else {
        return err("'id' is required (a window id from list_windows).");
    };
    let Some(state) = args.get("window_state").and_then(|v| v.as_str()) else {
        return err("'window_state' is required (minimize, maximize, restore, or close).");
    };
    let h = match window_by_id(id as usize) {
        Ok(h) => h,
        Err(e) => return err(e),
    };
    let title = win::title(h);
    match win::show(h, state) {
        Ok(()) => ok(
            match state {
                "close" => format!("Asked '{title}' to close (it may show a save prompt — snapshot to check)."),
                s => format!("{}d '{title}'.", s),
            },
            None,
        ),
        Err(e) => err(e),
    }
}

/// `launch_app`: start it, wait for its window, and make that the target —
/// so the very next call can type into it.
pub(crate) async fn execute_dip_launch_app_tool(args: &serde_json::Value) -> ToolResult {
    let Some(command) = args.get("command").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string) else {
        return err("'command' is required — an app name (notepad, calc), a path, a document, a folder, or a URI (ms-settings:, https://…).");
    };
    let extra: Vec<String> = args
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let params = extra.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ");

    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let before: std::collections::HashSet<isize> = win::list_windows().into_iter().map(|w| w.hwnd).collect();
        let pid = win::launch(&command, &params)?;
        // The new window: the launched process's, or — for launchers that
        // hand off and exit (Store apps, Explorer, URIs) — any window that
        // wasn't there before.
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(150));
            let now = win::list_windows();
            let fresh = now
                .iter()
                .find(|w| Some(w.pid) == pid)
                .or_else(|| now.iter().find(|w| !before.contains(&w.hwnd) && !win::is_own(w.hwnd)));
            if let Some(w) = fresh {
                win::set_target(w.hwnd);
                let _ = win::force_foreground(w.hwnd);
                return Ok(format!("Started '{command}'. Its window '{}' is open, in front, and is the target now.", w.title));
            }
        }
        Ok(format!("Started '{command}', but no new window appeared within 10s — it may still be loading (call list_windows), or it opened in an existing window."))
    })
    .await;
    match result {
        Ok(Ok(msg)) => ok(msg, None),
        Ok(Err(e)) => err(e),
        Err(e) => err(e.to_string()),
    }
}

/// Live checks against a real app. They drive the desktop (open Notepad,
/// type, close it), so they are ignored by default:
/// `cargo test --lib dip::live -- --ignored --test-threads=1`
#[cfg(test)]
mod live {
    use super::*;

    fn wait_for_window(pid: Option<u32>, before: &std::collections::HashSet<isize>) -> isize {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(150));
            let now = win::list_windows();
            if let Some(w) = now.iter().find(|w| Some(w.pid) == pid).or_else(|| now.iter().find(|w| !before.contains(&w.hwnd) && w.process.to_lowercase().contains("notepad"))) {
                return w.hwnd;
            }
        }
        panic!("Notepad's window never appeared");
    }

    /// The text of the first document/edit element in the last snapshot.
    async fn notepad_text() -> String {
        on_worker(|ctx| {
            let editable = |f: &&Found| matches!(f.role, ControlType::Document | ControlType::Edit);
            let f = ctx
                .elements
                .iter()
                .filter(editable)
                .find(|f| f.element.has_keyboard_focus().unwrap_or(false))
                .or_else(|| ctx.elements.iter().find(editable))
                .expect("an editable element");
            if let Ok(p) = f.element.get_pattern::<UITextPattern>() {
                if let Ok(t) = p.get_document_range().and_then(|r| r.get_text(-1)) {
                    return t;
                }
            }
            f.element.get_pattern::<UIValuePattern>().and_then(|p| p.get_value()).unwrap_or_default()
        })
        .await
        .unwrap()
    }

    /// Typing into a plain Win32 text box: isolates input from any one
    /// app's quirks. Needs the `DIP Test Box` window (scratchpad dipbox.ps1).
    #[tokio::test]
    #[ignore]
    async fn typing_into_a_plain_text_box() {
        let w = win::list_windows().into_iter().find(|w| w.title == "DIP Test Box").expect("open DIP Test Box first");
        win::set_target(w.hwnd);
        win::force_foreground(w.hwnd).expect("foreground");
        std::thread::sleep(Duration::from_millis(300));
        let sample = "héllo 👋
line two — ünïcode ✓";
        let t0 = Instant::now();
        win::type_text(sample).expect("type");
        println!("typed {} chars in {} ms", sample.chars().count(), t0.elapsed().as_millis());
        std::thread::sleep(Duration::from_millis(300));
        let h = w.hwnd;
        let _ = on_worker(move |ctx| snapshot(ctx, h)).await.unwrap().expect("snapshot");
        let text = on_worker(|ctx| {
            let f = ctx.elements.iter().find(|f| f.role == ControlType::Edit).expect("the text box");
            f.element.get_pattern::<UIValuePattern>().and_then(|p| p.get_value()).unwrap_or_default()
        })
        .await
        .unwrap();
        println!("box holds: {text:?}");
        assert_eq!(text.replace("
", "
"), sample);
    }

    #[tokio::test]
    #[ignore]
    async fn notepad_end_to_end() {
        let before: std::collections::HashSet<isize> = win::list_windows().into_iter().map(|w| w.hwnd).collect();
        let pid = win::launch("notepad.exe", "").expect("launch");
        let h = wait_for_window(pid, &before);
        win::set_target(h);
        win::force_foreground(h).expect("foreground");
        std::thread::sleep(Duration::from_millis(600));
        // Notepad restores last session's tabs; type into a fresh one.
        win::press(&win::parse_combo("ctrl+n").unwrap(), 1).unwrap();
        std::thread::sleep(Duration::from_millis(600));

        // Typing: Unicode, emoji, a real Enter.
        let t0 = Instant::now();
        win::type_text("héllo 👋\nline two").expect("type");
        let typed_ms = t0.elapsed().as_millis();
        std::thread::sleep(Duration::from_millis(300));

        // Snapshot speed, and reading the text back.
        let t1 = Instant::now();
        let snap = on_worker(move |ctx| snapshot(ctx, h)).await.unwrap().expect("snapshot");
        let snap_ms = t1.elapsed().as_millis();
        println!("typed in {typed_ms} ms; snapshot of '{}' ({} elements) in {snap_ms} ms", snap.title, snap.lines.len());
        println!("{}", snap.lines.iter().take(15).cloned().collect::<Vec<_>>().join("\n"));
        let text = notepad_text().await;
        println!("text now: {text:?}");
        assert!(text.contains("héllo 👋"), "typed text missing: {text:?}");
        assert!(text.contains("line two"), "second line missing: {text:?}");
        assert!(snap_ms < 3000, "snapshot took {snap_ms} ms");

        // set_value replaces rather than appends.
        let doc = on_worker(|ctx| {
            ctx.elements
                .iter()
                .position(|f| matches!(f.role, ControlType::Document | ControlType::Edit) && f.element.has_keyboard_focus().unwrap_or(false))
        })
        .await
        .unwrap()
        .expect("the focused document");
        on_worker(move |ctx| interact(ctx, doc, "set_value", Some("replaced".into()))).await.unwrap().expect("set_value");
        std::thread::sleep(Duration::from_millis(300));
        let _ = on_worker(move |ctx| snapshot(ctx, h)).await.unwrap();
        let text = notepad_text().await;
        println!("after set_value: {text:?}");
        assert_eq!(text.trim(), "replaced");

        // Keys: select all + delete leaves it empty (and nothing to save).
        win::force_foreground(h).unwrap();
        win::press(&win::parse_combo("ctrl+a").unwrap(), 1).unwrap();
        win::press(&win::parse_combo("delete").unwrap(), 1).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let _ = on_worker(move |ctx| snapshot(ctx, h)).await.unwrap();
        assert_eq!(notepad_text().await.trim(), "");

        // Close the (now empty) tab, then the window.
        win::press(&win::parse_combo("ctrl+w").unwrap(), 1).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let _ = win::show(h, "close");
    }
}
