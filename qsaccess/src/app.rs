// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Application state shared by the windows.

use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use qsa_config::{ConfigDir, Credentials, ReadError, Settings, Systems};

/// Everything the windows share.
pub struct AppState {
    /// Settings directory; `None` when the platform gives no home.
    pub dir: Option<ConfigDir>,
    pub systems: Systems,
    pub settings: Settings,
    pub credentials: Credentials,
    /// Files that could not be read at start-up, to report as `QAD0030`.
    pub startup_errors: Vec<ReadError>,
}

/// Shared, mutable application state.
pub type Shared = Rc<RefCell<AppState>>;

impl AppState {
    /// Loads settings and systems from `dir`.
    pub fn load(dir: Option<ConfigDir>, credentials: Credentials) -> AppState {
        let mut startup_errors = Vec::new();
        let (settings, systems) = match &dir {
            Some(d) => {
                let (settings, e1) = d.load_settings();
                let (systems, e2) = d.load_systems();
                startup_errors.extend(e1);
                startup_errors.extend(e2);
                (settings, systems)
            }
            None => (Settings::default(), Systems::new()),
        };
        AppState {
            dir,
            systems,
            settings,
            credentials,
            startup_errors,
        }
    }

    /// Writes `systems.toml`.
    pub fn save_systems(&self) -> io::Result<()> {
        match &self.dir {
            Some(d) => d.save_systems(&self.systems),
            None => Ok(()),
        }
    }

    /// Writes `settings.toml`.
    pub fn save_settings(&self) -> io::Result<()> {
        match &self.dir {
            Some(d) => d.save_settings(&self.settings),
            None => Ok(()),
        }
    }

    /// Wraps the state for sharing.
    pub fn shared(self) -> Shared {
        Rc::new(RefCell::new(self))
    }
}
