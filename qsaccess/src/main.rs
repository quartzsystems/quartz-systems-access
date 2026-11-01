// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QS/400 Access for Desktop.

#![cfg_attr(windows, windows_subsystem = "windows")]

use fltk::{app, prelude::*, window::Window};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let snapshot = args
        .iter()
        .position(|a| a == "--snapshot")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);
    let app = app::App::default();
    if let Err(e) = qsa_ui::theme::init() {
        eprintln!("qsaccess: fonts not loaded: {e}");
    }
    if args.iter().any(|a| a == "--ui-sampler") {
        let mut win = qsa_ui::sampler::build();
        qsa_ui::propdialog::center_on(&mut win, None);
        win.show();
        if let Some(path) = snapshot {
            snap(&win, &path);
            return;
        }
        app.run().unwrap();
    }
}

/// Lets the window draw, writes it to `path` as PNG, and returns.
fn snap(win: &Window, path: &std::path::Path) {
    for _ in 0..20 {
        app::wait_for(0.02).ok();
    }
    let img = qsa_ui::snapshot::capture(win);
    if let Err(e) = qsa_ui::snapshot::write_png(path, &img) {
        eprintln!("qsaccess: {}: {e}", path.display());
        std::process::exit(1);
    }
}
