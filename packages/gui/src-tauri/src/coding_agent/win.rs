//! The Win32 layer under `dip`: input, focus, windows, launching.
//!
//! Everything here goes straight to the OS rather than through `enigo` or UI
//! Automation, for three reasons that each made `dip` fail in practice:
//!
//! - **Typing went nowhere.** Input goes to the foreground window, and while
//!   the agent works, that is Vivianne. `ensure_target_foreground` puts the
//!   agent's target window back in front first, and `force_foreground` gets
//!   past Windows' foreground lock (a background process may not normally
//!   steal focus) by attaching to the foreground thread's input queue — the
//!   documented way, not a hack around it.
//! - **Keys were missing.** The old key map knew ~20 names. `vk_for` knows the
//!   function keys to F24, the numpad, media and browser keys, and anything
//!   else by its character; extended keys carry `KEYEVENTF_EXTENDEDKEY`, or
//!   arrows turn into numpad digits in some apps.
//! - **It was slow.** Typing is one batched `SendInput` of Unicode events, not
//!   one call per character; window listing is `EnumWindows`, not a UI
//!   Automation tree walk.
//!
//! Input that Windows refuses (UIPI: the target runs elevated and we do not)
//! is detected and reported as such, instead of "typed" into nothing.

use std::sync::Mutex;

use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, GetProcessId, OpenProcess, OpenProcessToken,
    QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::*;

// ── Windows ────────────────────────────────────────────────────────────────

/// A top-level window, as the agent sees it.
#[derive(Debug, Clone)]
pub struct Window {
    pub hwnd: isize,
    pub title: String,
    pub pid: u32,
    pub process: String,
    pub minimized: bool,
}

pub fn hwnd(h: isize) -> HWND {
    HWND(h as *mut core::ffi::c_void)
}

fn raw(h: HWND) -> isize {
    h.0 as isize
}

pub fn title(h: isize) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd(h), &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

pub fn pid_of(h: isize) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd(h), Some(&mut pid)) };
    pid
}

pub fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().unwrap_or(&path).to_string()
    }
}

/// Whether this window belongs to Vivianne itself.
pub fn is_own(h: isize) -> bool {
    pid_of(h) == unsafe { GetCurrentProcessId() }
}

/// Visible, titled, not a tool window, not cloaked (a UWP app suspended in
/// the background reports visible but draws nothing).
fn is_user_window(h: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(h).as_bool() || GetWindowTextLengthW(h) == 0 {
            return false;
        }
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
        cloaked == 0
    }
}

unsafe extern "system" fn collect(h: HWND, lparam: LPARAM) -> BOOL {
    let out = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };
    if is_user_window(h) {
        out.push(raw(h));
    }
    BOOL(1)
}

/// Every top-level window a person would call "open", front to back.
pub fn list_windows() -> Vec<Window> {
    let mut handles: Vec<isize> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut handles as *mut Vec<isize> as isize));
    }
    handles
        .into_iter()
        .map(|h| {
            let pid = pid_of(h);
            Window { hwnd: h, title: title(h), pid, process: process_name(pid), minimized: unsafe { IsIconic(hwnd(h)).as_bool() } }
        })
        .collect()
}

pub fn foreground() -> isize {
    raw(unsafe { GetForegroundWindow() })
}

pub fn rect(h: isize) -> Option<(i32, i32, i32, i32)> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd(h), &mut r).ok()? };
    Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
}

/// The top-level window under a screen point.
pub fn window_at(x: i32, y: i32) -> isize {
    unsafe {
        let child = WindowFromPoint(POINT { x, y });
        raw(GetAncestor(child, GA_ROOT))
    }
}

/// Bring a window to the front and give it the keyboard.
///
/// `SetForegroundWindow` alone is refused whenever another process owns the
/// foreground — which, while the user watches the agent, is always. The
/// documented way through is to attach this thread's input to the current
/// foreground thread's for the duration of the switch. Verified afterwards:
/// a switch Windows still declines is reported, not assumed.
pub fn force_foreground(h: isize) -> Result<(), String> {
    let target = hwnd(h);
    unsafe {
        if !IsWindow(Some(target)).as_bool() {
            return Err("that window no longer exists — call list_windows again".into());
        }
        if IsIconic(target).as_bool() {
            let _ = ShowWindow(target, SW_RESTORE);
        }
        if GetForegroundWindow() == target {
            return Ok(());
        }
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let me = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
        let _ = BringWindowToTop(target);
        let _ = SetForegroundWindow(target);
        if attached {
            let _ = AttachThreadInput(me, fg_thread, false);
        }
        // Still refused: a tapped (unmodified) Alt counts as user input and
        // lifts the lock — the long-standing workaround the OS itself honours.
        if GetForegroundWindow() != target {
            send(&[key_input(VK_MENU, false), key_input(VK_MENU, true)]);
            let _ = SetForegroundWindow(target);
        }
        for _ in 0..20 {
            if GetForegroundWindow() == target {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    Err(format!("Windows kept '{}' in front instead", title(foreground())))
}

pub fn show(h: isize, how: &str) -> Result<(), String> {
    let cmd = match how {
        "minimize" => SW_MINIMIZE,
        "maximize" => SW_MAXIMIZE,
        "restore" => SW_RESTORE,
        "close" => {
            unsafe { PostMessageW(Some(hwnd(h)), WM_CLOSE, WPARAM(0), LPARAM(0)) }.map_err(|e| e.to_string())?;
            return Ok(());
        }
        other => return Err(format!("unknown window_state '{other}' — use minimize, maximize, restore or close")),
    };
    unsafe {
        let _ = ShowWindow(hwnd(h), cmd);
    }
    Ok(())
}

// ── The target window ──────────────────────────────────────────────────────

/// The window the agent is working in: the last one it listed and picked,
/// focused, launched, snapshotted or clicked in. Typing and keys go here even
/// when the user has clicked back to Vivianne to watch.
static TARGET: Mutex<Option<isize>> = Mutex::new(None);

pub fn set_target(h: isize) {
    if h != 0 && !is_own(h) {
        *TARGET.lock().unwrap_or_else(|e| e.into_inner()) = Some(h);
    }
}

pub fn target() -> Option<isize> {
    let t = (*TARGET.lock().unwrap_or_else(|e| e.into_inner()))?;
    unsafe { IsWindow(Some(hwnd(t))).as_bool() }.then_some(t)
}

/// Make sure keyboard input will land in the agent's window, not Vivianne.
///
/// If some other app is already in front, that is where the user or the
/// agent put it, and it becomes the target. If Vivianne is in front, the
/// target goes back in front — and without a target, the call is refused
/// rather than typing into the chat.
pub fn ensure_target_foreground() -> Result<isize, String> {
    let fg = foreground();
    if fg != 0 && !is_own(fg) {
        if target() != Some(fg) {
            set_target(fg);
        }
        return Ok(fg);
    }
    let Some(t) = target() else {
        return Err(
            "Vivianne's own window is in front, and no target window is known yet — typing now would go into the chat. \
             Call list_windows and focus_window (or launch_app, or click inside the app) first."
                .into(),
        );
    };
    force_foreground(t)?;
    Ok(t)
}

// ── Elevation ──────────────────────────────────────────────────────────────

fn elevated(pid: u32) -> Option<bool> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut token = HANDLE::default();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token).is_ok();
        let _ = CloseHandle(process);
        if !opened {
            return None;
        }
        let mut info = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut info as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok.then_some(info.TokenIsElevated != 0)
    }
}

/// Why input to this window would be silently dropped, if it would.
///
/// Windows does not let a normal process send input to one running as
/// administrator (UIPI). `SendInput` still "succeeds" in some cases, so this
/// is checked up front to say what is wrong instead of typing into nothing.
pub fn input_blocked_reason(h: isize) -> Option<String> {
    let me = unsafe { GetCurrentProcessId() };
    let target_elevated = elevated(pid_of(h)).unwrap_or(false);
    let me_elevated = elevated(me).unwrap_or(false);
    (target_elevated && !me_elevated).then(|| {
        format!(
            "'{}' is running as administrator and Vivianne is not, so Windows blocks input to it. Run Vivianne as administrator to control it, or close and reopen that app normally.",
            title(h)
        )
    })
}

// ── Launching ──────────────────────────────────────────────────────────────

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Start anything the Run dialog would: an exe on PATH or by full path, an
/// app alias ("notepad", "calc"), a document, a folder, or a URI
/// ("ms-settings:display", "https://…"). Returns the new process id when the
/// shell reports one.
pub fn launch(command: &str, args: &str) -> Result<Option<u32>, String> {
    let verb = wide("open");
    let file = wide(command);
    let params = wide(args);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: if args.is_empty() { PCWSTR::null() } else { PCWSTR(params.as_ptr()) },
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| format!("couldn't start '{command}': {}", e.message()))?;
        if info.hProcess.is_invalid() {
            return Ok(None);
        }
        let pid = GetProcessId(info.hProcess);
        let _ = CloseHandle(info.hProcess);
        Ok((pid != 0).then_some(pid))
    }
}

// ── Input ──────────────────────────────────────────────────────────────────

fn send(inputs: &[INPUT]) -> u32 {
    if inputs.is_empty() {
        return 0;
    }
    unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) }
}

/// Keys whose scan code needs the extended flag — without it, arrows,
/// Home/End and friends arrive as their numpad twins in some apps.
fn is_extended(vk: VIRTUAL_KEY) -> bool {
    matches!(
        vk,
        VK_INSERT | VK_DELETE | VK_HOME | VK_END | VK_PRIOR | VK_NEXT | VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN
            | VK_RCONTROL | VK_RMENU | VK_LWIN | VK_RWIN | VK_APPS | VK_DIVIDE | VK_NUMLOCK | VK_SNAPSHOT
    )
}

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) } as u16;
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn unicode_input(unit: u16, up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: unit, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

/// The input events that type `text`. Line breaks are real Enter presses and
/// tabs real Tab presses (a Unicode "\n" is ignored by most edit controls);
/// everything else is sent as UTF-16, so emoji and any script type as-is.
pub fn text_inputs(text: &str) -> Vec<INPUT> {
    let mut out = Vec::with_capacity(text.len() * 2);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.extend([key_input(VK_RETURN, false), key_input(VK_RETURN, true)]);
            }
            '\n' => out.extend([key_input(VK_RETURN, false), key_input(VK_RETURN, true)]),
            '\t' => out.extend([key_input(VK_TAB, false), key_input(VK_TAB, true)]),
            _ => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf).iter() {
                    out.push(unicode_input(*unit, false));
                    out.push(unicode_input(*unit, true));
                }
            }
        }
    }
    out
}

/// Type text into whatever has the keyboard.
///
/// One character at a time — each character's events in their own
/// `SendInput`, with a moment between. A single large batch is faster on
/// paper and wrong in practice: modern apps (the new Notepad, anything
/// WinUI) coalesce a flood of Unicode key events, dropping accents and emoji
/// and repeating characters. Paced like this it still types ~300 characters
/// a second, and every one lands.
pub fn type_text(text: &str) -> Result<usize, String> {
    let inputs = text_inputs(text);
    let mut chars = 0usize;
    let mut i = 0;
    while i < inputs.len() {
        // One character: a down/up pair, or two pairs for a surrogate pair
        // (the high and low halves must arrive together).
        let unit = unsafe { inputs[i].Anonymous.ki };
        let is_high = unit.dwFlags.contains(KEYEVENTF_UNICODE) && (0xD800..0xDC00).contains(&unit.wScan);
        let len = if is_high { 4 } else { 2 };
        let end = (i + len).min(inputs.len());
        if (send(&inputs[i..end]) as usize) < end - i {
            return Err(
                "Windows blocked the input partway (another program may have locked input, or the target runs as administrator)".into(),
            );
        }
        chars += 1;
        i = end;
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    Ok(chars)
}

/// A key by name or character: "enter", "ctrl", "f5", "a", "/", "volume_up".
pub fn vk_for(name: &str) -> Option<VIRTUAL_KEY> {
    let n = name.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    let vk = match n.as_str() {
        "ctrl" | "control" | "lctrl" | "ctl" => VK_CONTROL,
        "rctrl" | "rcontrol" => VK_RCONTROL,
        "shift" | "lshift" => VK_SHIFT,
        "rshift" => VK_RSHIFT,
        "alt" | "option" | "lalt" | "menu" => VK_MENU,
        "ralt" | "altgr" => VK_RMENU,
        "win" | "windows" | "meta" | "super" | "cmd" | "command" | "lwin" => VK_LWIN,
        "rwin" => VK_RWIN,
        "enter" | "return" => VK_RETURN,
        "tab" => VK_TAB,
        "esc" | "escape" => VK_ESCAPE,
        "space" | "spacebar" => VK_SPACE,
        "backspace" | "back" | "bksp" => VK_BACK,
        "delete" | "del" => VK_DELETE,
        "insert" | "ins" => VK_INSERT,
        "home" => VK_HOME,
        "end" => VK_END,
        "pageup" | "pgup" | "page_up" | "prior" => VK_PRIOR,
        "pagedown" | "pgdn" | "page_down" | "next" => VK_NEXT,
        "up" | "arrowup" | "up_arrow" => VK_UP,
        "down" | "arrowdown" | "down_arrow" => VK_DOWN,
        "left" | "arrowleft" | "left_arrow" => VK_LEFT,
        "right" | "arrowright" | "right_arrow" => VK_RIGHT,
        "capslock" | "caps_lock" | "caps" => VK_CAPITAL,
        "numlock" | "num_lock" => VK_NUMLOCK,
        "scrolllock" | "scroll_lock" => VK_SCROLL,
        "printscreen" | "print_screen" | "prtsc" | "prtscn" | "snapshot" => VK_SNAPSHOT,
        "pause" | "break" => VK_PAUSE,
        "apps" | "contextmenu" | "context_menu" => VK_APPS,
        "volume_up" | "volumeup" => VK_VOLUME_UP,
        "volume_down" | "volumedown" => VK_VOLUME_DOWN,
        "volume_mute" | "mute" => VK_VOLUME_MUTE,
        "media_play_pause" | "play_pause" | "playpause" => VK_MEDIA_PLAY_PAUSE,
        "media_next" | "next_track" => VK_MEDIA_NEXT_TRACK,
        "media_prev" | "media_previous" | "prev_track" | "previous_track" => VK_MEDIA_PREV_TRACK,
        "media_stop" => VK_MEDIA_STOP,
        "browser_back" => VK_BROWSER_BACK,
        "browser_forward" => VK_BROWSER_FORWARD,
        "browser_refresh" => VK_BROWSER_REFRESH,
        "browser_home" => VK_BROWSER_HOME,
        "num_add" | "numpad_add" | "add" => VK_ADD,
        "num_subtract" | "numpad_subtract" | "subtract" => VK_SUBTRACT,
        "num_multiply" | "numpad_multiply" | "multiply" => VK_MULTIPLY,
        "num_divide" | "numpad_divide" | "divide" => VK_DIVIDE,
        "num_decimal" | "numpad_decimal" | "decimal" => VK_DECIMAL,
        "plus" => VK_OEM_PLUS,
        "minus" => VK_OEM_MINUS,
        _ => {
            // F1–F24.
            if let Some(num) = n.strip_prefix('f').and_then(|d| d.parse::<u16>().ok()) {
                if (1..=24).contains(&num) {
                    return Some(VIRTUAL_KEY(VK_F1.0 + num - 1));
                }
            }
            // Numpad digits: "num5", "numpad5".
            if let Some(d) = n.strip_prefix("numpad").or_else(|| n.strip_prefix("num")).and_then(|d| d.parse::<u16>().ok()) {
                if d <= 9 {
                    return Some(VIRTUAL_KEY(VK_NUMPAD0.0 + d));
                }
            }
            // Any single character, by the key that produces it here.
            let mut chars = name.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            if c.is_ascii_alphanumeric() {
                return Some(VIRTUAL_KEY(c.to_ascii_uppercase() as u16));
            }
            let mut buf = [0u16; 2];
            let units = c.encode_utf16(&mut buf);
            if units.len() != 1 {
                return None;
            }
            let scanned = unsafe { VkKeyScanW(units[0]) };
            if scanned == -1 {
                return None;
            }
            return Some(VIRTUAL_KEY((scanned as u16) & 0xff));
        }
    };
    Some(vk)
}

/// "ctrl+shift+t" → its keys, in order. `+` itself is written "plus".
pub fn parse_combo(combo: &str) -> Result<Vec<VIRTUAL_KEY>, String> {
    let parts: Vec<&str> = combo.split('+').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Err("empty key combination".into());
    }
    parts
        .iter()
        .map(|p| vk_for(p).ok_or_else(|| format!("unknown key '{p}' — use names like ctrl, shift, alt, win, enter, tab, esc, f5, up, pagedown, or a single character")))
        .collect()
}

/// Press a combination: every key down in order, then up in reverse, in one
/// batch so nothing can slip between them.
pub fn press(keys: &[VIRTUAL_KEY], times: u32) -> Result<(), String> {
    for i in 0..times.max(1) {
        let mut inputs: Vec<INPUT> = keys.iter().map(|k| key_input(*k, false)).collect();
        inputs.extend(keys.iter().rev().map(|k| key_input(*k, true)));
        if (send(&inputs) as usize) < inputs.len() {
            return Err("Windows blocked the key press".into());
        }
        if i + 1 < times {
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    }
    Ok(())
}

pub fn key_down(keys: &[VIRTUAL_KEY]) -> Result<(), String> {
    let inputs: Vec<INPUT> = keys.iter().map(|k| key_input(*k, false)).collect();
    ((send(&inputs) as usize) == inputs.len()).then_some(()).ok_or_else(|| "Windows blocked the key press".into())
}

pub fn key_up(keys: &[VIRTUAL_KEY]) -> Result<(), String> {
    let inputs: Vec<INPUT> = keys.iter().rev().map(|k| key_input(*k, true)).collect();
    ((send(&inputs) as usize) == inputs.len()).then_some(()).ok_or_else(|| "Windows blocked the key release".into())
}

fn mouse_input(flags: MOUSE_EVENT_FLAGS, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

pub fn move_to(x: i32, y: i32) -> Result<(), String> {
    unsafe { SetCursorPos(x, y) }.map_err(|e| format!("couldn't move the mouse: {}", e.message()))
}

pub fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    let _ = unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

#[derive(Clone, Copy)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

fn button_flags(b: MouseButton) -> (MOUSE_EVENT_FLAGS, MOUSE_EVENT_FLAGS) {
    match b {
        MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
    }
}

pub fn click(b: MouseButton, count: u32) -> Result<(), String> {
    let (down, up) = button_flags(b);
    let mut inputs = Vec::new();
    for _ in 0..count.max(1) {
        inputs.push(mouse_input(down, 0));
        inputs.push(mouse_input(up, 0));
    }
    ((send(&inputs) as usize) == inputs.len()).then_some(()).ok_or_else(|| "Windows blocked the click".into())
}

pub fn button(b: MouseButton, down: bool) -> Result<(), String> {
    let (d, u) = button_flags(b);
    ((send(&[mouse_input(if down { d } else { u }, 0)])) == 1).then_some(()).ok_or_else(|| "Windows blocked the mouse button".into())
}

/// Scroll by notches (one wheel click each). Positive is up / right.
pub fn scroll(vertical: i32, horizontal: i32) -> Result<(), String> {
    let mut inputs = Vec::new();
    if vertical != 0 {
        inputs.push(mouse_input(MOUSEEVENTF_WHEEL, vertical * WHEEL_DELTA as i32));
    }
    if horizontal != 0 {
        inputs.push(mouse_input(MOUSEEVENTF_HWHEEL, horizontal * WHEEL_DELTA as i32));
    }
    ((send(&inputs) as usize) == inputs.len()).then_some(()).ok_or_else(|| "Windows blocked the scroll".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_characters_map_to_keys() {
        assert_eq!(vk_for("Enter"), Some(VK_RETURN));
        assert_eq!(vk_for("ctrl"), Some(VK_CONTROL));
        assert_eq!(vk_for("Win"), Some(VK_LWIN));
        assert_eq!(vk_for("F5"), Some(VK_F5));
        assert_eq!(vk_for("f24"), Some(VK_F24));
        assert_eq!(vk_for("numpad7"), Some(VK_NUMPAD7));
        assert_eq!(vk_for("page down"), Some(VK_NEXT));
        assert_eq!(vk_for("volume_up"), Some(VK_VOLUME_UP));
        assert_eq!(vk_for("a"), Some(VIRTUAL_KEY(b'A' as u16)));
        assert_eq!(vk_for("7"), Some(VIRTUAL_KEY(b'7' as u16)));
        assert_eq!(vk_for("f25"), None);
        assert_eq!(vk_for("banana"), None);
    }

    #[test]
    fn combos_parse_in_order() {
        assert_eq!(parse_combo("Ctrl+Shift+Esc").unwrap(), vec![VK_CONTROL, VK_SHIFT, VK_ESCAPE]);
        assert_eq!(parse_combo("win + r").unwrap(), vec![VK_LWIN, VIRTUAL_KEY(b'R' as u16)]);
        assert_eq!(parse_combo("ctrl+plus").unwrap(), vec![VK_CONTROL, VK_OEM_PLUS]);
        assert!(parse_combo("ctrl+nope").is_err());
        assert!(parse_combo("").is_err());
    }

    #[test]
    fn text_becomes_unicode_events_with_real_enter() {
        // "a" = down+up; emoji = a surrogate pair, each unit down+up; "\n" = Enter.
        let events = text_inputs("a👋\nb");
        assert_eq!(events.len(), 2 + 4 + 2 + 2);
        let ki = |i: usize| unsafe { events[i].Anonymous.ki };
        assert_eq!(ki(0).wScan, 'a' as u16);
        assert!(ki(0).dwFlags.contains(KEYEVENTF_UNICODE));
        assert_eq!(ki(2).wScan, 0xD83D); // high surrogate of 👋
        assert_eq!(ki(4).wScan, 0xDC4B); // low surrogate
        assert_eq!(ki(6).wVk, VK_RETURN);
        assert!(!ki(6).dwFlags.contains(KEYEVENTF_UNICODE));
        // "\r\n" is one Enter, not two.
        assert_eq!(text_inputs("x\r\ny").len(), 6);
    }

    #[test]
    fn navigation_keys_are_extended() {
        assert!(is_extended(VK_LEFT) && is_extended(VK_DELETE) && is_extended(VK_LWIN));
        assert!(!is_extended(VK_RETURN) && !is_extended(VIRTUAL_KEY(b'A' as u16)));
    }
}
