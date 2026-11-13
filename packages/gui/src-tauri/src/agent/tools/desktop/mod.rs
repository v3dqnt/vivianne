//! Tools that drive the desktop: computer use (`computer`), DIP's UI
//! Automation on Windows and Accessibility on macOS, and on-screen OCR.

pub mod computer;
pub mod consent;
pub mod ocr;
#[cfg(target_os = "windows")]
pub mod dip;
#[cfg(target_os = "windows")]
pub mod win;
#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(target_os = "macos")]
pub mod dip_mac;

/// Stop any typing or key presses in progress (the activity notch's Stop).
pub fn win_cancel() {
    #[cfg(target_os = "windows")]
    win::cancel();
}

/// A new action starts un-cancelled.
pub fn win_cancel_reset() {
    #[cfg(target_os = "windows")]
    win::cancel_reset();
}
