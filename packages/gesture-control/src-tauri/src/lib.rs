//! `gesture-control` — a standalone tray app that turns hand gestures (via
//! webcam + MediaPipe) into real Windows OS input (mouse/keyboard), instead
//! of driving a webview DOM the way Vivianne's built-in gesture module does.
//!
//! Architecture: `gesture::sidecar::GestureSidecar` runs the same
//! landmarks → smoothing → classification pipeline as `packages/gui`
//! (copied in as-is; see `gesture/mod.rs`), emitting `gesture_event` Tauri
//! events exactly like the original. The one thing that's different here is
//! what listens to that stream: instead of a frontend DOM handler, `main.rs`
//! wires an internal listener straight to `os_control::OsController`, which
//! turns each event into a real `enigo` mouse/keyboard action against
//! whatever window currently has OS focus.

pub mod gesture;
pub mod os_control;

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, Manager, WindowEvent};

use gesture::commands::{cmd_gesture_set_debug, cmd_gesture_start, cmd_gesture_status, cmd_gesture_stop};
use gesture::events::GestureEvent;
use gesture::sidecar::{resolve_script_path, GestureSidecar};
use os_control::OsController;

const SETTINGS_WINDOW_LABEL: &str = "settings";

/// Toggle tracking on/off from the tray menu — the tray item's checked state
/// mirrors sidecar status, so this fires the same start/stop path the
/// settings window's buttons use.
#[tauri::command]
async fn cmd_toggle_tracking(app: AppHandle, sidecar: tauri::State<'_, Arc<GestureSidecar>>) -> Result<bool, String> {
    if sidecar.is_running().await {
        sidecar.stop().await?;
        Ok(false)
    } else {
        let script = resolve_script_path(&app)?;
        sidecar
            .start(app.clone(), script, 0, gesture::config::GestureConfig::default(), false)
            .await?;
        Ok(true)
    }
}

#[tauri::command]
fn cmd_show_settings(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        let _ = win.show();
        let _ = win.set_focus();
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .manage(Arc::new(GestureSidecar::new()))
        .invoke_handler(tauri::generate_handler![
            cmd_gesture_start,
            cmd_gesture_stop,
            cmd_gesture_status,
            cmd_gesture_set_debug,
            cmd_toggle_tracking,
            cmd_show_settings,
        ])
        .setup(|app| {
            // OS controller: one Enigo handle for the process lifetime,
            // shared via managed state so the gesture-event listener below
            // can reach it.
            let os_controller = Arc::new(OsController::new().map_err(|e| {
                log::error!("[gesture-control] failed to initialize OS input controller: {e}");
                e
            })?);
            app.manage(os_controller.clone());

            // Bridge: every `gesture_event` the sidecar emits also gets
            // applied as real OS input. This is the one piece that
            // distinguishes this app from `packages/gui`'s DOM-scoped
            // gesture module — everything upstream of this listener is the
            // unmodified pipeline.
            let controller_for_listener = os_controller.clone();
            app.listen("gesture_event", move |event| {
                match serde_json::from_str::<GestureEvent>(event.payload()) {
                    Ok(gesture_event) => controller_for_listener.apply(&gesture_event),
                    Err(e) => log::warn!("[gesture-control] malformed gesture_event payload: {e}"),
                }
            });

            // Tray icon: start/stop toggle + quit. No visible main window by
            // default — this is a background utility, the settings window
            // opens on demand from the tray.
            let start_stop = MenuItem::with_id(app, "toggle", "Start Tracking", true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&start_stop, &settings_item, &PredefinedMenuItem::separator(app)?, &quit_item])?;

            let tray = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().cloned().ok_or("no default window icon")?)
                .menu(&menu)
                .show_menu_on_left_click(true)
                .tooltip("Gesture Control (stopped)")
                .on_menu_event(move |app, event| {
                    let app = app.clone();
                    match event.id().as_ref() {
                        "toggle" => {
                            let start_stop = start_stop.clone();
                            tauri::async_runtime::spawn(async move {
                                let sidecar = app.state::<Arc<GestureSidecar>>();
                                let running = if sidecar.is_running().await {
                                    let _ = sidecar.stop().await;
                                    false
                                } else {
                                    match resolve_script_path(&app) {
                                        Ok(script) => {
                                            let cfg = gesture::config::GestureConfig::default();
                                            if let Err(e) = sidecar.start(app.clone(), script, 0, cfg, false).await {
                                                log::error!("[gesture-control] failed to start tracking: {e}");
                                            }
                                        }
                                        Err(e) => log::error!("[gesture-control] {e}"),
                                    }
                                    sidecar.is_running().await
                                };
                                let label = if running { "Stop Tracking" } else { "Start Tracking" };
                                let _ = start_stop.set_text(label);
                                if let Some(tray) = app.tray_by_id("main-tray") {
                                    let _ = tray.set_tooltip(Some(if running {
                                        "Gesture Control (tracking)"
                                    } else {
                                        "Gesture Control (stopped)"
                                    }));
                                }
                            });
                        }
                        "settings" => {
                            if let Some(win) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                        "quit" => {
                            let app = app.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Some(sidecar) = app.try_state::<Arc<GestureSidecar>>() {
                                    let _ = sidecar.stop().await;
                                }
                                app.exit(0);
                            });
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::DoubleClick { .. } = event {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;
            app.manage(tray);

            // Settings window starts hidden — closing it (the "x" button)
            // hides rather than destroys it, and hides the whole app back to
            // just the tray icon, matching how tray-resident utilities
            // usually behave on Windows.
            if let Some(win) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
                let win_for_handler = win.clone();
                win.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win_for_handler.hide();
                    }
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running gesture-control application");
}
