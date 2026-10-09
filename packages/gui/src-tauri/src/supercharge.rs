//! Supercharge: give Vivianne the machine.
//!
//! With the user's go-ahead, close the apps they don't need right now, quit
//! background sync clients and launchers that eat bandwidth and memory, run
//! Vivianne at above-normal priority, and switch Windows to the High
//! performance power plan. Turning it off puts the power plan and priority
//! back, and can reopen what it closed.
//!
//! What it will and won't touch:
//! - Apps are asked to close the normal way (`WM_CLOSE`), so one with unsaved
//!   work can ask to save, and stays open if the user says so. Only
//!   background clients that ignore the request (sync clients, launchers) are
//!   ended, and only if the user ticked them.
//! - Never offered: Vivianne and its own helper processes, whatever launched
//!   Vivianne (the terminal running a dev build), Windows' own shell, and
//!   apps running as administrator (Windows won't let a normal app close
//!   those, and they're usually there for a reason).
//! - Offered but unticked: browsers, terminals, editors, and the Claude app,
//!   which may be holding work the user needs right now.
//! - No Windows services are stopped and nothing needs administrator rights.
//!
//! On macOS the same idea, within what macOS lets an app do without an admin
//! password: apps are asked to quit the way ⌘Q does (so unsaved work can
//! still prompt), ticked background clients are force-quit, and the Mac is
//! kept awake with `caffeinate` while Supercharge is on. There is no power
//! plan to switch and no priority to raise without root, so it doesn't try.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// The executable's full path, lowercased: what a selection refers to.
    pub id: String,
    pub name: String,
    pub exe: String,
    /// What it is showing, when it has a window.
    pub title: Option<String>,
    /// No window of its own: a tray or sync client, a launcher.
    pub background: bool,
    pub processes: usize,
    pub memory_mb: u64,
    /// Ticked by default.
    pub recommended: bool,
    /// Why it isn't ticked, when it isn't.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosedApp {
    pub name: String,
    pub exe: String,
}

/// Kept on disk, so turning it off still restores after a restart.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuperchargeState {
    pub on: bool,
    pub since: Option<u64>,
    /// The power plan that was active before, to switch back to.
    pub previous_power_plan: Option<String>,
    pub closed: Vec<ClosedApp>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    pub closed: Vec<String>,
    /// Asked to close and still open, most likely asking to save.
    pub still_open: Vec<String>,
    pub power_plan: String,
    pub freed_mb: u64,
    pub state: SuperchargeState,
}

const HIGH_PERFORMANCE: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";

/// Never offered: Windows' shell and session processes.
const SYSTEM: &[&str] = &[
    "explorer.exe", "dwm.exe", "csrss.exe", "winlogon.exe", "sihost.exe", "ctfmon.exe", "conhost.exe",
    "applicationframehost.exe", "shellexperiencehost.exe", "startmenuexperiencehost.exe", "searchhost.exe",
    "searchapp.exe", "textinputhost.exe", "lockapp.exe", "systemsettingsbroker.exe", "runtimebroker.exe",
    "taskhostw.exe", "securityhealthsystray.exe", "smartscreen.exe", "widgets.exe", "phoneexperiencehost.exe",
    "nvidia share.exe", "nvcontainer.exe", "rtkuwp.exe", "msedgewebview2.exe",
];

/// Background clients worth quitting: sync, launchers, chat, music.
const BACKGROUND: &[&str] = &[
    "onedrive.exe", "dropbox.exe", "googledrivefs.exe", "icloudservices.exe", "iclouddrive.exe", "box.exe",
    "steam.exe", "steamwebhelper.exe", "epicgameslauncher.exe", "epicwebhelper.exe", "battle.net.exe",
    "eadesktop.exe", "origin.exe", "upc.exe", "ubisoftconnect.exe", "gog galaxy.exe", "riotclientservices.exe",
    "discord.exe", "spotify.exe", "ms-teams.exe", "teams.exe", "slack.exe", "whatsapp.exe", "telegram.exe",
    "zoom.exe", "skype.exe", "creative cloud.exe", "adobe desktop service.exe", "ccxprocess.exe",
    "adobeipcbroker.exe", "acrotray.exe", "adobecollabsync.exe",
];

/// Offered but unticked: they may hold work the user needs right now.
fn keep_note(exe: &str) -> Option<&'static str> {
    const BROWSERS: &[&str] = &["chrome.exe", "msedge.exe", "brave.exe", "firefox.exe", "opera.exe", "vivaldi.exe", "arc.exe", "helium.exe", "chromium.exe"];
    const TERMINALS: &[&str] = &["windowsterminal.exe", "wt.exe", "cmd.exe", "powershell.exe", "pwsh.exe", "wezterm-gui.exe", "alacritty.exe", "mintty.exe"];
    const EDITORS: &[&str] = &["code.exe", "cursor.exe", "windsurf.exe", "devenv.exe", "idea64.exe", "pycharm64.exe", "webstorm64.exe", "rider64.exe", "zed.exe", "sublime_text.exe", "notepad++.exe"];
    if BROWSERS.contains(&exe) {
        Some("A browser: Vivianne may be using it, and it holds your sign-ins")
    } else if TERMINALS.contains(&exe) {
        Some("A terminal: it may be running something you need")
    } else if EDITORS.contains(&exe) {
        Some("An editor: it may have unsaved work")
    } else if exe == "claude.exe" {
        Some("The Claude app: closing it ends its sessions")
    } else {
        None
    }
}

fn state_path() -> Option<std::path::PathBuf> {
    crate::agent::paths::viv_home().ok().map(|h| h.join("supercharge.json"))
}

fn load_state() -> SuperchargeState {
    state_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_state(state: &SuperchargeState) {
    if let (Some(p), Ok(json)) = (state_path(), serde_json::to_string_pretty(state)) {
        let _ = std::fs::write(p, json);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// "Microsoft Word" from a window title like "Report.docx - Word" is not
/// reliable, so the name is the exe's, tidied: "WINWORD.EXE" → "Winword".
fn friendly(exe_path: &str) -> String {
    let file = exe_path.rsplit(['\\', '/']).next().unwrap_or(exe_path);
    let stem = file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file);
    let mut c = stem.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => stem.to_string(),
    }
}

#[cfg(windows)]
mod win {
    use std::collections::{HashMap, HashSet};

    use windows::core::{BOOL, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, SetPriorityClass,
        TerminateProcess, ABOVE_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE, PROCESS_VM_READ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE};

    pub struct Proc {
        pub pid: u32,
        pub parent: u32,
        pub file: String,
    }

    pub fn processes() -> Vec<Proc> {
        let mut out = Vec::new();
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
            let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            if Process32FirstW(snap, &mut e).is_ok() {
                loop {
                    let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                    out.push(Proc { pid: e.th32ProcessID, parent: e.th32ParentProcessID, file: String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase() });
                    if Process32NextW(snap, &mut e).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
        }
        out
    }

    pub fn exe_path(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(h);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    pub fn memory_mb(pid: u32) -> u64 {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid) else { return 0 };
            let mut c = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
            let ok = GetProcessMemoryInfo(h, &mut c, c.cb).is_ok();
            let _ = CloseHandle(h);
            if ok { (c.WorkingSetSize / (1024 * 1024)) as u64 } else { 0 }
        }
    }

    pub fn alive(pid: u32) -> bool {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code).is_ok();
            let _ = CloseHandle(h);
            ok && code == 259 // STILL_ACTIVE
        }
    }

    pub fn terminate(pid: u32) {
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(h, 0);
                let _ = CloseHandle(h);
            }
        }
    }

    unsafe extern "system" fn collect_all(h: HWND, lparam: LPARAM) -> BOOL {
        let out = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };
        out.push(h.0 as isize);
        BOOL(1)
    }

    /// Ask every top-level window of these processes to close, hidden ones
    /// included (that's how tray apps receive it).
    pub fn ask_to_close(pids: &HashSet<u32>) {
        let mut all: Vec<isize> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(collect_all), LPARAM(&mut all as *mut Vec<isize> as isize));
        }
        for h in all {
            let hwnd = HWND(h as *mut core::ffi::c_void);
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pids.contains(&pid) {
                let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
        }
    }

    /// Vivianne's own process tree (it and everything it started) and the
    /// chain of processes that launched it.
    pub fn protected(procs: &[Proc]) -> HashSet<u32> {
        let me = unsafe { GetCurrentProcessId() };
        let parent_of: HashMap<u32, u32> = procs.iter().map(|p| (p.pid, p.parent)).collect();
        let mut keep: HashSet<u32> = HashSet::from([me]);
        // Descendants.
        loop {
            let before = keep.len();
            for p in procs {
                if keep.contains(&p.parent) {
                    keep.insert(p.pid);
                }
            }
            if keep.len() == before {
                break;
            }
        }
        // Ancestors.
        let mut cur = me;
        for _ in 0..32 {
            match parent_of.get(&cur) {
                Some(&p) if p != 0 && keep.insert(p) => cur = p,
                _ => break,
            }
        }
        keep
    }

    pub fn boost(on: bool) {
        unsafe {
            let _ = SetPriorityClass(GetCurrentProcess(), if on { ABOVE_NORMAL_PRIORITY_CLASS } else { NORMAL_PRIORITY_CLASS });
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn powercfg(args: &[&str]) -> Result<String, String> {
    let mut cmd = std::process::Command::new("powercfg");
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

/// The GUID in `powercfg /getactivescheme`'s "Power Scheme GUID: <guid>  (Balanced)".
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_scheme(text: &str) -> Option<String> {
    text.split_whitespace().find(|w| w.len() == 36 && w.chars().filter(|c| *c == '-').count() == 4).map(str::to_lowercase)
}

#[cfg(windows)]
pub fn scan() -> Vec<Candidate> {
    use std::collections::{BTreeMap, HashSet};
    let procs = win::processes();
    let protected = win::protected(&procs);
    let windows = crate::coding_agent::win::list_windows();
    let windowed: HashSet<u32> = windows.iter().map(|w| w.pid).collect();

    struct Group {
        exe: String,
        pids: Vec<u32>,
        title: Option<String>,
        windowed: bool,
    }
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for p in &procs {
        if protected.contains(&p.pid) || SYSTEM.contains(&p.file.as_str()) {
            continue;
        }
        let has_window = windowed.contains(&p.pid);
        if !has_window && !BACKGROUND.contains(&p.file.as_str()) {
            continue;
        }
        // Not ours to close: elevated, or another user's.
        let Some(exe) = win::exe_path(p.pid) else { continue };
        if exe.to_lowercase().starts_with(r"c:\windows\") {
            continue;
        }
        let g = groups.entry(exe.to_lowercase()).or_insert_with(|| Group { exe: exe.clone(), pids: Vec::new(), title: None, windowed: false });
        g.pids.push(p.pid);
        if has_window {
            g.windowed = true;
            if g.title.is_none() {
                g.title = windows.iter().find(|w| w.pid == p.pid).map(|w| w.title.clone());
            }
        }
    }
    // A launcher's helpers (steamwebhelper under steam) belong with it.
    let mut out: Vec<Candidate> = groups
        .into_iter()
        .map(|(id, g)| {
            let file = id.rsplit(['\\', '/']).next().unwrap_or(&id).to_string();
            let note = keep_note(&file).map(String::from);
            Candidate {
                name: friendly(&g.exe),
                exe: g.exe,
                title: g.title,
                background: !g.windowed,
                processes: g.pids.len(),
                memory_mb: g.pids.iter().map(|&p| win::memory_mb(p)).sum(),
                recommended: note.is_none(),
                note,
                id,
            }
        })
        .collect();
    out.sort_by(|a, b| b.recommended.cmp(&a.recommended).then(b.memory_mb.cmp(&a.memory_mb)));
    out
}

/// macOS: never offered — the shell, and apps that are part of the system.
#[cfg(target_os = "macos")]
const MAC_SYSTEM: &[&str] = &[
    "com.apple.finder", "com.apple.dock", "com.apple.systemuiserver", "com.apple.loginwindow", "com.apple.controlcenter",
    "com.apple.notificationcenterui", "com.apple.spotlight", "com.apple.siri", "com.apple.textinputmenuagent",
    "com.apple.windowmanager", "com.apple.wallpaper.agent", "com.apple.systempreferences", "com.apple.systemsettings",
];

/// macOS: background clients worth offering (matched on bundle id).
#[cfg(target_os = "macos")]
const MAC_BACKGROUND: &[&str] = &[
    "com.getdropbox.dropbox", "com.google.drivefs", "com.microsoft.onedrive", "com.valvesoftware.steam",
    "com.epicgames.epicgameslauncher", "com.hnc.discord", "com.spotify.client", "com.microsoft.teams", "com.microsoft.teams2",
    "com.tinyspeck.slackmacgap", "net.whatsapp.whatsapp", "ru.keepcoder.telegram", "us.zoom.xos", "com.adobe.acc.adobecreativecloud",
    "com.adobe.ccxprocess", "com.blizzard.battle.net",
];

/// macOS: offered but unticked.
#[cfg(target_os = "macos")]
fn mac_keep_note(bundle_id: &str) -> Option<&'static str> {
    const BROWSERS: &[&str] = &["com.apple.safari", "com.google.chrome", "org.mozilla.firefox", "company.thebrowser.browser", "com.brave.browser", "com.microsoft.edgemac", "com.vivaldi.vivaldi", "com.operasoftware.opera", "net.imput.helium", "org.chromium.chromium"];
    const TERMINALS: &[&str] = &["com.apple.terminal", "com.googlecode.iterm2", "dev.warp.warp-stable", "com.mitchellh.ghostty", "net.kovidgoyal.kitty", "org.alacritty", "com.github.wez.wezterm"];
    const EDITORS: &[&str] = &["com.microsoft.vscode", "com.todesktop.230313mzl4w4u92", "com.exafunction.windsurf", "com.apple.dt.xcode", "dev.zed.zed", "com.sublimetext.4", "com.jetbrains.intellij", "com.jetbrains.pycharm", "com.jetbrains.webstorm"];
    if BROWSERS.contains(&bundle_id) {
        Some("A browser: Vivianne may be using it, and it holds your sign-ins")
    } else if TERMINALS.contains(&bundle_id) {
        Some("A terminal: it may be running something you need")
    } else if EDITORS.contains(&bundle_id) {
        Some("An editor: it may have unsaved work")
    } else if bundle_id == "com.anthropic.claudefordesktop" {
        Some("The Claude app: closing it ends its sessions")
    } else {
        None
    }
}

/// macOS: resident memory of a process, from `ps` (no entitlement needed).
#[cfg(target_os = "macos")]
fn mac_memory_mb(pid: i32) -> u64 {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok())
        .map(|kb| kb / 1024)
        .unwrap_or(0)
}

/// macOS: the app that launched Vivianne (a terminal running a dev build),
/// which must not be offered.
#[cfg(target_os = "macos")]
fn mac_launcher_pids() -> std::collections::HashSet<i32> {
    let mut out = std::collections::HashSet::new();
    let mut cur = std::process::id() as i32;
    for _ in 0..16 {
        let parent = std::process::Command::new("ps")
            .args(["-o", "ppid=", "-p", &cur.to_string()])
            .output()
            .ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<i32>().ok());
        match parent {
            Some(p) if p > 1 && out.insert(p) => cur = p,
            _ => break,
        }
    }
    out
}

#[cfg(target_os = "macos")]
pub fn scan() -> Vec<Candidate> {
    use crate::coding_agent::mac;
    let launchers = mac_launcher_pids();
    let mut out: Vec<Candidate> = mac::all_apps()
        .into_iter()
        .filter(|a| !launchers.contains(&a.pid))
        .filter(|a| {
            let id = a.bundle_id.to_lowercase();
            !MAC_SYSTEM.contains(&id.as_str()) && (!a.background || MAC_BACKGROUND.contains(&id.as_str()))
        })
        .map(|a| {
            let id = a.bundle_id.to_lowercase();
            let note = mac_keep_note(&id).map(String::from);
            Candidate {
                id: if id.is_empty() { a.bundle_path.to_lowercase() } else { id },
                name: a.name.clone(),
                exe: a.bundle_path.clone(),
                title: None,
                background: a.background,
                processes: 1,
                memory_mb: mac_memory_mb(a.pid),
                recommended: note.is_none(),
                note,
            }
        })
        .collect();
    out.sort_by(|a, b| b.recommended.cmp(&a.recommended).then(b.memory_mb.cmp(&a.memory_mb)));
    out
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn scan() -> Vec<Candidate> {
    Vec::new()
}

/// macOS: the `caffeinate` that keeps the Mac awake while Supercharge is on.
#[cfg(target_os = "macos")]
static CAFFEINATE: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);

#[cfg(windows)]
pub async fn apply(ids: Vec<String>) -> Result<ApplyReport, String> {
    use std::collections::HashSet;
    let wanted: HashSet<String> = ids.into_iter().map(|s| s.to_lowercase()).collect();
    // Re-scanned, not trusted from the caller: only what is offered can be closed.
    let offered: Vec<Candidate> = scan().into_iter().filter(|c| wanted.contains(&c.id)).collect();
    let procs = win::processes();
    let protected = win::protected(&procs);
    let mut targets: Vec<(Candidate, HashSet<u32>)> = Vec::new();
    for c in offered {
        let pids: HashSet<u32> = procs
            .iter()
            .filter(|p| !protected.contains(&p.pid))
            .filter(|p| win::exe_path(p.pid).is_some_and(|e| e.to_lowercase() == c.id))
            .map(|p| p.pid)
            .collect();
        if !pids.is_empty() {
            targets.push((c, pids));
        }
    }
    let all: HashSet<u32> = targets.iter().flat_map(|(_, p)| p.iter().copied()).collect();
    win::ask_to_close(&all);

    // Give them time to close, and to put up "save changes?" if they need to.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    while std::time::Instant::now() < deadline && all.iter().any(|&p| win::alive(p)) {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    let mut closed = Vec::new();
    let mut still_open = Vec::new();
    let mut freed = 0u64;
    let mut state = load_state();
    for (c, pids) in &targets {
        let mut left: Vec<u32> = pids.iter().copied().filter(|&p| win::alive(p)).collect();
        // Background clients often ignore the request; the user ticked them.
        if !left.is_empty() && c.background {
            for &p in &left {
                win::terminate(p);
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            left.retain(|&p| win::alive(p));
        }
        if left.is_empty() {
            freed += c.memory_mb;
            closed.push(c.name.clone());
            if !state.closed.iter().any(|a| a.exe.eq_ignore_ascii_case(&c.exe)) {
                state.closed.push(ClosedApp { name: c.name.clone(), exe: c.exe.clone() });
            }
        } else {
            still_open.push(c.name.clone());
        }
    }

    win::boost(true);
    let power_plan = match powercfg(&["/getactivescheme"]).ok().and_then(|t| parse_scheme(&t)) {
        Some(current) => {
            if !state.on || state.previous_power_plan.is_none() {
                state.previous_power_plan = Some(current.clone());
            }
            if current == HIGH_PERFORMANCE {
                "Already on High performance".to_string()
            } else {
                match powercfg(&["/setactive", HIGH_PERFORMANCE]) {
                    Ok(_) => "Switched to High performance".to_string(),
                    Err(_) => "This PC has no High performance plan, so the power plan stays as it is".to_string(),
                }
            }
        }
        None => "Couldn't read the power plan; left as it is".to_string(),
    };
    state.on = true;
    state.since.get_or_insert(now_ms());
    save_state(&state);
    Ok(ApplyReport { closed, still_open, power_plan, freed_mb: freed, state })
}

#[cfg(target_os = "macos")]
pub async fn apply(ids: Vec<String>) -> Result<ApplyReport, String> {
    use crate::coding_agent::mac;
    use std::collections::HashSet;
    let wanted: HashSet<String> = ids.into_iter().map(|s| s.to_lowercase()).collect();
    // Re-scanned, not trusted from the caller: only what is offered can be closed.
    let offered: Vec<Candidate> = scan().into_iter().filter(|c| wanted.contains(&c.id)).collect();
    let apps = mac::all_apps();
    let targets: Vec<(Candidate, i32)> = offered
        .into_iter()
        .filter_map(|c| {
            let pid = apps.iter().find(|a| a.bundle_id.to_lowercase() == c.id || a.bundle_path.to_lowercase() == c.id)?.pid;
            Some((c, pid))
        })
        .collect();
    for (_, pid) in &targets {
        mac::quit(*pid);
    }
    // Time to quit, and to ask "save changes?" if they need to.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    while std::time::Instant::now() < deadline && targets.iter().any(|(_, p)| mac::is_running(*p)) {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    let mut closed = Vec::new();
    let mut still_open = Vec::new();
    let mut freed = 0u64;
    let mut state = load_state();
    for (c, pid) in &targets {
        // Background clients often ignore a polite quit; the user ticked them.
        if mac::is_running(*pid) && c.background {
            mac::force_quit(*pid);
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        if mac::is_running(*pid) {
            still_open.push(c.name.clone());
        } else {
            freed += c.memory_mb;
            closed.push(c.name.clone());
            if !c.exe.is_empty() && !state.closed.iter().any(|a| a.exe == c.exe) {
                state.closed.push(ClosedApp { name: c.name.clone(), exe: c.exe.clone() });
            }
        }
    }
    // Keep the Mac awake for as long as Vivianne runs (or until turned off).
    let power_plan = {
        let mut guard = CAFFEINATE.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = std::process::Command::new("caffeinate").args(["-dimsu", "-w", &std::process::id().to_string()]).spawn().ok();
        }
        if guard.is_some() { "Keeping the Mac awake while Supercharge is on".to_string() } else { "Couldn't keep the Mac awake".to_string() }
    };
    state.on = true;
    state.since.get_or_insert(now_ms());
    save_state(&state);
    Ok(ApplyReport { closed, still_open, power_plan, freed_mb: freed, state })
}

#[cfg(not(any(windows, target_os = "macos")))]
pub async fn apply(_ids: Vec<String>) -> Result<ApplyReport, String> {
    Err("Supercharge is available on Windows and macOS.".into())
}

/// Turn it off: power plan and priority back, and optionally reopen what it
/// closed. Returns the apps it couldn't reopen.
pub fn restore(reopen: bool) -> Vec<String> {
    let state = load_state();
    #[cfg(windows)]
    if let Some(plan) = &state.previous_power_plan {
        let _ = powercfg(&["/setactive", plan]);
    }
    #[cfg(windows)]
    win::boost(false);
    #[cfg(target_os = "macos")]
    if let Some(mut c) = CAFFEINATE.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    let mut failed = Vec::new();
    if reopen {
        for app in &state.closed {
            // Store apps can't be started from their exe path.
            #[cfg(not(target_os = "macos"))]
            let store_app = app.exe.to_lowercase().contains(r"\windowsapps\");
            // macOS: an app is reopened from its bundle, the way Finder does.
            #[cfg(target_os = "macos")]
            let started = std::process::Command::new("open").arg(&app.exe).spawn().is_ok();
            #[cfg(not(target_os = "macos"))]
            let started = !store_app && {
                let mut cmd = std::process::Command::new(&app.exe);
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(0x0000_0008); // DETACHED_PROCESS
                }
                cmd.spawn().is_ok()
            };
            if !started {
                failed.push(app.name.clone());
            }
        }
    }
    save_state(&SuperchargeState::default());
    failed
}

#[tauri::command]
pub async fn cmd_supercharge_status() -> Result<SuperchargeState, String> {
    Ok(load_state())
}

#[tauri::command]
pub async fn cmd_supercharge_scan() -> Result<Vec<Candidate>, String> {
    tauri::async_runtime::spawn_blocking(scan).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_supercharge_apply(ids: Vec<String>) -> Result<ApplyReport, String> {
    apply(ids).await
}

#[tauri::command]
pub async fn cmd_supercharge_restore(reopen: bool) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || restore(reopen)).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_scheme_guid() {
        let out = "Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)";
        assert_eq!(parse_scheme(out).as_deref(), Some("381b4222-f694-41f0-9685-ff5bb260df2e"));
        assert_eq!(parse_scheme("nothing here"), None);
    }

    #[test]
    fn keeps_what_may_hold_work_unticked() {
        assert!(keep_note("chrome.exe").is_some());
        assert!(keep_note("windowsterminal.exe").is_some());
        assert!(keep_note("code.exe").is_some());
        assert!(keep_note("claude.exe").is_some());
        assert!(keep_note("spotify.exe").is_none());
        assert!(SYSTEM.contains(&"explorer.exe"));
        assert_eq!(friendly(r"C:\Program Files\Spotify\spotify.exe"), "Spotify");
    }

    /// Never offers Vivianne itself, or what launched it.
    #[cfg(windows)]
    #[test]
    fn never_offers_itself_or_its_launcher() {
        let procs = win::processes();
        let protected = win::protected(&procs);
        let me = std::process::id();
        assert!(protected.contains(&me));
        let parent = procs.iter().find(|p| p.pid == me).map(|p| p.parent).unwrap();
        assert!(protected.contains(&parent));
        let offered = scan();
        let my_exe = std::env::current_exe().unwrap().display().to_string().to_lowercase();
        assert!(offered.iter().all(|c| c.id != my_exe));
    }
}
