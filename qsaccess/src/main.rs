// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QS/400 Access for Desktop.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use fltk::{app, prelude::*};
use qsa_config::settings::Geometry;
use qsa_config::{ConfigDir, Credentials};
use qsaccess::app::AppState;
use qsaccess::{about, log, mainwin};

struct Args {
    ui_sampler: Option<String>,
    snapshot: Option<PathBuf>,
    view: Option<String>,
    config_dir: Option<PathBuf>,
    rest: Vec<String>,
}

fn parse_args() -> Args {
    let mut args = Args {
        ui_sampler: None,
        snapshot: None,
        view: None,
        config_dir: None,
        rest: Vec::new(),
    };
    let mut it = std::env::args().skip(1).peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--ui-sampler" => {
                let part = it.next_if(|p| !p.starts_with("--"));
                args.ui_sampler = Some(part.unwrap_or_default());
            }
            "--snapshot" => args.snapshot = it.next().map(PathBuf::from),
            "--view" => args.view = it.next(),
            "--config-dir" => args.config_dir = it.next().map(PathBuf::from),
            _ => args.rest.push(a),
        }
    }
    args
}

fn main() {
    let args = parse_args();
    let dir = match &args.config_dir {
        Some(d) => Some(ConfigDir::at(d)),
        None => ConfigDir::default_location(),
    };
    log::init(dir.as_ref().map(|d| d.path().to_owned()));

    let app = app::App::default();
    if let Err(e) = qsa_ui::theme::init() {
        qsaccess::log!("fonts not loaded: {e}");
    }

    if let Some(part) = &args.ui_sampler {
        let m = qsa_msg::message(qsa_msg::id::NOT_AVAILABLE, &[]);
        let mut win = match part.as_str() {
            "details" => qsa_ui::msgdetails::details_window(&m),
            "message" => qsa_ui::msgdetails::message_box(&m).build().0,
            _ => qsa_ui::sampler::build(),
        };
        qsa_ui::propdialog::center_on(&mut win, None);
        win.show();
        if let Some(path) = &args.snapshot {
            snap(&win, path);
            return;
        }
        app.run().unwrap();
        return;
    }

    let state = AppState::load(dir, Credentials::platform()).shared();
    let (ui, rx) = mainwin::build(state.clone());
    let mut win = ui.window.clone();
    match state.borrow().settings.window {
        Some(g) if args.snapshot.is_none() => win.resize(g.x, g.y, g.w, g.h),
        _ => qsa_ui::propdialog::center_on(&mut win, None),
    }
    win.show();

    if let Some(path) = &args.snapshot {
        match args.view.as_deref() {
            Some("about") => {
                let mut a = about::build();
                qsa_ui::propdialog::center_on(&mut a, Some(&win));
                a.show();
                snap(&a, path);
            }
            _ => snap(&win, path),
        }
        return;
    }

    mainwin::report_startup_errors(&ui);
    while app.wait() {
        while let Some(action) = rx.recv() {
            ui.handle(action);
        }
        if !win.shown() {
            break;
        }
    }

    let mut st = state.borrow_mut();
    st.settings.window = Some(Geometry {
        x: win.x(),
        y: win.y(),
        w: win.w(),
        h: win.h(),
    });
    if let Some(name) = ui.selected_system() {
        st.settings.last_system = name;
    }
    if let Err(e) = st.save_settings() {
        qsaccess::log!("settings not saved: {e}");
    }
}

/// Lets the window draw, writes it to `path` as PNG, and returns.
fn snap(win: &fltk::window::Window, path: &Path) {
    for _ in 0..20 {
        app::wait_for(0.02).ok();
    }
    let img = qsa_ui::snapshot::capture(win);
    if let Err(e) = qsa_ui::snapshot::write_png(path, &img) {
        qsaccess::log!("{}: {e}", path.display());
        std::process::exit(1);
    }
}
