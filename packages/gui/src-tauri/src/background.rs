//! Keeping Vivianne alive after its window is closed.
//!
//! ## Why this exists
//!
//! Viv Instant is summoned by a global chord from anywhere on the desktop, so
//! it has to work when Vivianne is not on screen. Two things previously made
//! that impossible:
//!
//! 1. Closing the main window destroyed the last window, and Tauri exits the
//!    process when the last window goes. The global shortcut died with it.
//! 2. Even if the process survived, every LLM call is answered by the **main
//!    window's** provider dispatcher — a piece of frontend JavaScript (see
//!    `main.tsx`, which starts it only for `label === 'main'`). No main window
//!    means no dispatcher, and `cmd_instant_run` refuses to start rather than
//!    hang for the 120s idle guard.
//!
//! Both are solved the same way: **closing the main window hides it instead of
//! destroying it.** The webview stays loaded and the dispatcher stays live, so
//! a hidden Vivianne answers Viv Instant exactly as a visible one does — and
//! reopening is instant, because nothing was torn down.
//!
//! ## The cost, stated plainly
//!
//! A hidden window is still a running WebView2 process with the whole React app
//! in it. That is real memory to leave resident on a machine the user thinks
//! they closed the app on. It is the deliberate trade for a feature whose
//! entire premise is being available without being open — and it is why the
//! tray icon exists and why Quit genuinely quits, rather than leaving the only
//! exit as Task Manager.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

/// Set once the user has actually chosen to quit.
///
/// Without it the close handler cannot tell "the user closed the window" from
/// "the app is shutting down", and would veto the close during a real quit —
/// making the app impossible to exit.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// Whether a genuine quit is in progress.
pub fn is_quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

/// Show and focus the main window, restoring it if minimized.
///
/// Unlike the Viv Instant overlay — which must never take focus — this one
/// should: the user has explicitly asked for the app itself.
pub fn show_main(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// Quit for real.
pub fn quit(app: &tauri::AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);
    app.exit(0);
}

/// Install the tray icon and the close-to-background handler.
///
/// Called from `setup`. Failing to build the tray is logged and swallowed
/// rather than aborting startup: a missing tray is a degraded app, but a
/// Vivianne that refuses to launch because of one is a broken one.
pub fn init(app: &tauri::AppHandle) {
    if let Err(e) = install_tray(app) {
        log::warn!("[background] couldn't create the tray icon: {e}");
    }
    install_close_handler(app);
}

fn install_tray(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let open = MenuItem::with_id(app, "open", "Open Vivianne", true, None::<&str>)?;
    let instant = MenuItem::with_id(app, "instant", "Viv Instant", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &instant, &quit_item])?;

    TrayIconBuilder::with_id("viv_tray")
        .icon(app.default_window_icon().cloned().ok_or("no bundled window icon")?)
        .tooltip("Vivianne — hold Ctrl+Shift+Space and speak for Viv Instant")
        .menu(&menu)
        // The menu must NOT open on a left click: left click is bound to
        // "show the app", which is what every other tray app on Windows does
        // and what users try first.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "instant" => crate::instant::window::toggle(app),
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn install_close_handler(app: &tauri::AppHandle) {
    let Some(main) = app.get_webview_window("main") else {
        log::warn!("[background] no main window at setup — close-to-background not installed");
        return;
    };
    let handle = app.clone();
    main.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            // During a real quit the close must be allowed through, or `exit`
            // deadlocks against its own window teardown.
            if is_quitting() {
                return;
            }
            api.prevent_close();
            if let Some(win) = handle.get_webview_window("main") {
                let _ = win.hide();
            }
            log::info!("[background] main window hidden; Viv Instant stays available");
        }
    });
}

/// Hide the main window to the tray.
///
/// Exposed as a command so the title bar's close button takes the same path as
/// the OS close, rather than calling `window.close()` and relying on the event
/// handler to veto it.
#[tauri::command]
pub async fn cmd_hide_to_background(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Quit the app for real, from the UI.
#[tauri::command]
pub async fn cmd_quit_app(app: tauri::AppHandle) -> Result<(), String> {
    quit(&app);
    Ok(())
}
