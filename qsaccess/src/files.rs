// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! File choosers. These use FLTK's own chooser, drawn with the house look,
//! rather than the desktop's native dialog.

use std::path::PathBuf;

use fltk::app;
use fltk::dialog::{FileChooser, FileChooserType};

/// The directory a chooser opens in: the user's home, else the current one.
fn start_dir() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("."))
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
    run(FileChooser::new(
        start_dir(),
        pattern,
        FileChooserType::Single,
        title,
    ))
}

/// Asks for a file to write, suggesting `name`.
pub fn choose_save(title: &str, pattern: &str, name: &str) -> Option<PathBuf> {
    let mut chooser = FileChooser::new(start_dir(), pattern, FileChooserType::Create, title);
    chooser.set_value(name);
    run(chooser)
}
