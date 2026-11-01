// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! File choosers: the desktop's own Open, Save and folder dialogs.

use std::path::{Path, PathBuf};

use fltk::dialog::{NativeFileChooser, NativeFileChooserOptions, NativeFileChooserType};

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

/// `pattern`, `"Name (*.ext)\tAll Files (*)"`, as the native dialog's
/// filter: one `Name\t*.ext` line per entry.
fn native_filter(pattern: &str) -> String {
    pattern
        .split('\t')
        .map(|f| match f.rsplit_once(" (") {
            Some((name, glob)) => format!("{name}\t{}", glob.trim_end_matches(')')),
            None => f.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn run(
    kind: NativeFileChooserType,
    dir: &Path,
    title: &str,
    pattern: Option<&str>,
    name: Option<&str>,
) -> Option<PathBuf> {
    let mut d = NativeFileChooser::new(kind);
    d.set_title(title);
    d.set_option(NativeFileChooserOptions::NewFolder);
    // The nearest folder that exists, since the dialog ignores one that
    // does not (an install location not made yet, say).
    let start = dir.ancestors().find(|p| p.is_dir()).unwrap_or(dir);
    let _ = d.set_directory(&start);
    if let Some(p) = pattern {
        d.set_filter(&native_filter(p));
    }
    if let Some(n) = name {
        d.set_preset_file(n);
    }
    d.show();
    let path = d.filename();
    (!path.as_os_str().is_empty()).then_some(path)
}

/// Asks for an existing file. `pattern` is in FLTK form, for example
/// `"Configurations (*.qsc)\tAll Files (*)"`.
pub fn choose_open(title: &str, pattern: &str) -> Option<PathBuf> {
    choose_open_in(&start_dir(), title, pattern)
}

/// Asks for an existing file, starting in `dir`.
pub fn choose_open_in(dir: &Path, title: &str, pattern: &str) -> Option<PathBuf> {
    run(
        NativeFileChooserType::BrowseFile,
        dir,
        title,
        Some(pattern),
        None,
    )
}

/// Asks for a file to write, suggesting `name`.
pub fn choose_save(title: &str, pattern: &str, name: &str) -> Option<PathBuf> {
    choose_save_in(&start_dir(), title, pattern, name)
}

/// Asks for a file to write in `dir`, suggesting `name`. The caller asks
/// before replacing a file, once it has added any missing extension.
pub fn choose_save_in(dir: &Path, title: &str, pattern: &str, name: &str) -> Option<PathBuf> {
    run(
        NativeFileChooserType::BrowseSaveFile,
        dir,
        title,
        Some(pattern),
        Some(name),
    )
}

/// Asks for a folder, starting in `dir`.
pub fn choose_folder(dir: &Path, title: &str) -> Option<PathBuf> {
    run(NativeFileChooserType::BrowseDir, dir, title, None, None)
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
    fn filters_in_native_form() {
        assert_eq!(
            native_filter(SESSION_PATTERN),
            "Session Files\t*.qsd\nAll Files\t*"
        );
        assert_eq!(
            native_filter("Certificates (*.{pem,crt,cer})\tAll Files (*)"),
            "Certificates\t*.{pem,crt,cer}\nAll Files\t*"
        );
    }

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
