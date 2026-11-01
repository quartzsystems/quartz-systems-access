// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! What session (`.qsd`), keyboard map (`.qkm`) and macro (`.qsm`) files
//! have in common.
//!
//! Each is TOML whose first table is the header:
//!
//! ```toml
//! [qsaccess]
//! kind = "display"   # or "keymap", "macro"
//! version = 1
//! ```
//!
//! - `version` is the format's major version. A file with a higher one is
//!   refused (`QAD2002`) before anything else in it is looked at.
//! - Keys this release does not know are kept: every table carries them in
//!   an `extra` map and writes them back on save.
//! - Writes are atomic (see [`crate::store::write_atomic`]).
//! - Paths inside a file are written relative to the file's folder when
//!   they can be, with `/` separators, and resolved against it on reading.

use std::io;
use std::path::{Component, Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::store::write_atomic;

/// Keys a table holds that this release does not know, kept for writing
/// back.
pub type Extra = toml::Table;

/// `kind` of a display session file.
pub const DISPLAY: &str = "display";
/// `kind` of a keyboard map file.
pub const KEYMAP: &str = "keymap";
/// `kind` of a macro file.
pub const MACRO: &str = "macro";
/// The format version this release reads and writes, for every kind.
pub const VERSION: u32 = 1;

/// The `[qsaccess]` table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub kind: String,
    pub version: u32,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Header {
    /// The header this release writes for `kind`.
    pub fn new(kind: &str) -> Header {
        Header {
            kind: kind.to_owned(),
            version: VERSION,
            extra: Extra::new(),
        }
    }
}

/// Why a file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Parse(String),
    #[error("the file has no [qsaccess] table with a kind and version")]
    NoHeader,
    #[error("the file holds \"{found}\", not {expected}")]
    WrongKind {
        found: String,
        expected: &'static str,
    },
    #[error("the file is format version {0}")]
    Newer(i64),
    #[error("{0}")]
    Invalid(String),
}

impl FileError {
    /// The message to show for `path`: `QAD2002` for a newer format,
    /// `QAD2001` otherwise.
    pub fn message(&self, path: &Path) -> qsa_msg::Message {
        let p = path.display().to_string();
        match self {
            FileError::Newer(v) => {
                qsa_msg::message(qsa_msg::id::SESSION_FILE_NEWER, &[&p, &v.to_string()])
            }
            other => {
                let why = other.to_string();
                let why = why.trim().trim_end_matches('.');
                qsa_msg::message(qsa_msg::id::SESSION_FILE_UNREADABLE, &[&p, why])
            }
        }
    }
}

/// A human name for a kind, for "not a ..." errors.
fn kind_name(kind: &str) -> &'static str {
    match kind {
        DISPLAY => "a display session",
        KEYMAP => "a keyboard map",
        MACRO => "a macro",
        _ => "this kind of file",
    }
}

/// Reads the header of `text` only: its kind and version, checked against
/// `kind` and [`VERSION`]. Nothing else in the file is looked at, so a file
/// from a newer release is refused for its version, not for some key it
/// has changed.
pub fn check_header(text: &str, kind: &str) -> Result<(), FileError> {
    let table: toml::Table =
        toml::from_str(text).map_err(|e| FileError::Parse(e.message().trim().to_owned()))?;
    let header = table
        .get("qsaccess")
        .and_then(toml::Value::as_table)
        .ok_or(FileError::NoHeader)?;
    let found = header
        .get("kind")
        .and_then(toml::Value::as_str)
        .ok_or(FileError::NoHeader)?;
    let version = header
        .get("version")
        .and_then(toml::Value::as_integer)
        .ok_or(FileError::NoHeader)?;
    if found != kind {
        return Err(FileError::WrongKind {
            found: found.to_owned(),
            expected: kind_name(kind),
        });
    }
    if version > i64::from(VERSION) {
        return Err(FileError::Newer(version));
    }
    if version < 1 {
        return Err(FileError::Invalid(format!(
            "format version {version} is not valid"
        )));
    }
    Ok(())
}

/// Parses a file of `kind`: header first, then the whole file as `T`.
pub fn parse<T: DeserializeOwned>(text: &str, kind: &str) -> Result<T, FileError> {
    check_header(text, kind)?;
    toml::from_str(text).map_err(|e| FileError::Parse(e.message().trim().to_owned()))
}

/// The TOML text for `value`.
pub fn to_text<T: Serialize>(value: &T) -> String {
    toml::to_string_pretty(value).expect("file types always serialise")
}

/// Reads a whole file. A UTF-8 byte-order mark, as some editors write, is
/// dropped.
pub fn read(path: &Path) -> Result<String, FileError> {
    let text = std::fs::read_to_string(path)?;
    Ok(match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_owned(),
        None => text,
    })
}

/// Writes `text` to `path` atomically.
pub fn write(path: &Path, text: &str) -> io::Result<()> {
    write_atomic(path, text.as_bytes())
}

/// The folder paths in a file at `file` are relative to.
pub fn base_dir(file: &Path) -> PathBuf {
    let dir = file.parent().unwrap_or(Path::new(""));
    std::path::absolute(dir).unwrap_or_else(|_| dir.to_owned())
}

fn same(a: &Component, b: &Component) -> bool {
    if cfg!(windows) {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    } else {
        a == b
    }
}

/// `path` as written into a file whose folder is `base`: relative to `base`
/// (with `..` where needed) when both are on the same root, else as given.
/// Separators are always `/`.
pub fn relative(base: &Path, path: &Path) -> String {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let a: Vec<Component> = abs.components().collect();
    let b: Vec<Component> = base.components().collect();
    let shares_root = matches!((a.first(), b.first()), (Some(x), Some(y)) if same(x, y));
    if !path.is_absolute() || !shares_root {
        return slashes(path);
    }
    let common = a.iter().zip(&b).take_while(|(x, y)| same(x, y)).count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..b.len() {
        parts.push("..".into());
    }
    for c in &a[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

fn slashes(p: &Path) -> String {
    let s = p.to_string_lossy();
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s.into_owned()
    }
}

/// A path read from a file whose folder is `base`: relative ones are taken
/// from `base`, with `.` and `..` resolved.
pub fn resolve(base: &Path, written: &str) -> PathBuf {
    let p = Path::new(written);
    if p.is_absolute() {
        return p.to_owned();
    }
    let mut out = base.to_owned();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_checks() {
        let ok = "[qsaccess]\nkind = \"display\"\nversion = 1\n";
        assert!(check_header(ok, DISPLAY).is_ok());
        assert!(matches!(
            check_header(ok, MACRO),
            Err(FileError::WrongKind { found, .. }) if found == "display"
        ));
        assert!(matches!(
            check_header("[qsaccess]\nkind = \"display\"\nversion = 2\n", DISPLAY),
            Err(FileError::Newer(2))
        ));
        assert!(matches!(
            check_header("[qsaccess]\nkind = \"display\"\nversion = 0\n", DISPLAY),
            Err(FileError::Invalid(_))
        ));
        assert!(matches!(
            check_header("[system]\nname = \"a\"\n", DISPLAY),
            Err(FileError::NoHeader)
        ));
        assert!(matches!(
            check_header("[qsaccess\n", DISPLAY),
            Err(FileError::Parse(_))
        ));
        // A newer file is refused for its version even when the rest of it
        // would not parse as this release's format.
        let newer = "[qsaccess]\nkind = \"display\"\nversion = 3\n[session]\nscreen_size = 7\n";
        assert!(matches!(
            check_header(newer, DISPLAY),
            Err(FileError::Newer(3))
        ));
    }

    #[test]
    fn a_byte_order_mark_is_dropped() {
        let p = std::env::temp_dir().join(format!("qsa-bom-{}.qsd", std::process::id()));
        std::fs::write(&p, "\u{feff}[qsaccess]\nkind = \"display\"\nversion = 1\n").unwrap();
        let text = read(&p).unwrap();
        assert!(check_header(&text, DISPLAY).is_ok());
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn messages() {
        let p = Path::new("a.qsd");
        assert_eq!(FileError::Newer(2).message(p).id, "QAD2002");
        assert_eq!(
            FileError::Newer(2).message(p).text,
            "Session file a.qsd was made by a newer version (2)."
        );
        let m = FileError::Invalid("CCSID 9 is not supported.".into()).message(p);
        assert_eq!(m.id, "QAD2001");
        assert_eq!(
            m.text,
            "Session file a.qsd could not be read: CCSID 9 is not supported."
        );
    }

    #[cfg(windows)]
    #[test]
    fn relative_paths_windows() {
        let base = Path::new(r"C:\Users\u\Sessions");
        assert_eq!(
            relative(base, Path::new(r"C:\Users\u\Sessions\m.qsm")),
            "m.qsm"
        );
        assert_eq!(
            relative(base, Path::new(r"c:\users\u\Macros\m.qsm")),
            "../Macros/m.qsm"
        );
        assert_eq!(relative(base, Path::new(r"D:\m.qsm")), "D:/m.qsm");
        assert_eq!(
            resolve(base, "../Macros/m.qsm"),
            Path::new(r"C:\Users\u\Macros\m.qsm")
        );
        assert_eq!(resolve(base, "D:/m.qsm"), Path::new("D:/m.qsm"));
    }

    #[cfg(unix)]
    #[test]
    fn relative_paths_unix() {
        let base = Path::new("/home/u/Sessions");
        assert_eq!(relative(base, Path::new("/home/u/Sessions/m.qsm")), "m.qsm");
        assert_eq!(
            relative(base, Path::new("/home/u/Macros/a/m.qsm")),
            "../Macros/a/m.qsm"
        );
        assert_eq!(relative(base, Path::new("/m.qsm")), "../../../m.qsm");
        assert_eq!(
            resolve(base, "../Macros/m.qsm"),
            Path::new("/home/u/Macros/m.qsm")
        );
        assert_eq!(resolve(base, "/x/m.qsm"), Path::new("/x/m.qsm"));
    }
}
