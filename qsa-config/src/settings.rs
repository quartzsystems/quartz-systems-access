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

/// Screen size of a 5250 display session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenSize {
    #[default]
    #[serde(rename = "24x80")]
    S24x80,
    #[serde(rename = "27x132")]
    S27x132,
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

/// 5250 display settings for one system: the Start 5250 Display dialog's
/// last values and the keyboard map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Display {
    pub screen_size: ScreenSize,
    pub ccsid: u16,
    /// Empty: the system chooses.
    pub device_name: String,
    pub color_map: ColorMap,
    pub keymap: crate::KeyMap,
}

impl Default for Display {
    fn default() -> Display {
        Display {
            screen_size: ScreenSize::S24x80,
            ccsid: 37,
            device_name: String::new(),
            color_map: ColorMap::Console,
            keymap: crate::KeyMap::defaults(),
        }
    }
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
    /// 5250 display settings by system name.
    pub display: std::collections::BTreeMap<String, Display>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            version: 1,
            last_system: String::new(),
            window: None,
            display: Default::default(),
        }
    }
}

impl Settings {
    /// A system's display settings (the defaults if it has none).
    pub fn display_for(&self, system: &str) -> Display {
        self.display
            .get(&system.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    /// Keeps a system's display settings.
    pub fn set_display(&mut self, system: &str, d: Display) {
        self.display.insert(system.to_ascii_lowercase(), d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_settings_round_trip_per_system() {
        let mut s = Settings::default();
        let mut d = Display {
            screen_size: ScreenSize::S27x132,
            ccsid: 1140,
            device_name: "QSADSP01".into(),
            color_map: ColorMap::BlackOnWhite,
            ..Display::default()
        };
        d.keymap.assign("Ctrl+Enter", "FieldExit");
        s.set_display("QS400.example", d.clone());
        let text = toml::to_string(&s).unwrap();
        assert!(text.contains("screen_size = \"27x132\""), "{text}");
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.display_for("qs400.EXAMPLE"), d);
        assert_eq!(back.display_for("other"), Display::default());
        // Older files without the table still load.
        let old: Settings = toml::from_str("version = 1\nlast_system = \"a\"\n").unwrap();
        assert!(old.display.is_empty());
    }
}
