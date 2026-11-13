//! Starting other programs without a console window flashing up.
//!
//! On Windows a console program started from a windowed app gets a console
//! of its own unless told otherwise, so every `git status` behind a panel
//! blinked a terminal onto the screen. Everything that runs a program in the
//! background starts it through here. Programs meant to be seen (a terminal
//! the user asked for, an app being opened) use `Command::new` directly.

use std::ffi::OsStr;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A `tokio` command that opens no window.
pub fn quiet(program: impl AsRef<OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// A blocking command that opens no window.
pub fn quiet_std(program: impl AsRef<OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
