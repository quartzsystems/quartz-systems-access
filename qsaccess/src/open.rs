// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Opening a file, whatever opened it: File > Open, Recent Sessions,
//! Session Manager, or `qsaccess open <file>` (a double-click), which a
//! second instance hands to the running one.
//!
//! - `.qsd`: a session window, connected if the file says so.
//! - `.qsp`: a printer window, likewise.
//! - `.qkm`: Load Keyboard Map, into an open session or new sessions.
//! - `.qsm`: the Macro editor.
//! - `.qdx`: a Data Transfer window with the request, ready to run.
//!
//! A file with another extension is recognised by its `[qsaccess]` kind.

use std::path::Path;

use fltk::prelude::*;
use qsa_config::doc::{self, FileError};
use qsa_config::{qkm, qsd, qsm, qsp};

use crate::app::Shared;

/// What a file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Session,
    Printer,
    Keymap,
    Macro,
    Transfer,
}

/// The kind of file at `path`: by extension, else by its header.
pub fn kind_of(path: &Path) -> Result<Kind, FileError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match ext.as_deref() {
        Some(qsd::EXTENSION) => return Ok(Kind::Session),
        Some(qsp::EXTENSION) => return Ok(Kind::Printer),
        Some(qkm::EXTENSION) => return Ok(Kind::Keymap),
        Some(qsm::EXTENSION) => return Ok(Kind::Macro),
        Some(qsa_transfer::request::EXTENSION) => return Ok(Kind::Transfer),
        _ => {}
    }
    let text = doc::read(path)?;
    for (kind, k) in [
        (doc::DISPLAY, Kind::Session),
        (doc::PRINTER, Kind::Printer),
        (doc::KEYMAP, Kind::Keymap),
        (doc::MACRO, Kind::Macro),
        (qsa_transfer::request::KIND, Kind::Transfer),
    ] {
        match doc::check_header(&text, kind) {
            Ok(()) => return Ok(k),
            Err(FileError::WrongKind { .. }) => continue,
            Err(e) => return Err(e),
        }
    }
    Err(FileError::Invalid(
        "it is not a session, keyboard map, macro or transfer request file".into(),
    ))
}

/// Opens `path` as its kind says. Problems are shown to the user.
pub fn open_path(state: &Shared, path: &Path, parent: Option<&dyn WindowExt>) {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    crate::log!("opening {}", path.display());
    match kind_of(&path) {
        Ok(Kind::Session) => {
            crate::session::open_file(state, &path, parent);
        }
        Ok(Kind::Printer) => {
            crate::printer::open_file(state, &path, parent);
        }
        Ok(Kind::Keymap) => crate::session::loadmap::run(state, &path, parent),
        Ok(Kind::Macro) => crate::macros::open_file(state, &path, parent),
        Ok(Kind::Transfer) => crate::transfer::open_request(state, &path, parent),
        Err(e) => {
            let m = e.message(&path);
            crate::log!("{m}");
            qsa_ui::msgdetails::show_message(parent, &m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension_then_header() {
        let dir = std::env::temp_dir().join(format!("qsa-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(kind_of(Path::new("a.QSD")).unwrap(), Kind::Session);
        assert_eq!(kind_of(Path::new("a.qkm")).unwrap(), Kind::Keymap);
        assert_eq!(kind_of(Path::new("a.qsm")).unwrap(), Kind::Macro);
        assert_eq!(kind_of(Path::new("a.QSP")).unwrap(), Kind::Printer);
        assert_eq!(kind_of(Path::new("a.qdx")).unwrap(), Kind::Transfer);
        let p = dir.join("mail-attachment.txt");
        std::fs::write(&p, "[qsaccess]\nkind = \"macro\"\nversion = 1\n").unwrap();
        assert_eq!(kind_of(&p).unwrap(), Kind::Macro);
        std::fs::write(&p, "[qsaccess]\nkind = \"printer\"\nversion = 1\n").unwrap();
        assert_eq!(kind_of(&p).unwrap(), Kind::Printer);
        std::fs::write(&p, "[qsaccess]\nkind = \"display\"\nversion = 7\n").unwrap();
        assert!(matches!(kind_of(&p), Err(FileError::Newer(7))));
        std::fs::write(&p, "[qsaccess]\nkind = \"systems\"\nversion = 1\n").unwrap();
        assert!(matches!(kind_of(&p), Err(FileError::Invalid(_))));
        let _ = std::fs::remove_dir_all(dir);
    }
}
