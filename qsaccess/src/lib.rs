// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QS/400 Access for Desktop: the application's windows and state. The
//! binary in `main.rs` parses arguments and runs the event loop.

pub mod about;
pub mod app;
pub mod certs;
pub mod chgpwd;
pub mod files;
pub mod ifs;
pub mod ipc;
pub mod localtime;
pub mod log;
pub mod macros;
pub mod mainwin;
pub mod manager;
pub mod messages;
pub mod open;
pub mod printer;
pub mod prompt;
pub mod properties;
pub mod qap;
pub mod recent;
pub mod request;
pub mod runcmd;
pub mod runsql;
pub mod schemas;
pub mod session;
pub mod shell;
pub mod signon;
pub mod splf;
pub mod sysconfig;
pub mod transfer;
pub mod trust;
pub mod verify;

/// Product name, as shown in titles.
pub const PRODUCT: &str = "QS/400 Access for Desktop";
/// Product version as a release, VxRyMz: the version in VERSION (through
/// the workspace version), major as V, minor as R and patch as M.
pub const VERSION: &str = concat!(
    "V",
    env!("CARGO_PKG_VERSION_MAJOR"),
    "R",
    env!("CARGO_PKG_VERSION_MINOR"),
    "M",
    env!("CARGO_PKG_VERSION_PATCH")
);

#[cfg(test)]
mod tests {
    /// VERSION at the root is the version: Cargo.toml's must follow it
    /// (ci/version.sh prepare changes both).
    #[test]
    fn version_follows_the_version_file() {
        let file = include_str!("../../VERSION").trim();
        assert_eq!(
            env!("CARGO_PKG_VERSION"),
            file,
            "ci/version.sh prepare {file}"
        );
        let mut parts = file.split('.');
        let release = format!(
            "V{}R{}M{}",
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap()
        );
        assert_eq!(super::VERSION, release);
    }
}
