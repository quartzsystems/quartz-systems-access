// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Keyboard maps: which PC key performs which 5250 function.
//!
//! Keys are named as the keyboard map dialog shows them: modifiers first
//! (`Ctrl+`, `Shift+`, `Alt+`, in that order), then the key (`Enter`,
//! `Right Ctrl`, `Keypad +`, `F1`, `Page Up`, `R` ...). Functions are the
//! 5250 key names the session uses (`Enter`, `FieldExit`, `F:13`,
//! `SysReq` ...); [`FUNCTIONS`] lists them with their display names.
//!
//! A session keeps its map in its session file, as the changes from the
//! built-in defaults named [`DEFAULTS_ID`] (see [`crate::qsd`]); a whole map
//! is passed between sessions as a `.qkm` file (see [`crate::qkm`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Every 5250 function a key can be given: the stored name and the name
/// shown in the dialog, in the order the dialog lists them.
pub const FUNCTIONS: &[(&str, &str)] = &[
    ("Enter", "Enter"),
    ("FieldExit", "Field Exit"),
    ("FieldPlus", "Field+"),
    ("FieldMinus", "Field-"),
    ("F:1", "F1"),
    ("F:2", "F2"),
    ("F:3", "F3"),
    ("F:4", "F4"),
    ("F:5", "F5"),
    ("F:6", "F6"),
    ("F:7", "F7"),
    ("F:8", "F8"),
    ("F:9", "F9"),
    ("F:10", "F10"),
    ("F:11", "F11"),
    ("F:12", "F12"),
    ("F:13", "F13"),
    ("F:14", "F14"),
    ("F:15", "F15"),
    ("F:16", "F16"),
    ("F:17", "F17"),
    ("F:18", "F18"),
    ("F:19", "F19"),
    ("F:20", "F20"),
    ("F:21", "F21"),
    ("F:22", "F22"),
    ("F:23", "F23"),
    ("F:24", "F24"),
    ("PageUp", "Page Up (Roll Down)"),
    ("PageDown", "Page Down (Roll Up)"),
    ("Attention", "Attention"),
    ("SysReq", "System Request"),
    ("Reset", "Reset"),
    ("Insert", "Insert toggle"),
    ("Clear", "Clear"),
    ("Help", "Help"),
    ("Print", "Print"),
    ("RecordBackspace", "Record Backspace"),
    ("Home", "Home"),
    ("Tab", "Field advance"),
    ("BackTab", "Field back"),
    ("NewLine", "New Line"),
    ("Dup", "Dup"),
    ("EraseInput", "Erase Input"),
    ("EraseEof", "Erase EOF"),
    ("Delete", "Delete"),
    ("Backspace", "Backspace"),
    ("Left", "Cursor left"),
    ("Right", "Cursor right"),
    ("Up", "Cursor up"),
    ("Down", "Cursor down"),
];

/// Names [`KeyMap::defaults`] as session files refer to it. The defaults
/// must not change under this name: a change to them needs a new name, so
/// that files saved against the old ones keep meaning what they meant.
pub const DEFAULTS_ID: &str = "defaults-v1";

/// Whether `f` is one of the [`FUNCTIONS`].
pub fn is_function(f: &str) -> bool {
    FUNCTIONS.iter().any(|(name, _)| *name == f)
}

/// The display name of a function, or the stored name if unknown.
pub fn function_label(f: &str) -> &str {
    FUNCTIONS
        .iter()
        .find(|(name, _)| *name == f)
        .map_or(f, |(_, label)| label)
}

/// A keyboard map: key name to function name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyMap {
    pub keys: BTreeMap<String, String>,
}

impl Default for KeyMap {
    fn default() -> KeyMap {
        KeyMap::defaults()
    }
}

impl KeyMap {
    /// The default map.
    pub fn defaults() -> KeyMap {
        let mut keys = BTreeMap::new();
        let mut put = |k: &str, f: &str| {
            keys.insert(k.to_owned(), f.to_owned());
        };
        put("Enter", "Enter");
        put("Right Ctrl", "FieldExit");
        put("Keypad +", "FieldPlus");
        put("Keypad -", "FieldMinus");
        for n in 1..=12 {
            put(&format!("F{n}"), &format!("F:{n}"));
            put(&format!("Shift+F{n}"), &format!("F:{}", n + 12));
        }
        put("Page Up", "PageUp");
        put("Page Down", "PageDown");
        put("Esc", "Attention");
        put("Ctrl+R", "Reset");
        put("Shift+Esc", "SysReq");
        put("Insert", "Insert");
        put("Pause", "Clear");
        put("Ctrl+Home", "Home");
        put("Tab", "Tab");
        put("Shift+Tab", "BackTab");
        // Not in the 5250 table, but needed to work at all.
        put("Left", "Left");
        put("Right", "Right");
        put("Up", "Up");
        put("Down", "Down");
        put("Delete", "Delete");
        put("Backspace", "Backspace");
        KeyMap { keys }
    }

    /// The function a key performs.
    pub fn function(&self, key: &str) -> Option<&str> {
        self.keys.get(key).map(String::as_str)
    }

    /// The keys that perform a function, in name order.
    pub fn keys_for(&self, function: &str) -> Vec<&str> {
        self.keys
            .iter()
            .filter(|(_, f)| *f == function)
            .map(|(k, _)| k.as_str())
            .collect()
    }

    /// Gives `key` to `function`, taking it from whatever it did before.
    pub fn assign(&mut self, key: &str, function: &str) {
        self.keys.insert(key.to_owned(), function.to_owned());
    }

    /// Removes every key from a function.
    pub fn clear_function(&mut self, function: &str) {
        self.keys.retain(|_, f| f != function);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_table() {
        let m = KeyMap::defaults();
        for (k, f) in [
            ("Enter", "Enter"),
            ("Right Ctrl", "FieldExit"),
            ("Keypad +", "FieldPlus"),
            ("Keypad -", "FieldMinus"),
            ("F1", "F:1"),
            ("F12", "F:12"),
            ("Shift+F1", "F:13"),
            ("Shift+F12", "F:24"),
            ("Page Up", "PageUp"),
            ("Page Down", "PageDown"),
            ("Esc", "Attention"),
            ("Ctrl+R", "Reset"),
            ("Shift+Esc", "SysReq"),
            ("Insert", "Insert"),
            ("Pause", "Clear"),
            ("Ctrl+Home", "Home"),
            ("Tab", "Tab"),
            ("Shift+Tab", "BackTab"),
        ] {
            assert_eq!(m.function(k), Some(f), "{k}");
        }
        // Every default function is a known one.
        for f in m.keys.values() {
            assert!(
                FUNCTIONS.iter().any(|(n, _)| n == f),
                "{f} is not a function"
            );
        }
    }

    #[test]
    fn assign_and_clear() {
        let mut m = KeyMap::defaults();
        m.assign("Ctrl+Enter", "FieldExit");
        assert_eq!(m.keys_for("FieldExit"), ["Ctrl+Enter", "Right Ctrl"]);
        m.assign("Right Ctrl", "Reset");
        assert_eq!(m.keys_for("FieldExit"), ["Ctrl+Enter"]);
        m.clear_function("Reset");
        assert!(m.function("Ctrl+R").is_none() && m.function("Right Ctrl").is_none());
    }

    #[test]
    fn saves_as_a_plain_table() {
        let mut m = KeyMap {
            keys: BTreeMap::new(),
        };
        m.assign("Right Ctrl", "FieldExit");
        #[derive(Serialize, Deserialize)]
        struct W {
            keymap: KeyMap,
        }
        let t = toml::to_string(&W { keymap: m.clone() }).unwrap();
        assert_eq!(t, "[keymap]\n\"Right Ctrl\" = \"FieldExit\"\n");
        let back: W = toml::from_str(&t).unwrap();
        assert_eq!(back.keymap, m);
    }
}
