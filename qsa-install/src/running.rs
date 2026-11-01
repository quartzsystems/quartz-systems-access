// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A running QS/400 Access for Desktop, found and closed through its
//! single-instance endpoint (see `qsaccess::ipc`).

use std::time::{Duration, Instant};

use qsaccess::ipc::Endpoint;

fn endpoint() -> Option<Endpoint> {
    let dir = qsa_config::ConfigDir::default_location().map(|d| d.path().to_owned());
    Endpoint::for_user(dir.as_deref()).ok()
}

/// Whether QS/400 Access for Desktop is running for this user.
pub fn is_running() -> bool {
    endpoint().is_some_and(|e| e.is_running())
}

/// Asks the running instance to close, and waits up to 20 seconds for it
/// to go. True once it has.
pub fn close() -> bool {
    let Some(ep) = endpoint() else { return true };
    if !ep.is_running() {
        return true;
    }
    crate::log("asking the running QS/400 Access for Desktop to close");
    if ep.send(&["--quit".to_owned()]).is_err() {
        return false;
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(200));
        if !ep.is_running() {
            // The endpoint goes as the process starts to exit; give it a
            // moment to let go of its files.
            std::thread::sleep(Duration::from_millis(1500));
            return true;
        }
    }
    crate::log("QS/400 Access for Desktop did not close");
    false
}
