// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! One-time move of the keyboard maps an earlier release kept per system in
//! `settings.toml` into session files.
//!
//! For each system with a map of its own (one that differs from the
//! defaults):
//!
//! - every session file for that system with no `[keymap]` yet is given the
//!   map;
//! - if there is no session file for the system at all, one is created in
//!   the Sessions folder from the Start dialog's last values for it, so the
//!   map is not lost.
//!
//! Maps equal to the defaults carry nothing and are simply dropped. Once
//! every file is written, the maps are removed from the settings; the
//! caller saves them. If a write fails, the settings keep their maps and the
//! move is tried again at the next start.

use std::io;
use std::path::{Path, PathBuf};

use crate::keymap::KeyMap;
use crate::qsd::{self, SessionFile};
use crate::settings::Settings;
use crate::system::System;
use crate::systems::Systems;

/// What was done for one system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Migrated {
    pub system: String,
    /// Session files given the map.
    pub applied_to: Vec<PathBuf>,
    /// The session file created for it, if there was none.
    pub created: Option<PathBuf>,
}

/// What the migration did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub migrated: Vec<Migrated>,
    /// Systems whose map was the defaults: nothing to move.
    pub dropped_default: Vec<String>,
    /// Session files that could not be read, and why. They were left alone.
    pub skipped: Vec<(PathBuf, String)>,
}

impl Report {
    /// Lines for the log.
    pub fn log_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for m in &self.migrated {
            for p in &m.applied_to {
                out.push(format!(
                    "keyboard map of system {} moved into {}",
                    m.system,
                    p.display()
                ));
            }
            if let Some(p) = &m.created {
                out.push(format!(
                    "keyboard map of system {} moved into new session file {}",
                    m.system,
                    p.display()
                ));
            }
            if m.applied_to.is_empty() && m.created.is_none() {
                out.push(format!(
                    "keyboard map of system {} dropped: every session file for it already has a map",
                    m.system
                ));
            }
        }
        for s in &self.dropped_default {
            out.push(format!(
                "keyboard map of system {s} was the default map; nothing to move"
            ));
        }
        for (p, why) in &self.skipped {
            out.push(format!("{} not read during migration: {why}", p.display()));
        }
        out
    }

    /// Systems whose map went into at least one session file.
    pub fn count(&self) -> usize {
        self.migrated
            .iter()
            .filter(|m| !m.applied_to.is_empty() || m.created.is_some())
            .count()
    }
}

/// Whether the settings still hold maps to move.
pub fn pending(settings: &Settings) -> bool {
    settings.display.values().any(|d| d.legacy_keymap.is_some())
}

/// The `.qsd` files directly in `dir` (not in subfolders), sorted.
pub fn session_files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case(qsd::EXTENSION))
        })
        .collect();
    out.sort();
    out
}

/// A file name for a session of `system` in `dir` that is not taken.
fn free_name(dir: &Path, system: &str) -> PathBuf {
    let stem: String = system
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut p = dir.join(format!("{stem}.{}", qsd::EXTENSION));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem} ({n}).{}", qsd::EXTENSION));
        n += 1;
    }
    p
}

/// Moves the per-system maps in `settings` into the session files in
/// `folders`, creating files in `new_dir` where needed. See the module
/// documentation.
pub fn migrate_keymaps(
    settings: &mut Settings,
    systems: &Systems,
    folders: &[PathBuf],
    new_dir: &Path,
) -> io::Result<Report> {
    let mut report = Report::default();
    if !pending(settings) {
        return Ok(report);
    }
    let mut sessions: Vec<(PathBuf, SessionFile)> = Vec::new();
    for dir in folders {
        for path in session_files_in(dir) {
            match SessionFile::load(&path) {
                Ok(f) => sessions.push((path, f)),
                Err(e) => report.skipped.push((path, e.to_string())),
            }
        }
    }
    let defaults = KeyMap::defaults();
    for (key, display) in &settings.display {
        let Some(map) = &display.legacy_keymap else {
            continue;
        };
        let name = systems
            .get(key)
            .map_or_else(|| key.clone(), |s| s.name.clone());
        if *map == defaults {
            report.dropped_default.push(name);
            continue;
        }
        let mut done = Migrated {
            system: name.clone(),
            applied_to: Vec::new(),
            created: None,
        };
        let mut any = false;
        for (path, f) in sessions.iter_mut() {
            if !f.system.name.eq_ignore_ascii_case(key) {
                continue;
            }
            any = true;
            if f.keymap.is_none() {
                f.set_keymap(map);
                f.save(path)?;
                done.applied_to.push(path.clone());
            }
        }
        if !any {
            let system = systems
                .get(key)
                .cloned()
                .unwrap_or_else(|| System::new(&name));
            let mut f = SessionFile::new(&system);
            f.session.title = name.clone();
            f.session.screen_size = display.screen_size;
            f.session.ccsid = display.ccsid;
            f.session.device_name = display.device_name.clone();
            f.session.colour_map = display.color_map;
            f.set_keymap(map);
            let path = free_name(new_dir, &name);
            f.save(&path)?;
            done.created = Some(path);
        }
        report.migrated.push(done);
    }
    for d in settings.display.values_mut() {
        d.legacy_keymap = None;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Display, ScreenSize};
    use crate::store::tests::temp_dir;

    fn custom() -> KeyMap {
        let mut m = KeyMap::defaults();
        m.assign("Ctrl+Enter", "FieldExit");
        m
    }

    fn legacy(settings: &mut Settings, system: &str, map: KeyMap, size: ScreenSize) {
        settings.display.insert(
            system.to_ascii_lowercase(),
            Display {
                screen_size: size,
                device_name: "QSADSP01".into(),
                legacy_keymap: Some(map),
                ..Display::default()
            },
        );
    }

    #[test]
    fn moves_maps_into_sessions_without_one_and_creates_where_none() {
        let root = temp_dir("migrate");
        let folder = root.join("Sessions");
        let mut systems = Systems::new();
        systems.upsert(System::new("SYS1"));
        systems.upsert(System::new("sys2"));
        // sys1 has two sessions: one without a map, one with its own.
        let mut plain = SessionFile::new(&System::new("sys1"));
        plain.save(&folder.join("a.qsd")).unwrap();
        let mut own = KeyMap::defaults();
        own.assign("F1", "Help");
        plain.set_keymap(&own);
        plain.save(&folder.join("b.qsd")).unwrap();
        std::fs::write(folder.join("broken.qsd"), "not toml [").unwrap();

        let mut settings = Settings::default();
        legacy(&mut settings, "sys1", custom(), ScreenSize::S24x80);
        legacy(&mut settings, "sys2", custom(), ScreenSize::S27x132);
        legacy(
            &mut settings,
            "sys3",
            KeyMap::defaults(),
            ScreenSize::S24x80,
        );
        assert!(pending(&settings));

        let report = migrate_keymaps(
            &mut settings,
            &systems,
            std::slice::from_ref(&folder),
            &folder,
        )
        .unwrap();
        assert!(!pending(&settings));
        assert_eq!(report.count(), 2);
        assert_eq!(report.dropped_default, ["sys3"]);
        assert_eq!(report.skipped.len(), 1);

        assert_eq!(
            SessionFile::load(&folder.join("a.qsd")).unwrap().keymap(),
            custom()
        );
        assert_eq!(
            SessionFile::load(&folder.join("b.qsd")).unwrap().keymap(),
            own,
            "a session's own map is not replaced"
        );
        let created = folder.join("sys2.qsd");
        let f = SessionFile::load(&created).unwrap();
        assert_eq!(f.keymap(), custom());
        assert_eq!(f.session.screen_size, ScreenSize::S27x132);
        assert_eq!(f.session.device_name, "QSADSP01");
        assert_eq!(f.system.name, "sys2");
        assert_eq!(
            report.migrated[1].created.as_deref(),
            Some(created.as_path())
        );

        // Nothing is left to do, and a second run changes nothing.
        let text = toml::to_string(&settings).unwrap();
        assert!(!text.contains("keymap"), "{text}");
        let again = migrate_keymaps(
            &mut settings,
            &systems,
            std::slice::from_ref(&folder),
            &folder,
        )
        .unwrap();
        assert_eq!(again, Report::default());
        assert!(report.log_lines().iter().any(|l| l.contains("sys2.qsd")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn created_names_do_not_overwrite() {
        let dir = temp_dir("migrate-names");
        std::fs::write(dir.join("a_b.qsd"), "x").unwrap();
        assert_eq!(free_name(&dir, "a b"), dir.join("a_b (2).qsd"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
