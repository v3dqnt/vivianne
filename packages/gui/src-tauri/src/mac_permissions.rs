//! The macOS privacy permissions Vivianne needs, and how to ask for them.
//!
//! macOS gates each of these behind a switch in System Settings → Privacy &
//! Security, and an app can't flip them itself. The most it can do is check,
//! ask macOS to show its prompt, and open the right pane — which is what
//! Settings → Viv Instant → Mac permissions does with these commands.
//!
//! - **Accessibility**: reading and operating other apps' controls (`dip`).
//! - **Screen Recording**: screenshots of other apps' windows.
//! - **Microphone**: Viv Instant's voice. macOS asks the first time the
//!   microphone is opened, using `NSMicrophoneUsageDescription`.
//! - **Automation**: sending Apple Events to other apps; macOS asks per app,
//!   the first time.
//!
//! On other platforms every command reports "not applicable".

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Permission {
    pub id: &'static str,
    pub label: &'static str,
    pub why: &'static str,
    /// `granted` | `denied` | `ask` (macOS hasn't asked yet / can't tell)
    pub state: &'static str,
}

#[tauri::command]
pub async fn cmd_mac_permissions() -> Result<Vec<Permission>, String> {
    #[cfg(target_os = "macos")]
    {
        use crate::coding_agent::mac;
        let yes = |b: bool| if b { "granted" } else { "denied" };
        Ok(vec![
            Permission { id: "accessibility", label: "Accessibility", why: "Lets Vivianne read and operate other apps' buttons, fields and menus.", state: yes(mac::trusted()) },
            Permission { id: "screen", label: "Screen Recording", why: "Lets Vivianne take screenshots of other apps' windows.", state: yes(mac::screen_capture_allowed()) },
            Permission { id: "microphone", label: "Microphone", why: "For talking to Viv Instant. macOS asks the first time you hold the keys.", state: "ask" },
            Permission { id: "automation", label: "Automation", why: "Lets Vivianne send commands to other apps. macOS asks once per app.", state: "ask" },
        ])
    }
    #[cfg(not(target_os = "macos"))]
    Ok(Vec::new())
}

/// Ask macOS for one permission: its own prompt where there is one, and the
/// right pane of System Settings either way.
#[tauri::command]
pub async fn cmd_mac_request_permission(id: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use crate::coding_agent::mac;
        let pane = match id.as_str() {
            "accessibility" => {
                mac::request_trust();
                "Privacy_Accessibility"
            }
            "screen" => {
                mac::request_screen_capture();
                "Privacy_ScreenCapture"
            }
            "microphone" => "Privacy_Microphone",
            "automation" => "Privacy_Automation",
            other => return Err(format!("unknown permission '{other}'")),
        };
        std::process::Command::new("open")
            .arg(format!("x-apple.systempreferences:com.apple.preference.security?{pane}"))
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = id;
        Ok(())
    }
}
