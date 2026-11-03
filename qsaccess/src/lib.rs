// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QS/400 Access for Desktop: the application's windows and state. The
//! binary in `main.rs` parses arguments and runs the event loop.

pub mod about;
pub mod app;
pub mod chgpwd;
pub mod files;
pub mod ipc;
pub mod localtime;
pub mod log;
pub mod macros;
pub mod mainwin;
pub mod manager;
pub mod open;
pub mod printer;
pub mod prompt;
pub mod properties;
pub mod qap;
pub mod recent;
pub mod request;
pub mod runcmd;
pub mod runsql;
pub mod session;
pub mod shell;
pub mod signon;
pub mod splf;
pub mod sysconfig;
pub mod trust;
pub mod verify;

/// Product name, as shown in titles.
pub const PRODUCT: &str = "QS/400 Access for Desktop";
/// Product version.
pub const VERSION: &str = "V1R0M0";
