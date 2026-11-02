// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! `.qsc` files: system configurations exported for another computer.
//!
//! A `.qsc` file is TOML with a `[qsaccess]` header (`kind = "systems"`,
//! `version = 1`) and a `[[system]]` table per system. It never holds a
//! password: the model has no password field to write.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::store::write_atomic;
use crate::system::{Invalid, System};
use crate::systems::Systems;

/// File extension, without the dot.
pub const EXTENSION: &str = "qsc";
/// The `kind` this module reads and writes.
pub const KIND: &str = "systems";
/// The `version` this module reads and writes.
pub const VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct Header {
    kind: String,
    version: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct QscFile {
    qsaccess: Header,
    #[serde(default, rename = "system")]
    systems: Vec<System>,
}

/// Why a `.qsc` file could not be imported.
#[derive(Debug, thiserror::Error)]
pub enum QscError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("The file is not valid: {0}")]
    Parse(String),
    #[error("The file holds \"{0}\", not system configurations.")]
    WrongKind(String),
    #[error("The file is version {0}; this program reads version {VERSION}.")]
    WrongVersion(u32),
    #[error("System {name} in the file is not valid: {why}")]
    BadSystem { name: String, why: Invalid },
}

/// The `.qsc` text for `systems`.
pub fn to_string(systems: &[System]) -> String {
    let file = QscFile {
        qsaccess: Header {
            kind: KIND.to_owned(),
            version: VERSION,
        },
        systems: systems.to_vec(),
    };
    toml::to_string_pretty(&file).expect("systems always serialise")
}

/// Parses `.qsc` text and validates every system in it.
pub fn from_str(text: &str) -> Result<Vec<System>, QscError> {
    let file: QscFile = toml::from_str(text).map_err(|e| QscError::Parse(e.message().into()))?;
    if file.qsaccess.kind != KIND {
        return Err(QscError::WrongKind(file.qsaccess.kind));
    }
    if file.qsaccess.version != VERSION {
        return Err(QscError::WrongVersion(file.qsaccess.version));
    }
    for s in &file.systems {
        s.validate().map_err(|why| QscError::BadSystem {
            name: s.name.clone(),
            why,
        })?;
    }
    Ok(file.systems)
}

/// Writes `systems` to a `.qsc` file, atomically.
pub fn export(path: &Path, systems: &[System]) -> io::Result<()> {
    write_atomic(path, to_string(systems).as_bytes())
}

/// Reads a `.qsc` file.
pub fn import(path: &Path) -> Result<Vec<System>, QscError> {
    from_str(&std::fs::read_to_string(path)?)
}

/// What a merge did, by system name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MergeReport {
    pub added: Vec<String>,
    pub replaced: Vec<String>,
    pub skipped: Vec<String>,
}

impl MergeReport {
    /// Systems added or replaced.
    pub fn imported(&self) -> usize {
        self.added.len() + self.replaced.len()
    }
}

/// Merges imported systems into `target`. For a system whose name is already
/// taken, `overwrite(existing, incoming)` decides: true replaces it, false
/// keeps the existing one.
pub fn merge(
    target: &mut Systems,
    incoming: Vec<System>,
    mut overwrite: impl FnMut(&System, &System) -> bool,
) -> MergeReport {
    let mut report = MergeReport::default();
    for sys in incoming {
        match target.get(&sys.name) {
            Some(existing) => {
                if overwrite(existing, &sys) {
                    report.replaced.push(sys.name.clone());
                    target.upsert(sys);
                } else {
                    report.skipped.push(sys.name.clone());
                }
            }
            None => {
                report.added.push(sys.name.clone());
                target.upsert(sys);
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{full_system, temp_dir};

    #[test]
    fn round_trip_without_loss() {
        let systems = vec![full_system("a.example.net"), System::new("b")];
        let dir = temp_dir("qsc");
        let path = dir.join("out.qsc");
        export(&path, &systems).unwrap();
        assert_eq!(import(&path).unwrap(), systems);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("[qsaccess]\nkind = \"systems\"\nversion = 1\n"));
        assert!(!text.to_lowercase().contains("password"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_wrong_header_and_bad_systems() {
        assert!(matches!(
            from_str("[qsaccess]\nkind = \"sessions\"\nversion = 1\n"),
            Err(QscError::WrongKind(k)) if k == "sessions"
        ));
        assert!(matches!(
            from_str("[qsaccess]\nkind = \"systems\"\nversion = 2\n"),
            Err(QscError::WrongVersion(2))
        ));
        assert!(matches!(
            from_str("[[system]]\nname = \"a\"\n"),
            Err(QscError::Parse(_))
        ));
        assert!(matches!(
            from_str(
                "[qsaccess]\nkind = \"systems\"\nversion = 1\n[[system]]\nname = \"bad name\"\n"
            ),
            Err(QscError::BadSystem { .. })
        ));
        assert_eq!(
            from_str("[qsaccess]\nkind = \"systems\"\nversion = 1\n").unwrap(),
            vec![]
        );
    }

    #[test]
    fn unknown_password_field_is_not_kept() {
        let text = "[qsaccess]\nkind = \"systems\"\nversion = 1\n[[system]]\nname = \"a\"\ndefault_user = \"U\"\npassword = \"hunter2\"\n";
        let systems = from_str(text).unwrap();
        assert!(!to_string(&systems).contains("hunter2"));
    }

    #[test]
    fn merge_asks_before_overwriting() {
        let mut target = Systems::new();
        target.upsert(System {
            description: "old".into(),
            ..System::new("A")
        });
        target.upsert(System::new("keep"));
        let incoming = vec![
            System {
                description: "new".into(),
                ..System::new("a")
            },
            System::new("keep"),
            System::new("fresh"),
        ];
        let mut asked = Vec::new();
        let report = merge(&mut target, incoming, |old, new| {
            asked.push((old.name.clone(), new.name.clone()));
            new.name == "a"
        });
        assert_eq!(
            asked,
            [("A".into(), "a".into()), ("keep".into(), "keep".into())]
        );
        assert_eq!(report.added, ["fresh"]);
        assert_eq!(report.replaced, ["a"]);
        assert_eq!(report.skipped, ["keep"]);
        assert_eq!(report.imported(), 2);
        assert_eq!(target.get("a").unwrap().description, "new");
        assert_eq!(target.len(), 3);
    }
}
