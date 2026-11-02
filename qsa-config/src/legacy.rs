// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Removing the passwords releases before systems file version 4 kept in
//! the platform credential store. Passwords are never saved now: the program
//! signs on once per system per run and keeps the QAP session instead.
//!
//! Those releases wrote one entry per system under the service name
//! [`SERVICE`]:
//!
//! - Windows: a generic Credential Manager credential whose target name is
//!   the system name in lower case, a dot, and [`SERVICE`]. Every such
//!   credential is deleted, whether or not its system is still configured.
//! - Linux: a Secret Service item with the attribute `service` set to
//!   [`SERVICE`]. They are found and cleared with `secret-tool` (libsecret),
//!   when it is installed; without it nothing is removed and [`Forgotten`]
//!   says so.
//!
//! Every failure is ignored: what could not be removed stays, and nothing
//! else depends on it.

/// The service name earlier releases saved every entry under.
pub const SERVICE: &str = "Quartz Systems QS/400 Access";

/// What [`forget_saved_passwords`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Forgotten {
    /// Entries removed.
    pub removed: usize,
    /// The store could not be searched (on Linux, `secret-tool` is
    /// missing or failed), so entries may remain.
    pub store_unavailable: bool,
}

/// Deletes every entry earlier releases saved, ignoring errors.
pub fn forget_saved_passwords() -> Forgotten {
    imp::forget()
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    use windows_sys::Win32::Security::Credentials::{
        CRED_ENUMERATE_ALL_CREDENTIALS, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW,
        CredEnumerateW, CredFree,
    };

    use super::{Forgotten, SERVICE};

    /// A null-terminated wide string as a `String`.
    ///
    /// # Safety
    /// `p` is null or points at a null-terminated UTF-16 string.
    unsafe fn wide(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let mut n = 0;
        // SAFETY: the caller promises a terminator.
        while unsafe { *p.add(n) } != 0 {
            n += 1;
        }
        // SAFETY: `n` units were just read.
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, n) })
    }

    /// The target names of this program's generic credentials.
    fn targets() -> Vec<String> {
        let suffix = format!(".{SERVICE}").to_lowercase();
        let mut count = 0u32;
        let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: both out-pointers are valid; a null filter with
        // CRED_ENUMERATE_ALL_CREDENTIALS lists every credential of the user.
        let ok = unsafe {
            CredEnumerateW(
                std::ptr::null(),
                CRED_ENUMERATE_ALL_CREDENTIALS,
                &mut count,
                &mut list,
            )
        };
        if ok == 0 {
            // ERROR_NOT_FOUND (none at all) lands here too.
            return Vec::new();
        }
        let mut out = Vec::new();
        for i in 0..count as usize {
            // SAFETY: CredEnumerateW returned `count` valid pointers.
            let c = unsafe { &**list.add(i) };
            if c.Type != CRED_TYPE_GENERIC {
                continue;
            }
            // SAFETY: TargetName is a null-terminated string.
            let name = unsafe { wide(c.TargetName) };
            if name.to_lowercase().ends_with(&suffix) {
                out.push(name);
            }
        }
        // SAFETY: `list` came from CredEnumerateW and is freed once.
        unsafe { CredFree(list as *const c_void) };
        out
    }

    pub fn forget() -> Forgotten {
        let removed = targets()
            .iter()
            .filter(|name| {
                let w: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
                // SAFETY: `w` is null-terminated and outlives the call.
                unsafe { CredDeleteW(w.as_ptr(), CRED_TYPE_GENERIC, 0) != 0 }
            })
            .count();
        Forgotten {
            removed,
            store_unavailable: false,
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::process::{Command, Stdio};

    use super::{Forgotten, SERVICE};

    /// `secret-tool` with `args`; its standard output when it ran and
    /// succeeded.
    fn secret_tool(args: &[&str]) -> Option<String> {
        let out = Command::new("secret-tool")
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub fn forget() -> Forgotten {
        // `search --all` prints each item as a "[/object/path]" line and
        // its attributes; it fails when nothing matches.
        let found = match secret_tool(&["search", "--all", "service", SERVICE]) {
            Some(text) => text.lines().filter(|l| l.starts_with("[/")).count(),
            None if Command::new("secret-tool")
                .arg("--version")
                .output()
                .is_err() =>
            {
                return Forgotten {
                    removed: 0,
                    store_unavailable: true,
                };
            }
            None => 0,
        };
        if found == 0 {
            return Forgotten::default();
        }
        match secret_tool(&["clear", "service", SERVICE]) {
            Some(_) => Forgotten {
                removed: found,
                store_unavailable: false,
            },
            None => Forgotten {
                removed: 0,
                store_unavailable: true,
            },
        }
    }
}
