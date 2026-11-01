// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! File choosers. These use FLTK's own chooser, drawn with the house look,
//! rather than the desktop's native dialog.

use std::path::{Path, PathBuf};

use fltk::app;
use fltk::dialog::{FileChooser, FileChooserType};

/// Session files, for choosers.
pub const SESSION_PATTERN: &str = "Session Files (*.qsd)\tAll Files (*)";
/// Keyboard map files, for choosers.
pub const KEYMAP_PATTERN: &str = "Keyboard Map Files (*.qkm)\tAll Files (*)";
/// Macro files, for choosers.
pub const MACRO_PATTERN: &str = "Macro Files (*.qsm)\tAll Files (*)";
/// Every file QS/400 Access for Desktop opens, for File > Open.
pub const ANY_PATTERN: &str = "QS/400 Access Files (*.{qsd,qkm,qsm})\tSession Files (*.qsd)\tKeyboard Map Files (*.qkm)\tMacro Files (*.qsm)\tAll Files (*)";

/// The directory a chooser opens in: the user's home, else the current one.
fn start_dir() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// The Sessions folder, created if it is missing; else the home directory.
pub fn sessions_dir() -> PathBuf {
    match qsa_config::store::default_sessions_dir() {
        Some(d) if std::fs::create_dir_all(&d).is_ok() => d,
        _ => start_dir(),
    }
}

fn run(mut chooser: FileChooser) -> Option<PathBuf> {
    chooser.show();
    while chooser.shown() {
        app::wait();
    }
    chooser.value(1).map(PathBuf::from)
}

/// Asks for an existing file. `pattern` is in FLTK form, for example
/// `"Configurations (*.qsc)\tAll Files (*)"`.
pub fn choose_open(title: &str, pattern: &str) -> Option<PathBuf> {
    choose_open_in(&start_dir(), title, pattern)
}

/// Asks for an existing file, starting in `dir`.
pub fn choose_open_in(dir: &Path, title: &str, pattern: &str) -> Option<PathBuf> {
    run(FileChooser::new(
        dir,
        pattern,
        FileChooserType::Single,
        title,
    ))
}

/// Asks for a file to write, suggesting `name`.
pub fn choose_save(title: &str, pattern: &str, name: &str) -> Option<PathBuf> {
    choose_save_in(&start_dir(), title, pattern, name)
}

/// Asks for a file to write in `dir`, suggesting `name`.
pub fn choose_save_in(dir: &Path, title: &str, pattern: &str, name: &str) -> Option<PathBuf> {
    let mut chooser = FileChooser::new(dir, pattern, FileChooserType::Create, title);
    chooser.set_value(name);
    run(chooser)
}

/// `path` with extension `ext` if it has none.
pub fn with_extension(path: PathBuf, ext: &str) -> PathBuf {
    if path.extension().is_some() {
        path
    } else {
        path.with_extension(ext)
    }
}

/// A file name made from `title`: characters that are not allowed in file
/// names on either platform become `_`.
pub fn file_name_for(title: &str, ext: &str) -> String {
    let stem: String = title
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let stem = stem.trim_matches(['.', ' ']);
    let stem = if stem.is_empty() { "Session" } else { stem };
    format!("{stem}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        assert_eq!(file_name_for("Payroll", "qsd"), "Payroll.qsd");
        assert_eq!(file_name_for(" a/b:c? ", "qsd"), "a_b_c_.qsd");
        assert_eq!(file_name_for("..", "qsd"), "Session.qsd");
        assert_eq!(
            with_extension(PathBuf::from("x"), "qsd"),
            PathBuf::from("x.qsd")
        );
        assert_eq!(
            with_extension(PathBuf::from("x.txt"), "qsd"),
            PathBuf::from("x.txt")
        );
    }
}
