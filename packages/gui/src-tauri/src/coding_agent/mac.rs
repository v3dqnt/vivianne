//! macOS primitives for `dip`: the Accessibility API, apps and windows.
//!
//! The counterpart of `super::win`. Structured control on macOS is the
//! Accessibility API (`AXUIElement`), the same interface VoiceOver uses: every
//! app's windows and controls as a tree of elements with roles, titles, values
//! and actions. It needs the user to allow Vivianne under System Settings →
//! Privacy & Security → Accessibility (`trusted`, `request_trust`).
//!
//! The AX functions are declared here directly against the ApplicationServices
//! framework: their C signatures have been stable since 10.2, and holding the
//! declarations here keeps them checkable against Apple's headers. Apps come
//! from `NSWorkspace` (through objc2).
//!
//! Windows are addressed by AX, not by CGWindowList: CGWindowList needs the
//! Screen Recording permission to so much as read a window's title, while AX
//! already has everything `dip` needs.

#![allow(non_upper_case_globals, unused_unsafe)]

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFGetTypeID, CFRelease, CFRetain, CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use std::ffi::c_void;

pub type AXUIElementRef = *const c_void;
type AXValueRef = *const c_void;
type AXError = i32;
const kAXErrorSuccess: AXError = 0;
const kAXValueCGPointType: u32 = 1;
const kAXValueCGSizeType: u32 = 2;

#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Default, Clone, Copy, Debug)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(element: AXUIElementRef, attribute: CFStringRef, value: *mut CFTypeRef) -> AXError;
    fn AXUIElementCopyMultipleAttributeValues(element: AXUIElementRef, attributes: CFArrayRef, options: u32, values: *mut CFArrayRef) -> AXError;
    fn AXUIElementSetAttributeValue(element: AXUIElementRef, attribute: CFStringRef, value: CFTypeRef) -> AXError;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    fn AXUIElementCopyActionNames(element: AXUIElementRef, names: *mut CFArrayRef) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: f32) -> AXError;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> AXError;
    fn AXUIElementGetTypeID() -> usize;
    fn AXValueGetValue(value: AXValueRef, the_type: u32, value_ptr: *mut c_void) -> u8;
    fn AXValueCreate(the_type: u32, value_ptr: *const c_void) -> AXValueRef;
    fn AXValueGetTypeID() -> usize;
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    // Both 10.15+, which is Vivianne's floor.
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

// ── Permissions ──

/// Whether Vivianne may use the Accessibility API.
pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Ask macOS to show the Accessibility prompt (it opens System Settings with
/// Vivianne listed). Returns whether it is already trusted.
pub fn request_trust() -> bool {
    let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
    let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_CFTypeRef()) != 0 }
}

pub fn screen_capture_allowed() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

pub fn request_screen_capture() -> bool {
    unsafe { CGRequestScreenCaptureAccess() }
}

pub const NOT_TRUSTED: &str = "Vivianne isn't allowed to control apps yet. Open System Settings → Privacy & Security → Accessibility and switch Vivianne on (Settings → Viv Instant → Mac permissions has a button for it), then try again.";

// ── Elements ──

/// An owned, retained `AXUIElementRef`.
pub struct Element(AXUIElementRef);

unsafe impl Send for Element {}

impl Element {
    /// Takes ownership of a +1 reference.
    unsafe fn owned(r: AXUIElementRef) -> Option<Self> {
        (!r.is_null()).then(|| Self(r))
    }

    pub fn application(pid: i32) -> Option<Self> {
        let e = unsafe { Self::owned(AXUIElementCreateApplication(pid))? };
        // An unresponsive app otherwise blocks every call for the default 6s.
        unsafe { AXUIElementSetMessagingTimeout(e.0, 2.0) };
        Some(e)
    }

    pub fn system_wide() -> Option<Self> {
        unsafe { Self::owned(AXUIElementCreateSystemWide()) }
    }

    pub fn pid(&self) -> Option<i32> {
        let mut pid = 0;
        (unsafe { AXUIElementGetPid(self.0, &mut pid) } == kAXErrorSuccess).then_some(pid)
    }

    pub fn raw(&self) -> AXUIElementRef {
        self.0
    }

    pub fn attr(&self, name: &str) -> Option<CFType> {
        let mut v: CFTypeRef = std::ptr::null();
        let name = CFString::new(name);
        let err = unsafe { AXUIElementCopyAttributeValue(self.0, name.as_concrete_TypeRef(), &mut v) };
        (err == kAXErrorSuccess && !v.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(v) })
    }

    pub fn string(&self, name: &str) -> String {
        self.attr(name).map(|v| cf_to_string(&v)).unwrap_or_default()
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        self.attr(name).and_then(|v| v.downcast::<CFBoolean>()).map(bool::from)
    }

    pub fn element(&self, name: &str) -> Option<Element> {
        let v = self.attr(name)?;
        if unsafe { CFGetTypeID(v.as_CFTypeRef()) } != unsafe { AXUIElementGetTypeID() } {
            return None;
        }
        unsafe { CFRetain(v.as_CFTypeRef()) };
        unsafe { Self::owned(v.as_CFTypeRef()) }
    }

    pub fn elements(&self, name: &str) -> Vec<Element> {
        let Some(v) = self.attr(name) else { return Vec::new() };
        let Some(arr) = v.downcast::<CFArray>() else { return Vec::new() };
        arr.iter()
            .filter_map(|item| {
                let r = *item as AXUIElementRef;
                if r.is_null() || unsafe { CFGetTypeID(r) } != unsafe { AXUIElementGetTypeID() } {
                    return None;
                }
                unsafe { CFRetain(r) };
                unsafe { Self::owned(r) }
            })
            .collect()
    }

    /// Several attributes in one cross-process round trip. Missing ones come
    /// back as `None`.
    pub fn attrs(&self, names: &[&str]) -> Vec<Option<CFType>> {
        let keys: Vec<CFString> = names.iter().map(|n| CFString::new(n)).collect();
        let arr = CFArray::from_CFTypes(&keys);
        let mut out: CFArrayRef = std::ptr::null();
        let err = unsafe { AXUIElementCopyMultipleAttributeValues(self.0, arr.as_concrete_TypeRef(), 0, &mut out) };
        if err != kAXErrorSuccess || out.is_null() {
            return names.iter().map(|n| self.attr(n)).collect();
        }
        let values: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(out) };
        let error_type = unsafe { AXValueGetTypeID() };
        (0..names.len())
            .map(|i| {
                let v = values.get(i as isize)?;
                // Errors come back as AXValues of type kAXValueAXErrorType (5).
                if unsafe { CFGetTypeID(v.as_CFTypeRef()) } == error_type && ax_value_type(&v) == Some(5) {
                    return None;
                }
                Some(v.clone())
            })
            .collect()
    }

    pub fn position(&self) -> Option<CGPoint> {
        let v = self.attr("AXPosition")?;
        let mut p = CGPoint::default();
        (unsafe { AXValueGetValue(v.as_CFTypeRef(), kAXValueCGPointType, &mut p as *mut _ as *mut c_void) } != 0).then_some(p)
    }

    pub fn size(&self) -> Option<CGSize> {
        let v = self.attr("AXSize")?;
        let mut s = CGSize::default();
        (unsafe { AXValueGetValue(v.as_CFTypeRef(), kAXValueCGSizeType, &mut s as *mut _ as *mut c_void) } != 0).then_some(s)
    }

    pub fn set_position(&self, p: CGPoint) -> bool {
        unsafe {
            let v = AXValueCreate(kAXValueCGPointType, &p as *const _ as *const c_void);
            if v.is_null() {
                return false;
            }
            let r = AXUIElementSetAttributeValue(self.0, CFString::new("AXPosition").as_concrete_TypeRef(), v) == kAXErrorSuccess;
            CFRelease(v);
            r
        }
    }

    pub fn set_size(&self, s: CGSize) -> bool {
        unsafe {
            let v = AXValueCreate(kAXValueCGSizeType, &s as *const _ as *const c_void);
            if v.is_null() {
                return false;
            }
            let r = AXUIElementSetAttributeValue(self.0, CFString::new("AXSize").as_concrete_TypeRef(), v) == kAXErrorSuccess;
            CFRelease(v);
            r
        }
    }

    pub fn set_string(&self, name: &str, value: &str) -> bool {
        let v = CFString::new(value);
        unsafe { AXUIElementSetAttributeValue(self.0, CFString::new(name).as_concrete_TypeRef(), v.as_CFTypeRef()) == kAXErrorSuccess }
    }

    pub fn set_bool(&self, name: &str, value: bool) -> bool {
        let v = if value { CFBoolean::true_value() } else { CFBoolean::false_value() };
        unsafe { AXUIElementSetAttributeValue(self.0, CFString::new(name).as_concrete_TypeRef(), v.as_CFTypeRef()) == kAXErrorSuccess }
    }

    pub fn perform(&self, action: &str) -> bool {
        unsafe { AXUIElementPerformAction(self.0, CFString::new(action).as_concrete_TypeRef()) == kAXErrorSuccess }
    }

    pub fn actions(&self) -> Vec<String> {
        let mut out: CFArrayRef = std::ptr::null();
        if unsafe { AXUIElementCopyActionNames(self.0, &mut out) } != kAXErrorSuccess || out.is_null() {
            return Vec::new();
        }
        let arr: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(out) };
        arr.iter().map(|v| cf_to_string(&v)).collect()
    }
}

impl Clone for Element {
    fn clone(&self) -> Self {
        unsafe { CFRetain(self.0) };
        Self(self.0)
    }
}

impl Drop for Element {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

fn ax_value_type(v: &CFType) -> Option<u32> {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXValueGetType(value: AXValueRef) -> u32;
    }
    Some(unsafe { AXValueGetType(v.as_CFTypeRef()) })
}

/// A CF value as text: strings as is, numbers and booleans formatted.
pub fn cf_to_string(v: &CFType) -> String {
    if let Some(s) = v.downcast::<CFString>() {
        return s.to_string();
    }
    if let Some(n) = v.downcast::<CFNumber>() {
        return n.to_f64().map(|f| if f.fract() == 0.0 { format!("{}", f as i64) } else { format!("{f}") }).unwrap_or_default();
    }
    if let Some(b) = v.downcast::<CFBoolean>() {
        return bool::from(b).to_string();
    }
    String::new()
}

/// Read a point/size AXValue out of a multi-attribute result.
pub fn ax_point(v: &CFType) -> Option<CGPoint> {
    let mut p = CGPoint::default();
    (unsafe { AXValueGetValue(v.as_CFTypeRef(), kAXValueCGPointType, &mut p as *mut _ as *mut c_void) } != 0).then_some(p)
}
pub fn ax_size(v: &CFType) -> Option<CGSize> {
    let mut s = CGSize::default();
    (unsafe { AXValueGetValue(v.as_CFTypeRef(), kAXValueCGSizeType, &mut s as *mut _ as *mut c_void) } != 0).then_some(s)
}

// ── Apps ──

#[derive(Debug, Clone)]
pub struct App {
    pub pid: i32,
    pub name: String,
    pub bundle_id: String,
    /// The `.app` it runs from, for reopening it.
    pub bundle_path: String,
    pub active: bool,
    pub hidden: bool,
    /// A menu-bar or background app (no Dock icon): sync clients, launchers.
    pub background: bool,
}

/// Every app macOS lists as running (Dock and menu-bar apps alike), Vivianne
/// excluded. Daemons without a bundle aren't apps and aren't listed.
pub fn all_apps() -> Vec<App> {
    use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};
    let me = std::process::id() as i32;
    let mut out = Vec::new();
    unsafe {
        let ws = NSWorkspace::sharedWorkspace();
        for app in ws.runningApplications().iter() {
            let policy = app.activationPolicy();
            if policy == NSApplicationActivationPolicy::Prohibited {
                continue;
            }
            let pid = app.processIdentifier();
            if pid == me {
                continue;
            }
            out.push(App {
                pid,
                name: app.localizedName().map(|s| s.to_string()).unwrap_or_default(),
                bundle_id: app.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default(),
                bundle_path: app.bundleURL().and_then(|u| u.path()).map(|s| s.to_string()).unwrap_or_default(),
                active: app.isActive(),
                hidden: app.isHidden(),
                background: policy == NSApplicationActivationPolicy::Accessory,
            });
        }
    }
    out
}

/// Regular (Dock) apps that are running, Vivianne excluded.
pub fn running_apps() -> Vec<App> {
    all_apps().into_iter().filter(|a| !a.background).collect()
}

/// Ask an app to quit right away, without the chance to save. Only for
/// background clients the user chose to end.
pub fn force_quit(pid: i32) -> bool {
    use objc2_app_kit::NSRunningApplication;
    unsafe { NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(|a| a.forceTerminate()) }
}

pub fn is_running(pid: i32) -> bool {
    use objc2_app_kit::NSRunningApplication;
    unsafe { NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(|a| !a.isTerminated()) }
}

/// The frontmost app's pid.
pub fn frontmost_pid() -> Option<i32> {
    use objc2_app_kit::NSWorkspace;
    unsafe { NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier()) }
}

/// Bring an app to the front, all its windows.
pub fn activate(pid: i32) -> bool {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    unsafe {
        match NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
            Some(app) => {
                let _ = app.unhide();
                #[allow(deprecated)]
                app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps)
            }
            None => false,
        }
    }
}

/// Ask an app to quit, the way ⌘Q would (it may ask to save).
pub fn quit(pid: i32) -> bool {
    use objc2_app_kit::NSRunningApplication;
    unsafe { NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(|a| a.terminate()) }
}

// ── Windows ──

#[derive(Clone)]
pub struct Window {
    pub pid: i32,
    pub app: String,
    pub title: String,
    pub element: Element,
    pub minimized: bool,
    pub frame: (f64, f64, f64, f64),
}

/// Every standard window of every running app, front app first.
pub fn windows() -> Vec<Window> {
    let mut apps = running_apps();
    apps.sort_by_key(|a| !a.active);
    let mut out = Vec::new();
    for app in apps {
        let Some(ax) = Element::application(app.pid) else { continue };
        for w in ax.elements("AXWindows") {
            let v = w.attrs(&["AXTitle", "AXSubrole", "AXMinimized", "AXPosition", "AXSize"]);
            let s = |i: usize| v.get(i).and_then(|x| x.as_ref()).map(cf_to_string).unwrap_or_default();
            // Panels, sheets and floating palettes aren't windows to act in.
            let subrole = s(1);
            if !subrole.is_empty() && subrole != "AXStandardWindow" && subrole != "AXDialog" {
                continue;
            }
            let p = v.get(3).and_then(|x| x.as_ref()).and_then(ax_point).unwrap_or_default();
            let z = v.get(4).and_then(|x| x.as_ref()).and_then(ax_size).unwrap_or_default();
            out.push(Window {
                pid: app.pid,
                app: app.name.clone(),
                title: s(0),
                minimized: s(2) == "true",
                frame: (p.x, p.y, z.width, z.height),
                element: w,
            });
        }
    }
    out
}

/// Raise one window and its app to the front.
pub fn focus(w: &Window) -> bool {
    if w.minimized {
        w.element.set_bool("AXMinimized", false);
    }
    let raised = w.element.perform("AXRaise");
    activate(w.pid) || raised
}

// ── The target window ──

/// The window the agent is working in: the last one it listed, focused,
/// snapshotted or acted on. Typing and keys go there, not into Vivianne.
static TARGET: std::sync::Mutex<Option<(i32, String)>> = std::sync::Mutex::new(None);

pub fn set_target(pid: i32, title: &str) {
    *TARGET.lock().unwrap_or_else(|e| e.into_inner()) = Some((pid, title.to_string()));
}

pub fn target() -> Option<(i32, String)> {
    TARGET.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Before typing or a key press: if Vivianne is in front, put the target app
/// back in front, and say which app now has the keyboard.
pub fn ensure_target_front() -> Result<String, String> {
    let me = std::process::id() as i32;
    let front = frontmost_pid();
    if front.is_some() && front != Some(me) {
        return Ok(app_name(front.unwrap_or_default()));
    }
    let Some((pid, title)) = target() else {
        return Err("Vivianne is in front and no target app is known yet: list_windows and focus_window the app to type into first.".into());
    };
    if let Some(w) = windows().into_iter().find(|w| w.pid == pid && (title.is_empty() || w.title == title)) {
        focus(&w);
    } else {
        activate(pid);
    }
    std::thread::sleep(std::time::Duration::from_millis(120));
    match frontmost_pid() {
        Some(p) if p == pid => Ok(app_name(pid)),
        _ => Err(format!("Couldn't bring {} to the front to type into it.", app_name(pid))),
    }
}

pub fn app_name(pid: i32) -> String {
    running_apps().into_iter().find(|a| a.pid == pid).map(|a| a.name).unwrap_or_else(|| format!("process {pid}"))
}

/// The focused element anywhere on screen (for reading typed text back).
pub fn focused_element() -> Option<Element> {
    Element::system_wide()?.element("AXFocusedUIElement")
}

#[allow(dead_code)]
fn _assert_types(_: CFType) {}
