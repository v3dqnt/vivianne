//! The `dip` tool — one surface for controlling the machine.
//!
//! - **Pixel mode** (this file): screenshots, the mouse, the keyboard. Works
//!   on anything, including games and canvas UIs.
//! - **Structured mode** (`super::dip`, Windows): the app's real UI tree —
//!   read in one round trip, acted on by element id.
//!
//! ## Screenshots are scaled, and coordinates follow them
//!
//! A full-resolution capture of a 1440p or 4K screen is several megabytes
//! that the provider downscales anyway — after which every coordinate the
//! model reads off its (smaller) view misses on the real screen. So a
//! screenshot is downscaled here, to at most `MAX_EDGE` pixels on its long
//! side, as a JPEG, and remembered as the current **frame**: its origin on
//! the virtual desktop and its scale. Every coordinate the model passes
//! afterwards is in that image's pixels and is mapped back to the screen
//! here. What the model sees is exactly the space it clicks in.
//!
//! On Windows, input goes through `super::win` (`SendInput`): Unicode
//! typing, the full key map, and — the part that made typing "not work" —
//! making sure the agent's target window is in front, not Vivianne. Other
//! platforms keep `enigo`.

use base64::Engine;
use serde_json::json;
use std::sync::Mutex;

use super::tool_exec::ToolResult;

#[cfg(target_os = "windows")]
pub(crate) async fn execute_computer_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> ToolResult {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return err("'action' is required"),
    };
    match action {
        "snapshot" => super::dip::execute_dip_snapshot_tool(app, args).await,
        "find" => super::dip::execute_dip_find_tool(args).await,
        "read_text" => super::dip::execute_dip_read_text_tool(args).await,
        "interact" => super::dip::execute_dip_interact_tool(app, args).await,
        "list_windows" => super::dip::execute_dip_list_windows_tool().await,
        "focus_window" => super::dip::execute_dip_focus_window_tool(args).await,
        "window_action" => super::dip::execute_dip_window_action_tool(args).await,
        "launch_app" => super::dip::execute_dip_launch_app_tool(args).await,
        "point_at" | "highlight" | "clear_pointer" => execute_narration_action(action, args, app),
        "screenshot" => run_blocking(capture_screenshot_for(args.clone())).await,
        "zoom" => run_blocking(zoom_for(args.clone())).await,
        "wait" => wait(args).await,
        _ => {
            let args = args.clone();
            let app = app.clone();
            match tokio::task::spawn_blocking(move || windows_pointer_action(&args, &app)).await {
                Ok(r) => r,
                Err(e) => err(format!("input failed: {e}")),
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) async fn execute_computer_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> ToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or_default();
    match action {
        "point_at" | "highlight" | "clear_pointer" => execute_narration_action(action, args, app),
        "screenshot" => run_blocking(capture_screenshot_for(args.clone())).await,
        "zoom" => run_blocking(zoom_for(args.clone())).await,
        "wait" => wait(args).await,
        _ => enigo_pointer_action(args),
    }
}

async fn run_blocking(f: impl FnOnce() -> ToolResult + Send + 'static) -> ToolResult {
    tokio::task::spawn_blocking(f).await.unwrap_or_else(|e| err(format!("capture failed: {e}")))
}

/// `wait{duration}` — give an app time to load or animate, in seconds.
async fn wait(args: &serde_json::Value) -> ToolResult {
    let secs = args.get("duration").or_else(|| args.get("seconds")).and_then(|v| v.as_f64()).unwrap_or(1.0).clamp(0.05, 30.0);
    tokio::time::sleep(std::time::Duration::from_secs_f64(secs)).await;
    ok(format!("Waited {secs:.1}s."))
}

/// The narration cursor: point at something, ring a region, or clear it.
/// It only draws — it can never move the real mouse, click or take focus.
/// Its coordinates are in the current screenshot's space, like every other.
fn execute_narration_action(action: &str, args: &serde_json::Value, app: &tauri::AppHandle) -> ToolResult {
    let num = |k: &str| args.get(k).and_then(|v| v.as_f64());
    let label = args.get("text").and_then(|v| v.as_str()).map(str::to_string);
    let f = frame();

    let outcome = match action {
        "clear_pointer" => {
            crate::instant::pointer::hide(app);
            Ok("Pointer hidden.".to_string())
        }
        "point_at" => match (num("x"), num("y")) {
            (Some(x), Some(y)) => {
                let (sx, sy) = f.to_screen(x, y);
                crate::instant::pointer::point(app, sx as f64, sy as f64, label).map(|()| format!("Pointing at ({x:.0}, {y:.0})."))
            }
            _ => return err("'point_at' needs 'x' and 'y'."),
        },
        "highlight" => match (num("x"), num("y"), num("width"), num("height")) {
            (Some(x), Some(y), Some(w), Some(h)) => {
                let (sx, sy) = f.to_screen(x, y);
                crate::instant::pointer::highlight(app, sx as f64, sy as f64, w / f.scale, h / f.scale, label)
                    .map(|()| format!("Highlighting {w:.0}x{h:.0} at ({x:.0}, {y:.0})."))
            }
            _ => return err("'highlight' needs 'x', 'y', 'width' and 'height'."),
        },
        _ => return err(format!("unknown narration action '{action}'")),
    };
    match outcome {
        Ok(msg) => ok(msg),
        Err(e) => err(e),
    }
}

fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult { content: format!("Error: {}", msg.into()), is_error: true, details: None }
}

fn ok(text: impl Into<String>) -> ToolResult {
    ToolResult { content: text.into(), is_error: false, details: None }
}

// ── The coordinate frame ────────────────────────────────────────────────────

/// Longest side of a screenshot the model sees. Large enough to read UI text,
/// small enough that no provider downscales it again (which is what used to
/// make coordinates miss).
pub const MAX_EDGE: u32 = 1366;

/// How screenshot pixels map to the screen: `screen = origin + image / scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: (i32, i32),
    pub scale: f64,
}

impl Frame {
    /// No screenshot yet: coordinates are screen pixels.
    pub const IDENTITY: Frame = Frame { origin: (0, 0), scale: 1.0 };

    pub fn to_screen(&self, x: f64, y: f64) -> (i32, i32) {
        (self.origin.0 + (x / self.scale).round() as i32, self.origin.1 + (y / self.scale).round() as i32)
    }

    pub fn to_image(&self, sx: i32, sy: i32) -> (i32, i32) {
        (((sx - self.origin.0) as f64 * self.scale).round() as i32, ((sy - self.origin.1) as f64 * self.scale).round() as i32)
    }
}

/// The scale that fits `w`×`h` within `MAX_EDGE` (never enlarging).
pub fn fit_scale(w: u32, h: u32, max_edge: u32) -> f64 {
    let long = w.max(h).max(1) as f64;
    (max_edge as f64 / long).min(1.0)
}

static FRAME: Mutex<Frame> = Mutex::new(Frame::IDENTITY);

fn frame() -> Frame {
    *FRAME.lock().unwrap_or_else(|e| e.into_inner())
}

fn coordinate(args: &serde_json::Value, key: &str) -> Option<(i32, i32)> {
    let a = args.get(key)?.as_array()?;
    let x = a.first()?.as_f64()?;
    let y = a.get(1)?.as_f64()?;
    Some(frame().to_screen(x, y))
}

// ── Capturing ──────────────────────────────────────────────────────────────

/// The monitor to capture: the one asked for by index, else the one the
/// target window is on, else the one under the cursor.
fn pick_monitor(args: &serde_json::Value) -> Result<xcap::Monitor, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("could not enumerate monitors: {e}"))?;
    if monitors.is_empty() {
        return Err("no monitors found".into());
    }
    if let Some(i) = args.get("monitor").and_then(|v| v.as_u64()) {
        return monitors.get(i as usize).cloned().ok_or_else(|| format!("no monitor {i} — there are {}", monitors.len()));
    }
    #[cfg(target_os = "windows")]
    if let Some((x, y, w, h)) = super::win::target().and_then(super::win::rect) {
        if let Ok(m) = xcap::Monitor::from_point(x + w / 2, y + h / 2) {
            return Ok(m);
        }
    }
    #[cfg(target_os = "windows")]
    {
        let (cx, cy) = super::win::cursor();
        if let Ok(m) = xcap::Monitor::from_point(cx, cy) {
            return Ok(m);
        }
    }
    Ok(monitors.iter().find(|m| m.is_primary().unwrap_or(false)).cloned().unwrap_or_else(|| monitors[0].clone()))
}

/// Downscale (or upscale, for zoom) and encode as JPEG.
fn encode(image: image::RgbaImage, scale: f64) -> Result<(String, u32, u32), String> {
    let (w, h) = (((image.width() as f64) * scale).round().max(1.0) as u32, ((image.height() as f64) * scale).round().max(1.0) as u32);
    let resized =
        if (scale - 1.0).abs() < 1e-3 { image } else { image::imageops::resize(&image, w, h, image::imageops::FilterType::Triangle) };
    let rgb = image::DynamicImage::ImageRgba8(resized).to_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80)
        .encode_image(&rgb)
        .map_err(|e| format!("failed to encode screenshot: {e}"))?;
    Ok((base64::engine::general_purpose::STANDARD.encode(&jpeg), w, h))
}

fn image_result(text: String, b64: String, w: u32, h: u32) -> ToolResult {
    ToolResult {
        content: text,
        is_error: false,
        details: Some(json!({ "image_base64": b64, "media_type": "image/jpeg", "width": w, "height": h })),
    }
}

/// `screenshot`: the monitor (or, with `window: true` or a window `id`, just
/// the target window), scaled to fit, becoming the coordinate frame.
fn capture_screenshot_for(args: serde_json::Value) -> impl FnOnce() -> ToolResult + Send {
    move || {
        let monitor = match pick_monitor(&args) {
            Ok(m) => m,
            Err(e) => return err(e),
        };
        let (mx, my) = (monitor.x().unwrap_or(0), monitor.y().unwrap_or(0));
        let (mw, mh) = (monitor.width().unwrap_or(0), monitor.height().unwrap_or(0));

        // Region: the whole monitor, or the target window clipped to it.
        #[allow(unused_mut)]
        let mut region = (mx, my, mw, mh);
        #[allow(unused_mut)]
        let mut what = "the screen".to_string();
        #[cfg(target_os = "windows")]
        {
            let want_window = args.get("window").and_then(|v| v.as_bool()).unwrap_or(false);
            if want_window {
                if let Some(t) = super::win::target() {
                    if let Some((x, y, w, h)) = super::win::rect(t) {
                        let x0 = x.max(mx);
                        let y0 = y.max(my);
                        let x1 = (x + w).min(mx + mw as i32);
                        let y1 = (y + h).min(my + mh as i32);
                        if x1 > x0 && y1 > y0 {
                            region = (x0, y0, (x1 - x0) as u32, (y1 - y0) as u32);
                            what = format!("'{}'", super::win::title(t));
                        }
                    }
                }
            }
        }
        let image = if region == (mx, my, mw, mh) {
            monitor.capture_image()
        } else {
            monitor.capture_region((region.0 - mx) as u32, (region.1 - my) as u32, region.2, region.3)
        };
        let image = match image {
            Ok(i) => i,
            Err(e) => return err(format!("screen capture failed: {e}")),
        };
        let scale = fit_scale(image.width(), image.height(), MAX_EDGE);
        let (b64, w, h) = match encode(image, scale) {
            Ok(r) => r,
            Err(e) => return err(e),
        };
        let f = Frame { origin: (region.0, region.1), scale };
        *FRAME.lock().unwrap_or_else(|e| e.into_inner()) = f;
        image_result(
            format!(
                "Screenshot of {what} [{w}x{h}]. Coordinates you pass from now on are in this image's pixels — they're mapped to the screen for you."
            ),
            b64,
            w,
            h,
        )
    }
}

/// `zoom{region:[x0,y0,x1,y1]}`: a closer look at part of the current
/// screenshot, for reading small text. For looking only — coordinates stay
/// in the screenshot's space.
fn zoom_for(args: serde_json::Value) -> impl FnOnce() -> ToolResult + Send {
    move || {
        let Some(r) = args.get("region").and_then(|v| v.as_array()).filter(|a| a.len() == 4) else {
            return err("'region' is required: [x0, y0, x1, y1] in the current screenshot's pixels");
        };
        let n: Vec<f64> = r.iter().filter_map(|v| v.as_f64()).collect();
        if n.len() != 4 || n[2] <= n[0] || n[3] <= n[1] {
            return err("'region' must be [x0, y0, x1, y1] with x1 > x0 and y1 > y0");
        }
        let f = frame();
        let (sx0, sy0) = f.to_screen(n[0], n[1]);
        let (sx1, sy1) = f.to_screen(n[2], n[3]);
        let monitor = match xcap::Monitor::from_point(sx0, sy0) {
            Ok(m) => m,
            Err(e) => return err(format!("no monitor at that point: {e}")),
        };
        let (mx, my) = (monitor.x().unwrap_or(0), monitor.y().unwrap_or(0));
        let (mw, mh) = (monitor.width().unwrap_or(0) as i32, monitor.height().unwrap_or(0) as i32);
        let (x0, y0) = ((sx0 - mx).clamp(0, mw - 1), (sy0 - my).clamp(0, mh - 1));
        let (x1, y1) = ((sx1 - mx).clamp(x0 + 1, mw), (sy1 - my).clamp(y0 + 1, mh));
        let image = match monitor.capture_region(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32) {
            Ok(i) => i,
            Err(e) => return err(format!("zoom capture failed: {e}")),
        };
        // Up to 3x, to make small text legible, still within MAX_EDGE.
        let scale = (MAX_EDGE as f64 / image.width().max(image.height()) as f64).min(3.0);
        match encode(image, scale) {
            Ok((b64, w, h)) => image_result(
                format!("Zoomed view [{w}x{h}] — for reading only; coordinates stay in the last screenshot's space."),
                b64,
                w,
                h,
            ),
            Err(e) => err(e),
        }
    }
}

// ── Windows input ──────────────────────────────────────────────────────────

/// "Now in front: 'Untitled - Notepad'" — so the model knows where input
/// actually went.
#[cfg(target_os = "windows")]
fn front_note() -> String {
    let fg = super::win::foreground();
    if fg == 0 {
        return String::new();
    }
    format!(" In front: '{}'.", super::win::title(fg))
}

#[cfg(target_os = "windows")]
fn windows_pointer_action(args: &serde_json::Value, app: &tauri::AppHandle) -> ToolResult {
    use super::win::{self, MouseButton};
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or_default();
    let text = args.get("text").and_then(|v| v.as_str());
    let at = coordinate(args, "coordinate");

    // A click lands in whatever window is under it — which then becomes the
    // agent's target for typing, as it would for a person.
    let aim = |p: (i32, i32)| -> Result<(), String> {
        let w = win::window_at(p.0, p.1);
        if w != 0 && !win::is_own(w) {
            if let Some(reason) = win::input_blocked_reason(w) {
                return Err(reason);
            }
            win::set_target(w);
        }
        win::move_to(p.0, p.1)?;
        let _ = crate::instant::pointer::point(app, p.0 as f64, p.1 as f64, None);
        Ok(())
    };
    let keyboard_ready = || -> Result<(), String> {
        let t = win::ensure_target_foreground()?;
        match win::input_blocked_reason(t) {
            Some(r) => Err(r),
            None => Ok(()),
        }
    };
    let repeat = args.get("repeat").and_then(|v| v.as_u64()).unwrap_or(1).clamp(1, 100) as u32;

    let result: Result<String, String> = (|| match action {
        "mouse_move" => {
            let p = at.ok_or("'coordinate' is required for mouse_move")?;
            aim(p)?;
            Ok("Moved the mouse.".to_string())
        }
        "left_click" | "right_click" | "middle_click" | "double_click" | "triple_click" => {
            if let Some(p) = at {
                aim(p)?;
            }
            let (b, n) = match action {
                "right_click" => (MouseButton::Right, 1),
                "middle_click" => (MouseButton::Middle, 1),
                "double_click" => (MouseButton::Left, 2),
                "triple_click" => (MouseButton::Left, 3),
                _ => (MouseButton::Left, 1),
            };
            win::click(b, n)?;
            Ok(format!("{}.{}", action.replace('_', " "), front_note()))
        }
        "left_click_drag" => {
            let to = at.ok_or("'coordinate' (where to drag to) is required for left_click_drag")?;
            let from = coordinate(args, "start_coordinate").unwrap_or_else(win::cursor);
            aim(from)?;
            win::button(MouseButton::Left, true)?;
            // In steps: many apps only register a drag they see move.
            for i in 1..=12 {
                let x = from.0 + (to.0 - from.0) * i / 12;
                let y = from.1 + (to.1 - from.1) * i / 12;
                let _ = win::move_to(x, y);
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            let released = win::button(MouseButton::Left, false);
            released.map(|()| "Dragged.".to_string())
        }
        "scroll" => {
            if let Some(p) = at {
                aim(p)?;
            }
            let amount = args.get("amount").and_then(|v| v.as_i64()).unwrap_or(3).clamp(1, 50) as i32;
            let (v, h) = match args.get("direction").and_then(|v| v.as_str()).unwrap_or("down") {
                "up" => (amount, 0),
                "down" => (-amount, 0),
                "left" => (0, -amount),
                "right" => (0, amount),
                other => return Err(format!("unknown direction '{other}' — up, down, left or right")),
            };
            win::scroll(v, h)?;
            Ok(format!("Scrolled {amount} notch(es)."))
        }
        "type" => {
            let t = text.ok_or("'text' is required for type")?;
            keyboard_ready()?;
            let (n, pasted) = win::type_text(t)?;
            // Sent is not landed: an app can drop or mangle input. Say so,
            // so the model checks before telling the user it's done.
            Ok(format!(
                "{} {n} character(s).{} Before saying it's done, check it landed with read_text (no id reads the window's document).",
                if pasted { "Pasted" } else { "Typed" },
                front_note()
            ))
        }
        "key" => {
            let combo = text.ok_or("'text' is required for key — e.g. \"enter\", \"ctrl+s\", \"alt+f4\"")?;
            let keys = win::parse_combo(combo)?;
            keyboard_ready()?;
            win::press(&keys, repeat)?;
            Ok(format!("Pressed {combo}{}.{}", if repeat > 1 { format!(" ×{repeat}") } else { String::new() }, front_note()))
        }
        "hold_key" => {
            let combo = text.ok_or("'text' is required for hold_key")?;
            let secs = args.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.5).clamp(0.05, 10.0);
            let keys = win::parse_combo(combo)?;
            keyboard_ready()?;
            win::key_down(&keys)?;
            std::thread::sleep(std::time::Duration::from_secs_f64(secs));
            win::key_up(&keys)?;
            Ok(format!("Held {combo} for {secs:.2}s."))
        }
        "key_down" | "key_up" => {
            let combo = text.ok_or("'text' is required")?;
            let keys = win::parse_combo(combo)?;
            keyboard_ready()?;
            if action == "key_down" { win::key_down(&keys)? } else { win::key_up(&keys)? };
            Ok(format!("{} {combo}.", if action == "key_down" { "Pressed and holding" } else { "Released" }))
        }
        "cursor_position" => {
            let (sx, sy) = win::cursor();
            let (x, y) = frame().to_image(sx, sy);
            Ok(format!("The cursor is at ({x}, {y}) in screenshot coordinates."))
        }
        other => Err(format!("unknown action '{other}'")),
    })();
    match result {
        Ok(msg) => ok(msg),
        Err(e) => err(e),
    }
}

// ── Other platforms: enigo ─────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
fn enigo_pointer_action(args: &serde_json::Value) -> ToolResult {
    use enigo::{Axis, Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or_default();
    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => return err(format!("failed to access input devices: {e}")),
    };
    let at = coordinate(args, "coordinate");
    let text = args.get("text").and_then(|v| v.as_str());
    let mut go = |p: Option<(i32, i32)>| -> Result<(), String> {
        if let Some((x, y)) = p {
            enigo.move_mouse(x, y, Coordinate::Abs).map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    let r: Result<String, String> = (|| match action {
        "mouse_move" => {
            go(at)?;
            Ok("Moved the mouse.".into())
        }
        "left_click" | "right_click" | "middle_click" | "double_click" | "triple_click" => {
            go(at)?;
            let (b, n) = match action {
                "right_click" => (Button::Right, 1),
                "middle_click" => (Button::Middle, 1),
                "double_click" => (Button::Left, 2),
                "triple_click" => (Button::Left, 3),
                _ => (Button::Left, 1),
            };
            for _ in 0..n {
                enigo.button(b, Direction::Click).map_err(|e| e.to_string())?;
            }
            Ok(format!("{}.", action.replace('_', " ")))
        }
        "left_click_drag" => {
            let to = at.ok_or("'coordinate' is required for left_click_drag")?;
            enigo.button(Button::Left, Direction::Press).map_err(|e| e.to_string())?;
            enigo.move_mouse(to.0, to.1, Coordinate::Abs).map_err(|e| e.to_string())?;
            enigo.button(Button::Left, Direction::Release).map_err(|e| e.to_string())?;
            Ok("Dragged.".into())
        }
        "scroll" => {
            go(at)?;
            let amount = args.get("amount").and_then(|v| v.as_i64()).unwrap_or(3) as i32;
            let (len, axis) = match args.get("direction").and_then(|v| v.as_str()).unwrap_or("down") {
                "up" => (-amount, Axis::Vertical),
                "left" => (-amount, Axis::Horizontal),
                "right" => (amount, Axis::Horizontal),
                _ => (amount, Axis::Vertical),
            };
            enigo.scroll(len, axis).map_err(|e| e.to_string())?;
            Ok("Scrolled.".into())
        }
        "type" => {
            enigo.text(text.ok_or("'text' is required")?).map_err(|e| e.to_string())?;
            Ok("Typed.".into())
        }
        "key" => {
            press_enigo_combo(&mut enigo, text.ok_or("'text' is required")?)?;
            Ok("Pressed.".into())
        }
        "cursor_position" => {
            let (sx, sy) = enigo.location().map_err(|e| e.to_string())?;
            let (x, y) = frame().to_image(sx, sy);
            Ok(format!("The cursor is at ({x}, {y}) in screenshot coordinates."))
        }
        other => Err(format!("unknown action '{other}'")),
    })();
    match r {
        Ok(m) => ok(m),
        Err(e) => err(e),
    }
}

#[cfg(not(target_os = "windows"))]
fn press_enigo_combo(enigo: &mut enigo::Enigo, combo: &str) -> Result<(), String> {
    use enigo::{Direction, Key, Keyboard};
    let key = |t: &str| -> Option<Key> {
        Some(match t.to_lowercase().as_str() {
            "ctrl" | "control" => Key::Control,
            "alt" | "option" => Key::Alt,
            "shift" => Key::Shift,
            "meta" | "cmd" | "command" | "super" | "win" => Key::Meta,
            "return" | "enter" => Key::Return,
            "tab" => Key::Tab,
            "escape" | "esc" => Key::Escape,
            "backspace" => Key::Backspace,
            "delete" | "del" => Key::Delete,
            "space" => Key::Space,
            "up" => Key::UpArrow,
            "down" => Key::DownArrow,
            "left" => Key::LeftArrow,
            "right" => Key::RightArrow,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" | "pgup" => Key::PageUp,
            "pagedown" | "pgdn" => Key::PageDown,
            _ => {
                let mut c = t.chars();
                let ch = c.next()?;
                if c.next().is_some() {
                    return None;
                }
                Key::Unicode(ch)
            }
        })
    };
    let keys: Vec<Key> = combo.split('+').map(str::trim).map(|t| key(t).ok_or(format!("unknown key '{t}'"))).collect::<Result<_, _>>()?;
    let (last, mods) = keys.split_last().ok_or("empty key combination")?;
    for m in mods {
        enigo.key(*m, Direction::Press).map_err(|e| e.to_string())?;
    }
    let r = enigo.key(*last, Direction::Click).map_err(|e| e.to_string());
    for m in mods.iter().rev() {
        let _ = enigo.key(*m, Direction::Release);
    }
    r
}

#[tauri::command]
pub fn cmd_capture_screen() -> Result<serde_json::Value, String> {
    use xcap::Monitor;
    let monitors = Monitor::all().map_err(|e| format!("monitors: {e}"))?;
    if monitors.is_empty() {
        return Err("no monitors found".into());
    }
    let monitor = monitors.iter().find(|m| m.is_primary().unwrap_or(false)).unwrap_or(&monitors[0]);
    let image = monitor.capture_image().map_err(|e| format!("capture: {e}"))?;
    let (w, h) = (image.width(), image.height());
    let mut png: Vec<u8> = Vec::new();
    {
        use image::codecs::png::PngEncoder;
        use image::{ExtendedColorType, ImageEncoder};
        let encoder = PngEncoder::new(&mut png);
        encoder.write_image(image.as_raw(), w, h, ExtendedColorType::Rgba8).map_err(|e| format!("encode: {e}"))?;
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(serde_json::json!({
        "image_base64": b64,
        "media_type": "image/png",
        "width": w,
        "height": h,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_map_image_pixels_to_the_screen_and_back() {
        // A 2560x1440 monitor at x=1920 captured into a 1366-wide image.
        let scale = fit_scale(2560, 1440, MAX_EDGE);
        assert!((scale - 1366.0 / 2560.0).abs() < 1e-9);
        let f = Frame { origin: (1920, 0), scale };
        assert_eq!(f.to_screen(0.0, 0.0), (1920, 0));
        assert_eq!(f.to_screen(683.0, 384.0), (1920 + 1280, 720));
        assert_eq!(f.to_image(1920 + 1280, 720), (683, 384));
        // Small images are never enlarged.
        assert_eq!(fit_scale(800, 600, MAX_EDGE), 1.0);
        assert_eq!(Frame::IDENTITY.to_screen(10.0, 20.0), (10, 20));
    }
}
