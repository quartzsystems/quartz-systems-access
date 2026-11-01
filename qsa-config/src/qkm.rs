// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! `.qkm` files: a whole keyboard map, for passing between sessions and
//! computers. Loading one copies it; nothing keeps a link to the file.
//!
//! ```toml
//! [qsaccess]
//! kind = "keymap"
//! version = 1
//!
//! [keys]
//! Enter = "Enter"
//! "Right Ctrl" = "FieldExit"
//! ```

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::doc::{self, Extra, FileError, Header};
use crate::keymap::{self, KeyMap};

/// File extension, without the dot.
pub const EXTENSION: &str = "qkm";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct KeymapDoc {
    qsaccess: Header,
    #[serde(default)]
    keys: std::collections::BTreeMap<String, String>,
    #[serde(flatten)]
    extra: Extra,
}

/// A keyboard map file.
#[derive(Clone, Debug, PartialEq)]
pub struct KeymapFile {
    pub header: Header,
    pub map: KeyMap,
    pub extra: Extra,
}

impl KeymapFile {
    /// A file holding `map`.
    pub fn new(map: KeyMap) -> KeymapFile {
        KeymapFile {
            header: Header::new(doc::KEYMAP),
            map,
            extra: Extra::new(),
        }
    }

    /// Parses `.qkm` text. Every function must be one this release knows.
    pub fn parse(text: &str) -> Result<KeymapFile, FileError> {
        let d: KeymapDoc = doc::parse(text, doc::KEYMAP)?;
        for (k, f) in &d.keys {
            if !keymap::is_function(f) {
                return Err(FileError::Invalid(format!(
                    "key {k} is given an unknown function \"{f}\""
                )));
            }
        }
        Ok(KeymapFile {
            header: d.qsaccess,
            map: KeyMap { keys: d.keys },
            extra: d.extra,
        })
    }

    /// The `.qkm` text.
    pub fn to_text(&self) -> String {
        let mut header = self.header.clone();
        header.kind = doc::KEYMAP.into();
        header.version = doc::VERSION;
        doc::to_text(&KeymapDoc {
            qsaccess: header,
            keys: self.map.keys.clone(),
            extra: self.extra.clone(),
        })
    }

    /// Reads a `.qkm` file.
    pub fn load(path: &Path) -> Result<KeymapFile, FileError> {
        KeymapFile::parse(&doc::read(path)?)
    }

    /// Writes a `.qkm` file atomically.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        doc::write(path, &self.to_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::temp_dir;

    #[test]
    fn round_trip() {
        let mut map = KeyMap::defaults();
        map.assign("Ctrl+Enter", "FieldExit");
        map.clear_function("Clear");
        let dir = temp_dir("qkm");
        let path = dir.join("mine.qkm");
        KeymapFile::new(map.clone()).save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("[qsaccess]\nkind = \"keymap\"\nversion = 1\n"));
        assert!(text.contains("\"Right Ctrl\" = \"FieldExit\""), "{text}");
        let back = KeymapFile::load(&path).unwrap();
        assert_eq!(back.map, map);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_keys_kept_and_bad_functions_refused() {
        let text = "[qsaccess]\nkind = \"keymap\"\nversion = 1\n\n[keys]\nF1 = \"Help\"\n\n[about]\nauthor = \"ops\"\n";
        let f = KeymapFile::parse(text).unwrap();
        assert_eq!(f.map.function("F1"), Some("Help"));
        assert!(f.to_text().contains("author = \"ops\""));
        assert!(matches!(
            KeymapFile::parse("[qsaccess]\nkind = \"keymap\"\nversion = 1\n[keys]\nF1 = \"Fly\"\n"),
            Err(FileError::Invalid(_))
        ));
        assert!(matches!(
            KeymapFile::parse("[qsaccess]\nkind = \"keymap\"\nversion = 4\n"),
            Err(FileError::Newer(4))
        ));
        assert!(matches!(
            KeymapFile::parse("[qsaccess]\nkind = \"macro\"\nversion = 1\n"),
            Err(FileError::WrongKind { .. })
        ));
    }
}
