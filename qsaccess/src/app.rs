// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Application state shared by the windows.

use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use qsa_config::{ConfigDir, ReadError, Settings, Systems, migrate, store};
use qsa_msg::Message;

/// Everything the windows share.
pub struct AppState {
    /// Settings directory; `None` when the platform gives no home.
    pub dir: Option<ConfigDir>,
    pub systems: Systems,
    pub settings: Settings,
    /// Files that could not be read at start-up, to report as `QAD0030`.
    pub startup_errors: Vec<ReadError>,
    /// Record the next 5250 session opened to this file (`--record`).
    pub record_next: Option<std::path::PathBuf>,
    /// Messages to show in the main window's status bar once it is up.
    pub startup_messages: Vec<Message>,
    /// Moves on whenever the recent sessions change, so that every window
    /// can redraw its Recent Sessions menu.
    pub recent_gen: u64,
    /// This process's QAP session per system, shared by every tool.
    pub qap: crate::qap::Registry,
}

/// Shared, mutable application state.
pub type Shared = Rc<RefCell<AppState>>;

impl AppState {
    /// Loads settings and systems from `dir`. The first start after an
    /// upgrade from a release that could save passwords (a systems file
    /// older than version 4) deletes what it saved, and rewrites the file.
    pub fn load(dir: Option<ConfigDir>) -> AppState {
        let mut startup_errors = Vec::new();
        let mut upgraded = false;
        let (settings, systems) = match &dir {
            Some(d) => {
                let (settings, e1) = d.load_settings();
                let (systems, version, e2) = d.load_systems_versioned();
                upgraded = version.is_some_and(|v| v < qsa_config::systems::VERSION);
                startup_errors.extend(e1);
                startup_errors.extend(e2);
                (settings, systems)
            }
            None => (Settings::default(), Systems::new()),
        };
        let mut st = AppState::new(dir, systems, settings);
        st.startup_errors = startup_errors;
        if upgraded {
            st.forget_saved_passwords();
        }
        st.migrate_keymaps();
        st
    }

    /// State from values already in hand (tests, screenshots).
    pub fn new(dir: Option<ConfigDir>, systems: Systems, settings: Settings) -> AppState {
        AppState {
            dir,
            systems,
            settings,
            startup_errors: Vec::new(),
            record_next: None,
            startup_messages: Vec::new(),
            recent_gen: 0,
            qap: crate::qap::Registry::default(),
        }
    }

    /// Puts a session file at the top of Recent Sessions and saves the
    /// settings.
    pub fn note_recent(&mut self, path: &std::path::Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        self.settings.add_recent(&path);
        self.recent_gen += 1;
        if let Err(e) = self.save_settings() {
            crate::log!("settings not saved: {e}");
        }
    }

    /// Takes a session file off Recent Sessions (it could not be opened).
    pub fn forget_recent(&mut self, path: &std::path::Path) {
        let before = self.settings.recent_sessions.len();
        self.settings
            .recent_sessions
            .retain(|p| !qsa_config::settings::same_path(p, path));
        if self.settings.recent_sessions.len() != before {
            self.recent_gen += 1;
            if let Err(e) = self.save_settings() {
                crate::log!("settings not saved: {e}");
            }
        }
    }

    /// Deletes the passwords earlier releases kept in the platform
    /// credential store, ignoring errors, and writes the systems file at
    /// the current version so that this happens once.
    fn forget_saved_passwords(&mut self) {
        let f = qsa_config::legacy::forget_saved_passwords();
        qsa_net::info!(
            "saved passwords from an earlier release removed from the credential store: {}{}",
            f.removed,
            if f.store_unavailable {
                " (the store could not be searched; any left can be removed with the desktop's password manager)"
            } else {
                ""
            }
        );
        if let Err(e) = self.save_systems() {
            crate::log!("systems not saved after removing saved passwords: {e}");
        }
    }

    /// Moves keyboard maps an earlier release kept per system into session
    /// files, once (see [`qsa_config::migrate`]), and logs what moved.
    fn migrate_keymaps(&mut self) {
        if !migrate::pending(&self.settings) {
            return;
        }
        let Some(sessions) = store::default_sessions_dir() else {
            crate::log!("keyboard maps not migrated: no Documents folder");
            return;
        };
        match migrate::migrate_keymaps(
            &mut self.settings,
            &self.systems,
            std::slice::from_ref(&sessions),
            &sessions,
        ) {
            Ok(report) => {
                for line in report.log_lines() {
                    crate::log!("{line}");
                }
                if let Err(e) = self.save_settings() {
                    crate::log!("settings not saved after keyboard map migration: {e}");
                }
                if report.count() > 0 {
                    let m = qsa_msg::message(
                        qsa_msg::id::KEYMAPS_MIGRATED,
                        &[&report.count().to_string()],
                    );
                    crate::log!("{m}");
                    self.startup_messages.push(m);
                }
            }
            Err(e) => crate::log!("keyboard maps not migrated; will try again: {e}"),
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
