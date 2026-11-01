// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Handing files to the desktop: open one in its default application, or
//! show it in the file manager. Failures are logged; the caller goes on.

use std::path::Path;
use std::process::Command;

/// Opens `path` (a file or folder) with the desktop's default application.
pub fn open(path: &Path) {
    let result = imp::open(path);
    if let Err(e) = result {
        crate::log!("{} could not be opened: {e}", path.display());
    }
}

/// Shows `path` selected in the file manager (Linux: its folder).
pub fn show_in_folder(path: &Path) {
    let result = imp::show_in_folder(path);
    if let Err(e) = result {
        crate::log!("{} could not be shown: {e}", path.display());
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;

    use super::*;

    pub fn open(path: &Path) -> std::io::Result<()> {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        // SAFETY: NUL-terminated strings that outlive the call.
        let r = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                wide.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // Values above 32 are success.
        if r as isize > 32 {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "ShellExecute returned {}",
                r as isize
            )))
        }
    }

    pub fn show_in_folder(path: &Path) -> std::io::Result<()> {
        use std::os::windows::process::CommandExt;
        Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .map(|_| ())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn open(path: &Path) -> std::io::Result<()> {
        Command::new("xdg-open").arg(path).spawn().map(|_| ())
    }

    pub fn show_in_folder(path: &Path) -> std::io::Result<()> {
        let dir = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        open(dir)
    }
}
