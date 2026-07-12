//! Computer tool — Rust rewrite of `packages/coding-agent/src/core/tools/computer.ts`.
//!
//! Lets the agent control the machine: move/click the mouse, type text, press
//! keys, read the cursor position, and take screenshots. Mouse/keyboard control
//! uses `enigo`; screen capture uses `xcap`. Screenshots are returned as a
//! base64 PNG in the tool result `details` so the executor can attach them as an
//! image content block the model can actually see.

use async_trait::async_trait;
use base64::Engine;
use enigo::{
    Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings,
};
use serde_json::json;

use super::tool_exec::{Tool, ToolResult};

pub struct ComputerTool {
    auto_resize: bool,
}

impl ComputerTool {
    pub fn new() -> Self {
        Self { auto_resize: true }
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

#[async_trait]
impl Tool for ComputerTool {
    fn name(&self) -> &str {
        "computer"
    }

    fn description(&self) -> &str {
        "Control the computer: move and click the mouse, type text, press keys, and take screenshots."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "key", "type", "mouse_move", "left_click", "left_click_drag",
                        "right_click", "middle_click", "double_click", "screenshot", "cursor_position"
                    ],
                    "description": "The action to perform."
                },
                "coordinate": {
                    "type": "array",
                    "items": { "type": "number" },
                    "description": "X,Y screen coordinates for mouse actions."
                },
                "text": { "type": "string", "description": "Text to type, or key combination to press (e.g. 'Control+C')." }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &serde_json::Value, _cwd: &str) -> ToolResult {
        let action = match args.get("action").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return err("'action' is required"),
        };

        // Screenshot needs no input device.
        if action == "screenshot" {
            return capture_screenshot(self.auto_resize);
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
            "left_click" => match enigo.button(Button::Left, Direction::Click) {
                Ok(_) => ok("Left clicked"),
                Err(e) => err(format!("left_click failed: {e}")),
            },
            "right_click" => match enigo.button(Button::Right, Direction::Click) {
                Ok(_) => ok("Right clicked"),
                Err(e) => err(format!("right_click failed: {e}")),
            },
            "middle_click" => match enigo.button(Button::Middle, Direction::Click) {
                Ok(_) => ok("Middle clicked"),
                Err(e) => err(format!("middle_click failed: {e}")),
            },
            "double_click" => {
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
fn capture_screenshot(_auto_resize: bool) -> ToolResult {
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
