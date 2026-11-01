// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Saved passwords. They go only to the platform credential store, through
//! the `keyring` crate: Windows Credential Manager, or Secret Service on
//! Linux. If the store cannot be reached nothing is saved anywhere, `QAD0040`
//! is raised once, and the user is prompted each time.
//!
//! There is one entry per system, keyed by system name, holding the user ID
//! and password together, so deleting a system removes everything saved for
//! it.

use std::cell::Cell;
use std::fmt;

/// Service name of every entry in the credential store.
pub const SERVICE: &str = "Quartz Systems QS/400 Access";

/// A password. Its `Debug` and `Display` never show it, and its memory is
/// overwritten when dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a password.
    pub fn new(s: impl Into<String>) -> Secret {
        Secret(s.into())
    }

    /// The password, for handing to a sign-on.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        bytes.fill(0);
        std::hint::black_box(&bytes);
    }
}

/// A saved sign-on for one system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub user: String,
    pub password: Secret,
}

impl Saved {
    fn encode(&self) -> Secret {
        Secret(format!("{}\n{}", self.user, self.password.expose()))
    }

    fn decode(s: &Secret) -> Option<Saved> {
        let (user, password) = s.expose().split_once('\n')?;
        Some(Saved {
            user: user.to_owned(),
            password: Secret::new(password),
        })
    }
}

/// Why a backend call failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BackendError {
    /// The store cannot be reached at all.
    #[error("credential store not available: {0}")]
    Unavailable(String),
    /// The store is there but this call failed.
    #[error("credential store error: {0}")]
    Failed(String),
}

/// A credential store. Entries are keyed by system name.
pub trait Backend {
    fn get(&self, system: &str) -> Result<Option<Secret>, BackendError>;
    fn set(&self, system: &str, secret: &Secret) -> Result<(), BackendError>;
    fn delete(&self, system: &str) -> Result<(), BackendError>;
}

/// The platform store, through `keyring`.
#[derive(Debug, Default)]
pub struct Keyring;

fn entry(system: &str) -> Result<keyring::Entry, BackendError> {
    keyring::Entry::new(SERVICE, &system.to_ascii_lowercase()).map_err(map_err)
}

fn map_err(e: keyring::Error) -> BackendError {
    use keyring::Error as E;
    match e {
        E::PlatformFailure(_) | E::NoStorageAccess(_) => BackendError::Unavailable(e.to_string()),
        other => BackendError::Failed(other.to_string()),
    }
}

impl Backend for Keyring {
    fn get(&self, system: &str) -> Result<Option<Secret>, BackendError> {
        match entry(system)?.get_password() {
            Ok(s) => Ok(Some(Secret(s))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_err(e)),
        }
    }

    fn set(&self, system: &str, secret: &Secret) -> Result<(), BackendError> {
        entry(system)?
            .set_password(secret.expose())
            .map_err(map_err)
    }

    fn delete(&self, system: &str) -> Result<(), BackendError> {
        match entry(system)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_err(e)),
        }
    }
}

/// An in-memory store, for tests.
#[derive(Debug, Default)]
pub struct Memory {
    entries: std::cell::RefCell<std::collections::BTreeMap<String, Secret>>,
    /// When set, every call fails as unavailable.
    pub unavailable: Cell<bool>,
}

impl Memory {
    fn check(&self) -> Result<(), BackendError> {
        if self.unavailable.get() {
            Err(BackendError::Unavailable("test".into()))
        } else {
            Ok(())
        }
    }

    /// Number of entries held.
    pub fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    /// Whether no entries are held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Backend for Memory {
    fn get(&self, system: &str) -> Result<Option<Secret>, BackendError> {
        self.check()?;
        Ok(self
            .entries
            .borrow()
            .get(&system.to_ascii_lowercase())
            .cloned())
    }

    fn set(&self, system: &str, secret: &Secret) -> Result<(), BackendError> {
        self.check()?;
        self.entries
            .borrow_mut()
            .insert(system.to_ascii_lowercase(), secret.clone());
        Ok(())
    }

    fn delete(&self, system: &str) -> Result<(), BackendError> {
        self.check()?;
        self.entries
            .borrow_mut()
            .remove(&system.to_ascii_lowercase());
        Ok(())
    }
}

/// Saved sign-ons, over a backend. Once the backend is found unavailable it
/// is not called again, and [`Credentials::take_warning`] yields `QAD0040`
/// exactly once.
pub struct Credentials {
    backend: Box<dyn Backend>,
    unavailable: Cell<bool>,
    warned: Cell<bool>,
}

impl Credentials {
    /// Saved sign-ons in the platform credential store.
    pub fn platform() -> Credentials {
        Credentials::with(Box::new(Keyring))
    }

    /// Saved sign-ons in the given backend.
    pub fn with(backend: Box<dyn Backend>) -> Credentials {
        Credentials {
            backend,
            unavailable: Cell::new(false),
            warned: Cell::new(false),
        }
    }

    /// Whether the store has been found unavailable.
    pub fn is_unavailable(&self) -> bool {
        self.unavailable.get()
    }

    fn guard<T>(&self, r: Result<T, BackendError>) -> Result<T, BackendError> {
        if let Err(BackendError::Unavailable(_)) = &r {
            self.unavailable.set(true);
        }
        r
    }

    /// The saved sign-on for a system, if any. `None` when nothing is saved
    /// or the store is unavailable.
    pub fn load(&self, system: &str) -> Option<Saved> {
        if self.unavailable.get() {
            return None;
        }
        self.guard(self.backend.get(system))
            .ok()
            .flatten()
            .and_then(|s| Saved::decode(&s))
    }

    /// Saves a sign-on. Returns false when it could not be saved, in which
    /// case nothing was written anywhere.
    pub fn save(&self, system: &str, saved: &Saved) -> bool {
        if self.unavailable.get() {
            return false;
        }
        self.guard(self.backend.set(system, &saved.encode()))
            .is_ok()
    }

    /// Removes any sign-on saved for a system.
    pub fn forget(&self, system: &str) -> bool {
        if self.unavailable.get() {
            return false;
        }
        self.guard(self.backend.delete(system)).is_ok()
    }

    /// `QAD0040`, the first time it is called after the store was found
    /// unavailable; `None` otherwise.
    pub fn take_warning(&self) -> Option<qsa_msg::Message> {
        if self.unavailable.get() && !self.warned.replace(true) {
            Some(qsa_msg::message(qsa_msg::id::NO_CREDENTIAL_STORE, &[]))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    struct Shared(Rc<Memory>);
    impl Backend for Shared {
        fn get(&self, s: &str) -> Result<Option<Secret>, BackendError> {
            self.0.get(s)
        }
        fn set(&self, s: &str, v: &Secret) -> Result<(), BackendError> {
            self.0.set(s, v)
        }
        fn delete(&self, s: &str) -> Result<(), BackendError> {
            self.0.delete(s)
        }
    }

    fn saved() -> Saved {
        Saved {
            user: "QSECOFR".into(),
            password: Secret::new("pa\nss word"),
        }
    }

    #[test]
    fn secrets_never_print() {
        let s = saved();
        let dbg = format!("{s:?} {} {:?}", s.password, s.password);
        assert!(!dbg.contains("hunter2"), "{dbg}");
        assert!(dbg.contains("QSECOFR"));
    }

    #[test]
    fn save_load_forget() {
        let mem = Rc::new(Memory::default());
        let c = Credentials::with(Box::new(Shared(mem.clone())));
        assert_eq!(c.load("SYS1"), None);
        assert!(c.save("SYS1", &saved()));
        assert_eq!(c.load("sys1"), Some(saved()));
        assert_eq!(mem.len(), 1);
        assert!(c.forget("Sys1"));
        assert!(mem.is_empty());
        assert_eq!(c.take_warning(), None);
    }

    #[test]
    fn unavailable_store_saves_nothing_and_warns_once() {
        let mem = Rc::new(Memory::default());
        mem.unavailable.set(true);
        let c = Credentials::with(Box::new(Shared(mem.clone())));
        assert!(!c.save("SYS1", &saved()));
        assert!(c.is_unavailable());
        let w = c.take_warning().expect("warned");
        assert_eq!(w.id, "QAD0040");
        assert_eq!(c.take_warning(), None);
        // Even if the store comes back, it is not used again this session.
        mem.unavailable.set(false);
        assert!(!c.save("SYS1", &saved()));
        assert!(mem.is_empty());
    }
}
