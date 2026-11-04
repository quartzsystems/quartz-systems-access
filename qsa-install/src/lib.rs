// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The QS/400 Access for Desktop installer.
//!
//! - [`payload`]: the files packed into the installer, and their checksum.
//! - [`manifest`]: `install-manifest.toml`, everything an install created.
//! - [`plan`]: where things go, for just me or for all users.
//! - [`install`]: install, upgrade, repair and uninstall.
//! - `windows` / `linux`: registry, shortcuts and file types; desktop
//!   entries, MIME types and links.
//! - [`cli`]: the command line; [`ui`]: the wizard and dialogs;
//!   [`running`]: closing a running QS/400 Access for Desktop.
//!
//! The same program is the installer (with its payload appended), the
//! uninstaller and the repairer (a copy kept in the install folder).

pub mod cli;
pub mod install;
pub mod manifest;
pub mod payload;
pub mod plan;
pub mod running;
pub mod ui;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;

/// Product name.
pub const PRODUCT: &str = "QS/400 Access for Desktop";
/// Version installed: VERSION's, through the workspace version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Release installed, VxRyMz from the version.
pub const RELEASE: &str = qsaccess::VERSION;
/// Publisher.
pub const PUBLISHER: &str = "Quartz Systems";

/// Exit codes of the installer, uninstaller and repairer. Documented in
/// the README.
pub mod exit {
    /// Done.
    pub const OK: i32 = 0;
    /// The arguments were not understood.
    pub const USAGE: i32 = 1;
    /// The payload is damaged (`QAD2103`).
    pub const DAMAGED: i32 = 2;
    /// Installing for all users needs administrator (root) rights
    /// (`QAD2104`).
    pub const NEEDS_ADMIN: i32 = 3;
    /// Cancelled by the user.
    pub const CANCELLED: i32 = 4;
    /// Something could not be written or removed; see the log.
    pub const FAILED: i32 = 5;
    /// QS/400 Access for Desktop is running and was not closed.
    pub const RUNNING: i32 = 6;
    /// Uninstall or repair found no install.
    pub const NOT_INSTALLED: i32 = 7;
}

/// The name of the uninstaller copy kept in the install folder.
pub fn uninstaller_name() -> &'static str {
    if cfg!(windows) {
        "uninstall.exe"
    } else {
        "quartz-access-uninstall"
    }
}

/// The name of the program in the install folder.
pub fn program_name() -> &'static str {
    if cfg!(windows) {
        "qsaccess.exe"
    } else {
        "qsaccess"
    }
}

/// A line for the installer's log (standard error, and the log file the
/// caller set up).
pub fn log(line: &str) {
    eprintln!("{line}");
    if let Some(f) = LOG.get()
        && let Ok(mut f) = f.lock()
    {
        use std::io::Write;
        let _ = writeln!(f, "{line}");
    }
}

static LOG: std::sync::OnceLock<std::sync::Mutex<std::fs::File>> = std::sync::OnceLock::new();

/// Also writes log lines to `path`.
pub fn log_to(path: &std::path::Path) {
    if let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = LOG.set(std::sync::Mutex::new(f));
    }
}
