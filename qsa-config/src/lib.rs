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
//! - [`credentials`]: saved passwords, in the platform credential store only.

pub mod credentials;
pub mod keymap;
pub mod qsc;
pub mod settings;
pub mod store;
pub mod system;
pub mod systems;

pub use credentials::{Credentials, Saved, Secret};
pub use keymap::KeyMap;
pub use settings::Settings;
pub use store::{ConfigDir, ReadError};
pub use system::{AddressLookup, CaCertificate, CredentialMode, Invalid, Ports, System};
pub use systems::Systems;
