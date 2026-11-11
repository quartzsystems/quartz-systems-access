// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Macro editor knows when it holds unsaved changes, which is when its
//! Close asks before dropping them: a macro opened from a file and not
//! touched has none, an edit makes some and undoing it takes them away, and
//! a macro with steps and no file yet (one just recorded) always has.
//!
//! The editor is a real window (never shown), so this runs as a plain
//! program on FLTK's one UI thread (`harness = false`).

use std::path::PathBuf;
use std::process::ExitCode;

use qsa_config::qsm::MacroFile;
use qsaccess::macros::editor;

fn run() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/macros/sign-on-wrkactjob-f3.qsm");
    let m = MacroFile::load(&path).expect("fixture macro");
    assert!(!m.steps.is_empty());
    let name = m.info.name.clone();

    // Opened from its file: nothing to save until something is changed.
    let e = editor::build(m.clone(), Some(path.clone()));
    assert!(!e.changed());
    e.set_name("Another name");
    assert!(e.changed());
    e.set_name(&name);
    assert!(!e.changed(), "the change was undone");
    // Spaces round a name are not a change: they are not saved.
    e.set_name(&format!("  {name} "));
    assert!(!e.changed());

    // The same macro with no file yet, as the recorder hands it over.
    let recorded = editor::build(m.clone(), None);
    assert!(recorded.changed());

    // A new macro with nothing in it has nothing to lose.
    let mut blank = m;
    blank.steps.clear();
    let new = editor::build(blank, None);
    assert!(!new.changed());
    new.set_name("Started");
    assert!(new.changed());

    // What Close asks, and that Escape is Cancel.
    let (win, _) = editor::unsaved_box("Sign on").build();
    drop(win);
}

fn main() -> ExitCode {
    if std::env::args().any(|a| a == "--list") {
        println!("macro_editor: test");
        return ExitCode::SUCCESS;
    }
    qsa_ui::theme::pin_font_rendering();
    let _app = fltk::app::App::default();
    if let Err(e) = qsa_ui::theme::init() {
        eprintln!("macro_editor: fonts not loaded: {e}");
        return ExitCode::FAILURE;
    }
    run();
    println!("ok       macro_editor");
    ExitCode::SUCCESS
}
