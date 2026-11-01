// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Application settings (`settings.toml`).

use serde::{Deserialize, Serialize};

/// Position and size of the main window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Application settings. Every field has a default, so older and partial
/// files load.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    /// The system selected when the program last closed.
    pub last_system: String,
    /// Main window geometry when the program last closed.
    pub window: Option<Geometry>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            version: 1,
            last_system: String::new(),
            window: None,
        }
    }
}
