//! The `dip` tool — one merged surface for controlling the machine.
//!
//! Two complementary halves, both reachable through the single `dip` tool
//! (see `agent/tool_schemas.rs` for the schema the model sees). The file is
//! still named `computer.rs` because pixel mode lives here; the module is the
//! dispatcher, and `dip.rs` holds the structured half.
//!
//! - **Pixel mode** (this file, `execute_pointer_action` — cross-platform):
//!   move/click the mouse, type text, press keys, read the cursor position,
//!   and take screenshots. Mouse/keyboard control uses `enigo`; screen
//!   capture uses `xcap`. Screenshots are returned as a base64 PNG in the
//!   tool result `details` so the executor can attach them as an image
//!   content block the model can actually see.
//! - **Structured mode** (`super::dip`, Windows only): reads the real UI
//!   Automation tree instead of guessing pixel coordinates. Preferred for
//!   real application UI; pixel mode is the fallback for canvas-rendered
//!   apps/games that expose nothing useful to UI Automation.
//!
//! These used to be two separate tools (`computer` and six `dip_*` tools).
//! They were merged into one because the split itself was the problem: the
//! model had to pick the right tool out of seven before it could even start,
//! with no guidance on when to reach for which. One tool with an `action`
//! enum, plus the `dip` skill (`~/.viv/skills/dip/SKILL.md` — same
//! live-loaded, no-restart skill mechanism as every other skill; see
//! `coding_agent::skills`) documenting the actual decision process, replaces
//! that guesswork with one flow to learn. `execute_computer_tool` below is
//! the merged dispatch point.
//!
//! The merged tool is named `dip`, so that the tool, this module pair, the
//! skill, and the overlay window all say the same word. `agent/tool_exec.rs`
//! still accepts `computer` as a silent alias: a tool called "computer" with
//! these exact action names is a strong prior for most models, and answering
//! it is cheaper than making a model recover from an unknown-tool error.

use base64::Engine;
use enigo::{
    Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings,
};
use serde_json::json;

use super::tool_exec::ToolResult;

/// Dispatches every `computer` action. Structured-mode actions
/// (`snapshot`/`interact`/`list_windows`/`focus_window`/`window_action`/
/// `launch_app`) only exist on Windows — `dip`'s functions require
/// `uiautomation`, a Windows-only dependency (see `Cargo.toml`'s
/// `[target."cfg(windows)".dependencies]`) — so this function itself is
/// compiled in two variants rather than branching at runtime on an action
/// list that wouldn't even resolve on other platforms.
#[cfg(target_os = "windows")]
pub(crate) async fn execute_computer_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> ToolResult {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return err("'action' is required"),
    };
    match action {
        "snapshot" => super::dip::execute_dip_snapshot_tool(app, args).await,
        "interact" => super::dip::execute_dip_interact_tool(args).await,
        "list_windows" => super::dip::execute_dip_list_windows_tool().await,
        "focus_window" => super::dip::execute_dip_focus_window_tool(args).await,
        "window_action" => super::dip::execute_dip_window_action_tool(args).await,
        "launch_app" => super::dip::execute_dip_launch_app_tool(args).await,
        "point_at" | "highlight" | "clear_pointer" => execute_narration_action(action, args, app),
        _ => execute_pointer_action(args).await,
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) async fn execute_computer_tool(args: &serde_json::Value, app: &tauri::AppHandle) -> ToolResult {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or_default();
    if matches!(action, "point_at" | "highlight" | "clear_pointer") {
        return execute_narration_action(action, args, app);
    }
    execute_pointer_action(args).await
}

/// The narration cursor: point at something, ring a region, or clear it.
///
/// Separate from `execute_pointer_action` (which drives the REAL mouse) because
/// these move nothing and click nothing — they only draw. Keeping them apart is
/// what guarantees a narration call can never have a side effect on the system
/// cursor, focus, or hover state.
///
/// Synchronous: it only positions a window and emits an event, so there is
/// nothing to await, and the model should not be made to wait on a drawing.
fn execute_narration_action(action: &str, args: &serde_json::Value, app: &tauri::AppHandle) -> ToolResult {
    let num = |k: &str| args.get(k).and_then(|v| v.as_f64());
    let label = args.get("text").and_then(|v| v.as_str()).map(str::to_string);

    let outcome = match action {
        "clear_pointer" => {
            crate::instant::pointer::hide(app);
            Ok("Pointer hidden.".to_string())
        }
        "point_at" => match (num("x"), num("y")) {
            (Some(x), Some(y)) => crate::instant::pointer::point(app, x, y, label)
                .map(|()| format!("Pointing at ({x:.0}, {y:.0}).")),
            _ => return err("'point_at' needs 'x' and 'y' screen coordinates."),
        },
        "highlight" => match (num("x"), num("y"), num("width"), num("height")) {
            (Some(x), Some(y), Some(w), Some(h)) => {
                crate::instant::pointer::highlight(app, x, y, w, h, label)
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
    ToolResult {
        content: format!("Error: {}", msg.into()),
        is_error: true,
        details: None,
    }
}

fn ok(text: impl Into<String>) -> ToolResult {
    ToolResult {
        content: text.into(),
        is_error: false,
        details: None,
    }
}

/// Map a key token (e.g. "Return", "ctrl", "a") to an enigo `Key`.
fn parse_key(token: &str) -> Option<Key> {
    let t = token.trim();
    let lower = t.to_lowercase();
    Some(match lower.as_str() {
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
            let mut chars = t.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None; // multi-char, non-named token → unsupported
            }
            Key::Unicode(c)
        }
    })
}

/// Pixel-mode actions: `screenshot`, `mouse_move`, `left_click`,
/// `right_click`, `middle_click`, `double_click`, `left_click_drag`, `type`,
/// `key`, `cursor_position`. Also the fallback for any action name a
/// structured-mode-only platform doesn't recognize.
async fn execute_pointer_action(args: &serde_json::Value) -> ToolResult {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return err("'action' is required"),
    };

    // Screenshot needs no input device.
    if action == "screenshot" {
        return capture_screenshot();
    }

    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(e) => e,
        Err(e) => return err(format!("failed to access input devices: {e}")),
    };

    let coordinate = args.get("coordinate").and_then(|v| v.as_array()).map(|a| {
        (
            a.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            a.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        )
    });
    let text = args.get("text").and_then(|v| v.as_str());

    match action {
        "mouse_move" => match coordinate {
            Some((x, y)) => match enigo.move_mouse(x, y, Coordinate::Abs) {
                Ok(_) => ok(format!("Moved mouse to X={x}, Y={y}")),
                Err(e) => err(format!("mouse_move failed: {e}")),
            },
            None => err("'coordinate' is required for mouse_move"),
        },
        "left_click" => {
            if let Some((x, y)) = coordinate {
                if let Err(e) = enigo.move_mouse(x, y, Coordinate::Abs) {
                    return err(format!("left_click failed: {e}"));
                }
            }
            match enigo.button(Button::Left, Direction::Click) {
                Ok(_) => ok("Left clicked"),
                Err(e) => err(format!("left_click failed: {e}")),
            }
        }
        "right_click" => {
            if let Some((x, y)) = coordinate {
                if let Err(e) = enigo.move_mouse(x, y, Coordinate::Abs) {
                    return err(format!("right_click failed: {e}"));
                }
            }
            match enigo.button(Button::Right, Direction::Click) {
                Ok(_) => ok("Right clicked"),
                Err(e) => err(format!("right_click failed: {e}")),
            }
        }
        "middle_click" => match enigo.button(Button::Middle, Direction::Click) {
            Ok(_) => ok("Middle clicked"),
            Err(e) => err(format!("middle_click failed: {e}")),
        },
        "double_click" => {
            if let Some((x, y)) = coordinate {
                if let Err(e) = enigo.move_mouse(x, y, Coordinate::Abs) {
                    return err(format!("double_click failed: {e}"));
                }
            }
            if let Err(e) = enigo.button(Button::Left, Direction::Click) {
                return err(format!("double_click failed: {e}"));
            }
            match enigo.button(Button::Left, Direction::Click) {
                Ok(_) => ok("Double clicked"),
                Err(e) => err(format!("double_click failed: {e}")),
            }
        }
        "left_click_drag" => match coordinate {
            Some((x, y)) => {
                if let Err(e) = enigo.button(Button::Left, Direction::Press) {
                    return err(format!("left_click_drag failed: {e}"));
                }
                if let Err(e) = enigo.move_mouse(x, y, Coordinate::Abs) {
                    let _ = enigo.button(Button::Left, Direction::Release);
                    return err(format!("left_click_drag failed: {e}"));
                }
                match enigo.button(Button::Left, Direction::Release) {
                    Ok(_) => ok(format!("Dragged to X={x}, Y={y}")),
                    Err(e) => err(format!("left_click_drag failed: {e}")),
                }
            }
            None => err("'coordinate' is required for left_click_drag"),
        },
        "type" => match text {
            Some(t) => match enigo.text(t) {
                Ok(_) => ok(format!("Typed: {t}")),
                Err(e) => err(format!("type failed: {e}")),
            },
            None => err("'text' is required for type"),
        },
        "key" => match text {
            Some(t) => press_key_combo(&mut enigo, t),
            None => err("'text' is required for key"),
        },
        "cursor_position" => match enigo.location() {
            Ok((x, y)) => ok(format!("Cursor is at X={x}, Y={y}")),
            Err(e) => err(format!("cursor_position failed: {e}")),
        },
        other => err(format!("unknown action: {other}")),
    }
}

/// Press a key or key-combo like "Control+Shift+T": modifiers are held down,
/// the final key is clicked, then modifiers are released in reverse order.
fn press_key_combo(enigo: &mut Enigo, combo: &str) -> ToolResult {
    let tokens: Vec<&str> = combo.split('+').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if tokens.is_empty() {
        return err("empty key combo");
    }

    let mut keys = Vec::with_capacity(tokens.len());
    for tok in &tokens {
        match parse_key(tok) {
            Some(k) => keys.push(k),
            None => {
                // Fall back to typing multi-char, non-named tokens verbatim.
                return match enigo.text(combo) {
                    Ok(_) => ok(format!("Pressed key: {combo}")),
                    Err(e) => err(format!("key failed: {e}")),
                };
            }
        }
    }

    let Some((final_key, modifiers)) = keys.split_last() else {
        return err("empty key combo");
    };
    for m in modifiers {
        if let Err(e) = enigo.key(*m, Direction::Press) {
            return err(format!("key failed: {e}"));
        }
    }
    let click_result = enigo.key(*final_key, Direction::Click);
    for m in modifiers.iter().rev() {
        let _ = enigo.key(*m, Direction::Release);
    }
    match click_result {
        Ok(_) => ok(format!("Pressed key: {combo}")),
        Err(e) => err(format!("key failed: {e}")),
    }
}

/// Capture the primary monitor and return it as a base64 PNG in `details`.
fn capture_screenshot() -> ToolResult {
    use xcap::Monitor;

    let monitors = match Monitor::all() {
        Ok(m) => m,
        Err(e) => return err(format!("could not enumerate monitors: {e}")),
    };
    if monitors.is_empty() {
        return err("no monitors found");
    }
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .unwrap_or(&monitors[0]);

    let image = match monitor.capture_image() {
        Ok(img) => img,
        Err(e) => return err(format!("screen capture failed: {e}")),
    };

    let (w, h) = (image.width(), image.height());
    let mut png: Vec<u8> = Vec::new();
    {
        use image::codecs::png::PngEncoder;
        use image::{ExtendedColorType, ImageEncoder};
        let encoder = PngEncoder::new(&mut png);
        if let Err(e) = encoder.write_image(image.as_raw(), w, h, ExtendedColorType::Rgba8) {
            return err(format!("failed to encode screenshot: {e}"));
        }
    }

    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    ToolResult {
        content: format!("Screenshot captured [{w}x{h}, image/png]"),
        is_error: false,
        details: Some(json!({
            "image_base64": b64,
            "media_type": "image/png",
            "width": w,
            "height": h,
        })),
    }
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
        encoder
            .write_image(image.as_raw(), w, h, ExtendedColorType::Rgba8)
            .map_err(|e| format!("encode: {e}"))?;
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(serde_json::json!({
        "image_base64": b64,
        "media_type": "image/png",
        "width": w,
        "height": h,
    }))
}
