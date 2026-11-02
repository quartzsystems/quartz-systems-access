// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Settings and System Configurations for QS/400 Access for Desktop.
//!
//! - [`system`]: one system configuration and its validation.
//! - [`systems`]: the list of systems.
//! - [`settings`]: application settings, 5250 display settings per system.
//! - [`keymap`]: keyboard maps for 5250 display sessions.
//! - [`store`]: where `settings.toml` and `systems.toml` live, and atomic
//!   reads and writes.
//! - [`qsc`]: `.qsc` export and import.
//! - [`doc`]: what session, keyboard map and macro files share: header,
//!   version check, unknown keys, relative paths.
//! - [`qsd`], [`qsp`], [`qkm`], [`qsm`]: display session, printer session,
//!   keyboard map and macro files.
//! - [`migrate`]: moving the earlier per-system keyboard maps into session
//!   files.
//! - [`legacy`]: removing the passwords earlier releases saved. Passwords
//!   are never saved.

pub mod doc;
pub mod keymap;
pub mod legacy;
pub mod migrate;
pub mod qkm;
pub mod qsc;
pub mod qsd;
pub mod qsm;
pub mod qsp;
pub mod settings;
pub mod store;
pub mod system;
pub mod systems;

pub use doc::FileError;
pub use keymap::KeyMap;
pub use settings::Settings;
pub use store::{ConfigDir, ReadError};
pub use system::{AddressLookup, CaCertificate, Invalid, Ports, System};
pub use systems::Systems;
