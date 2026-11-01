// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Application settings (`settings.toml`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How many sessions File > Recent Sessions lists.
pub const RECENT_MAX: usize = 10;

/// Position and size of a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Screen size of a 5250 display session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenSize {
    #[default]
    #[serde(rename = "24x80")]
    S24x80,
    #[serde(rename = "27x132")]
    S27x132,
}

impl ScreenSize {
    /// Rows and columns.
    pub fn rows_cols(self) -> (usize, usize) {
        match self {
            ScreenSize::S24x80 => (24, 80),
            ScreenSize::S27x132 => (27, 132),
        }
    }

    /// As shown to the user: `24 x 80`.
    pub fn label(self) -> &'static str {
        match self {
            ScreenSize::S24x80 => "24 x 80",
            ScreenSize::S27x132 => "27 x 132",
        }
    }
}

/// Colour map of a 5250 display session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ColorMap {
    /// The QS/400 service console's colours.
    #[default]
    Console,
    BlackOnWhite,
}

/// The CCSIDs a 5250 display session can use.
pub const CCSIDS: &[u16] = &[37, 273, 285, 297, 500, 1140];

/// Checks a device name: empty, or 1 to 10 of A-Z, 0-9, #, $, @ and _,
/// starting with a letter, #, $ or @. Returns it in upper case.
pub fn check_device(name: &str) -> Result<String, String> {
    let n = name.trim().to_ascii_uppercase();
    if n.is_empty() {
        return Ok(n);
    }
    let ok = n.len() <= 10
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || "#$@_".contains(c))
        && n.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || "#$@".contains(c));
    if ok {
        Ok(n)
    } else {
        Err(
            "Device name must be 1 to 10 letters, digits, #, $, @ or _, starting with a letter, #, $ or @."
                .into(),
        )
    }
}

/// 5250 display settings for one system: the Start 5250 Display dialog's
/// last values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Display {
    pub screen_size: ScreenSize,
    pub ccsid: u16,
    /// Empty: the system chooses.
    pub device_name: String,
    pub color_map: ColorMap,
    /// A keyboard map kept per system by an earlier release. It is read so
    /// that [`crate::migrate`] can move it into session files, and is never
    /// written back once that is done.
    #[serde(rename = "keymap", skip_serializing_if = "Option::is_none")]
    pub legacy_keymap: Option<crate::KeyMap>,
}

impl Default for Display {
    fn default() -> Display {
        Display {
            screen_size: ScreenSize::S24x80,
            ccsid: 37,
            device_name: String::new(),
            color_map: ColorMap::Console,
            legacy_keymap: None,
        }
    }
}

/// Application settings. Every field has a default, so older and partial
/// files load.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    /// The system selected when the program last closed.
    pub last_system: String,
    /// Main window geometry when the program last closed.
    pub window: Option<Geometry>,
    /// 5250 display settings by system name.
    pub display: std::collections::BTreeMap<String, Display>,
    /// Session files opened or saved most recently, newest first.
    pub recent_sessions: Vec<PathBuf>,
    /// The keyboard map new sessions start from, as changes from the
    /// built-in defaults. `None`: the defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_session_keymap: Option<crate::qsd::KeymapOverrides>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            version: 1,
            last_system: String::new(),
            window: None,
            display: Default::default(),
            recent_sessions: Vec::new(),
            new_session_keymap: None,
        }
    }
}

/// Whether two paths name the same file, ignoring case on Windows.
pub fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

impl Settings {
    /// Puts `path` at the top of the recent sessions, keeping at most
    /// [`RECENT_MAX`].
    pub fn add_recent(&mut self, path: &Path) {
        self.recent_sessions.retain(|p| !same_path(p, path));
        self.recent_sessions.insert(0, path.to_owned());
        self.recent_sessions.truncate(RECENT_MAX);
    }

    /// The keyboard map a new session starts from.
    pub fn new_session_keymap(&self) -> crate::KeyMap {
        self.new_session_keymap
            .as_ref()
            .and_then(|k| k.to_map().ok())
            .unwrap_or_default()
    }

    /// A system's display settings (the defaults if it has none).
    pub fn display_for(&self, system: &str) -> Display {
        self.display
            .get(&system.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    /// Keeps a system's display settings. A keyboard map not yet migrated
    /// stays with them.
    pub fn set_display(&mut self, system: &str, mut d: Display) {
        let key = system.to_ascii_lowercase();
        if d.legacy_keymap.is_none() {
            d.legacy_keymap = self
                .display
                .get(&key)
                .and_then(|old| old.legacy_keymap.clone());
        }
        self.display.insert(key, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_settings_round_trip_per_system() {
        let mut s = Settings::default();
        let d = Display {
            screen_size: ScreenSize::S27x132,
            ccsid: 1140,
            device_name: "QSADSP01".into(),
            color_map: ColorMap::BlackOnWhite,
            ..Display::default()
        };
        s.set_display("QS400.example", d.clone());
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("screen_size = \"27x132\""), "{text}");
        assert!(!text.contains("keymap"), "{text}");
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.display_for("qs400.EXAMPLE"), d);
        assert_eq!(back.display_for("other"), Display::default());
        // Older files without the table still load.
        let old: Settings = toml::from_str("version = 1\nlast_system = \"a\"\n").unwrap();
        assert!(old.display.is_empty());
    }

    #[test]
    fn earlier_per_system_maps_are_read_and_kept_until_migrated() {
        let text =
            "[display.sys1]\nccsid = 37\n\n[display.sys1.keymap]\n\"Right Ctrl\" = \"FieldExit\"\n";
        let mut s: Settings = toml::from_str(text).unwrap();
        let map = s.display_for("sys1").legacy_keymap.expect("read");
        assert_eq!(map.function("Right Ctrl"), Some("FieldExit"));
        // The Start dialog saving its values does not drop it.
        s.set_display(
            "SYS1",
            Display {
                ccsid: 285,
                ..Display::default()
            },
        );
        assert!(s.display_for("sys1").legacy_keymap.is_some());
        assert_eq!(s.display_for("sys1").ccsid, 285);
    }

    #[test]
    fn recent_sessions_newest_first_without_repeats() {
        let mut s = Settings::default();
        for n in 0..12 {
            s.add_recent(Path::new(&format!("/s/{n}.qsd")));
        }
        assert_eq!(s.recent_sessions.len(), RECENT_MAX);
        assert_eq!(s.recent_sessions[0], Path::new("/s/11.qsd"));
        s.add_recent(Path::new("/s/5.qsd"));
        assert_eq!(s.recent_sessions[0], Path::new("/s/5.qsd"));
        assert_eq!(s.recent_sessions.len(), RECENT_MAX);
        assert_eq!(
            s.recent_sessions
                .iter()
                .filter(|p| p.ends_with("5.qsd"))
                .count(),
            1
        );
        let text = toml::to_string(&s).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.recent_sessions, s.recent_sessions);
    }

    #[test]
    fn new_session_keymap_defaults_until_set() {
        let mut s = Settings::default();
        assert_eq!(s.new_session_keymap(), crate::KeyMap::defaults());
        let mut m = crate::KeyMap::defaults();
        m.assign("Ctrl+Enter", "FieldExit");
        s.new_session_keymap = Some(crate::qsd::KeymapOverrides::from_map(&m));
        let back: Settings = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.new_session_keymap(), m);
    }

    #[test]
    fn device_names() {
        assert_eq!(check_device(" qsadsp01 ").as_deref(), Ok("QSADSP01"));
        assert_eq!(check_device("").as_deref(), Ok(""));
        assert_eq!(check_device("#DSP$@_1").as_deref(), Ok("#DSP$@_1"));
        assert!(check_device("1DSP").is_err());
        assert!(check_device("BAD.NAME").is_err());
        assert!(check_device("ELEVENCHARS").is_err());
    }
}
