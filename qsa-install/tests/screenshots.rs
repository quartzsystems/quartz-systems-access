// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Screenshot tests of the installer: every page of the wizard, and the
//! Uninstall and Repair dialogs, against the goldens in
//! `tests/goldens/<platform>[-<tag>]/`. See `qsa_ui::goldens`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fltk::window::Window;
use qsa_install::install::Report;
use qsa_install::ui::{self, Outcome, PAGES, Wizard};
use qsa_ui::goldens::Goldens;

/// The install folder the pages show: the same on every machine.
fn folder() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Users\user\AppData\Local\Programs\Quartz Systems\Access")
    } else {
        PathBuf::from("/home/user/.local/opt/quartz-access")
    }
}

fn page(i: usize) -> Window {
    let mut w = Wizard::build(None);
    w.set_location(&folder());
    match PAGES[i].0 {
        "progress" => w.set_progress(0.45, "Copying qsaccess"),
        "finish" => w.show_outcome(&Outcome::Done(Report {
            warnings: Vec::new(),
            upgraded_from: None,
            dir: folder(),
        })),
        _ => {}
    }
    w.show_page(i);
    w.window
}

fn main() -> ExitCode {
    if Goldens::listing("screenshots") {
        return ExitCode::SUCCESS;
    }
    let Some((_app, mut g)) = Goldens::start(Path::new(env!("CARGO_MANIFEST_DIR"))) else {
        return ExitCode::FAILURE;
    };
    for (i, (name, ..)) in PAGES.iter().enumerate() {
        g.check(&format!("wizard-{i}-{name}"), page(i));
    }
    let sessions = if cfg!(windows) {
        r"C:\Users\user\Documents\Quartz Access\Sessions"
    } else {
        "/home/user/Documents/Quartz Access/Sessions"
    };
    g.check(
        "uninstall",
        ui::build_uninstall_with(&folder(), sessions, false)
            .0
            .window,
    );
    g.check(
        "repair",
        ui::build_uninstall_with(&folder(), sessions, true).0.window,
    );
    g.finish()
}
