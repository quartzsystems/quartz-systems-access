// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A small append-only log, `qsaccess.log` in the settings directory, also
//! echoed to standard error. Callers must never pass a password; the types
//! that hold one ([`qsa_config::Secret`]) print as `***`.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Log file name.
pub const LOG_FILE: &str = "qsaccess.log";

/// Sends log lines to `dir/qsaccess.log` from now on.
pub fn init(dir: Option<PathBuf>) {
    if let Ok(mut f) = FILE.lock() {
        *f = dir.map(|d| d.join(LOG_FILE));
    }
}

/// Writes one line.
pub fn line(text: &str) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = format!("{secs} [{}] {text}\n", std::process::id());
    eprint!("{entry}");
    let path = FILE.lock().ok().and_then(|f| f.clone());
    if let Some(path) = path {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = f.write_all(entry.as_bytes());
        }
    }
}

/// `format!`-style logging.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        $crate::log::line(&format!($($arg)*))
    };
}
