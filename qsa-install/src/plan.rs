// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! What an install is to do: for whom, where, and which shortcuts and file
//! types.
//!
//! | | Just me | All users |
//! |---|---|---|
//! | Windows | `%LOCALAPPDATA%\Programs\Quartz Systems\Access` | `%ProgramFiles%\Quartz Systems\Access` |
//! | Linux | `~/.local/opt/quartz-access` | `/opt/quartz-access` |

use std::path::{Path, PathBuf};

use crate::manifest::{Manifest, Scope};

/// The choices an install is made with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub scope: Scope,
    pub dir: PathBuf,
    /// Start menu folder (Windows) or applications menu entries (Linux).
    pub start_menu: bool,
    pub desktop_shortcut: bool,
    /// Register `.qsd`, `.qsp`, `.qkm` and `.qsm`.
    pub associations: bool,
}

impl Options {
    /// The defaults for `scope`: everything on, in the default folder.
    pub fn new(scope: Scope) -> Options {
        Options {
            scope,
            dir: default_dir(scope),
            start_menu: true,
            desktop_shortcut: true,
            associations: true,
        }
    }

    /// The choices an existing install was made with.
    pub fn from_manifest(m: &Manifest) -> Options {
        Options {
            scope: m.scope,
            dir: m.dir.clone(),
            start_menu: m.start_menu,
            desktop_shortcut: m.desktop_shortcut,
            associations: m.associations,
        }
    }
}

/// The default install folder for `scope`.
pub fn default_dir(scope: Scope) -> PathBuf {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(windows) {
        match scope {
            Scope::User => env("LOCALAPPDATA")
                .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default\AppData\Local"))
                .join("Programs")
                .join("Quartz Systems")
                .join("Access"),
            Scope::Machine => env("ProgramFiles")
                .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
                .join("Quartz Systems")
                .join("Access"),
        }
    } else {
        match scope {
            Scope::User => home().join(".local/opt/quartz-access"),
            Scope::Machine => PathBuf::from("/opt/quartz-access"),
        }
    }
}

/// The user's home folder.
pub fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// An install found in the default folders: for just me first, then for
/// all users.
pub fn existing() -> Option<Manifest> {
    [Scope::User, Scope::Machine]
        .into_iter()
        .find_map(|s| existing_in(&default_dir(s)))
}

/// The install in `dir`, if there is one.
pub fn existing_in(dir: &Path) -> Option<Manifest> {
    Manifest::load(dir).ok()
}

/// Whether this process may install for all users: elevated on Windows,
/// root on Linux.
pub fn is_admin() -> bool {
    #[cfg(windows)]
    {
        crate::windows::is_elevated()
    }
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions.
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        unsafe { geteuid() == 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_folders() {
        let u = default_dir(Scope::User)
            .to_string_lossy()
            .replace('\\', "/");
        let m = default_dir(Scope::Machine)
            .to_string_lossy()
            .replace('\\', "/");
        if cfg!(windows) {
            assert!(u.ends_with("/Programs/Quartz Systems/Access"), "{u}");
            assert!(m.ends_with("/Quartz Systems/Access"), "{m}");
        } else {
            assert!(u.ends_with("/.local/opt/quartz-access"), "{u}");
            assert_eq!(m, "/opt/quartz-access");
        }
    }
}
