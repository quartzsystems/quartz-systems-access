// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QS/400 Access for Desktop.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use fltk::{app, prelude::*};
use qsa_config::settings::Geometry;
use qsa_config::{ConfigDir, Credentials};
use qsaccess::app::{AppState, Shared};
use qsaccess::mainwin::{self, Task, Ui};
use qsaccess::request::Request;
use qsaccess::{about, ipc, log};

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
    qsa_ui::theme::pin_font_rendering();
    let args = parse_args();
    let dir = match &args.config_dir {
        Some(d) => Some(ConfigDir::at(d)),
        None => ConfigDir::default_location(),
    };
    log::init(dir.as_ref().map(|d| d.path().to_owned()));
    // The 5250 crate logs negotiation at debug level; QSACCESS_LOG=trace adds
    // record bytes, QSACCESS_LOG=info leaves negotiation out.
    let level = match std::env::var("QSACCESS_LOG").as_deref() {
        Ok("trace") => tn5250::log::Level::Trace,
        Ok("info") => tn5250::log::Level::Info,
        _ => tn5250::log::Level::Debug,
    };
    tn5250::log::set_sink(level, |l, text| log::line(&format!("{} {text}", l.name())));
    let mut request = Request::parse(&args.rest);
    if request.record.is_none()
        && let Some(p) = std::env::var_os("QSACCESS_RECORD")
    {
        let p = PathBuf::from(p);
        request.record = Some(std::path::absolute(&p).unwrap_or(p));
    }
    if !request.unknown.is_empty() {
        qsaccess::log!("arguments not understood: {:?}", request.unknown);
    }

    // Single instance: a second launch hands its request to the first and
    // exits. Tool modes (sampler, snapshot) always run on their own.
    let tool_mode = args.ui_sampler.is_some() || args.snapshot.is_some();
    let mut server = None;
    if !tool_mode {
        let started = ipc::Endpoint::for_user(dir.as_ref().map(|d| d.path()))
            .and_then(|ep| ipc::start(&ep, &request.to_args()));
        match started {
            Ok(ipc::Startup::Secondary) => {
                qsaccess::log!("handed the request to the running instance; exiting");
                return;
            }
            Ok(ipc::Startup::Primary(s)) => server = Some(s),
            Err(e) => qsaccess::log!("single-instance endpoint not available: {e}"),
        }
    }

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

    if let Some(path) = &args.snapshot {
        win.show();
        snapshot(&state, &win, args.view.as_deref(), path);
        return;
    }

    let (args_tx, args_rx) = std::sync::mpsc::channel::<Vec<String>>();
    let _server = server.map(|s| {
        s.spawn(move |received| {
            let _ = args_tx.send(received);
            app::awake();
        })
    });

    // A double-click opens only what it names; the main window stays hidden
    // until it is asked for.
    let mut main_shown = false;
    if !request.opens_window() {
        win.show();
        main_shown = true;
    }
    mainwin::report_startup_errors(&ui);
    serve(&ui, &state, &request, &mut main_shown);
    while app.wait() {
        while let Some(action) = rx.recv() {
            ui.handle(action);
        }
        qsaccess::session::pump();
        qsaccess::manager::pump();
        qsaccess::macros::editor::pump();
        ui.refresh_recent();
        while let Ok(received) = args_rx.try_recv() {
            qsaccess::log!("request from another instance: {received:?}");
            let r = Request::parse(&received);
            serve(&ui, &state, &r, &mut main_shown);
        }
        if !win.shown()
            && !qsaccess::session::any_open()
            && !qsaccess::manager::shown()
            && !qsaccess::macros::editor::any_open()
        {
            break;
        }
    }

    let mut st = state.borrow_mut();
    if main_shown {
        st.settings.window = Some(Geometry {
            x: win.x(),
            y: win.y(),
            w: win.w(),
            h: win.h(),
        });
    }
    if let Some(name) = ui.selected_system() {
        st.settings.last_system = name;
    }
    if let Err(e) = st.save_settings() {
        qsaccess::log!("settings not saved: {e}");
    }
}

/// Does what a launch asked for. A request that opens nothing raises the
/// main window.
fn serve(ui: &Ui, state: &Shared, r: &Request, main_shown: &mut bool) {
    if let Some(p) = &r.record {
        state.borrow_mut().record_next = Some(p.clone());
    }
    let parent = ui.window.shown().then_some(&ui.window);
    for p in &r.open {
        qsaccess::open::open_path(state, p, parent.map(|w| w as &dyn WindowExt));
    }
    if r.session_manager {
        ui.run(Task::SessionManager);
    }
    if !r.opens_window() {
        ui.window.clone().show();
        *main_shown = true;
    }
}

/// Draws one view for a screenshot and writes it to `path`.
fn snapshot(state: &Shared, win: &fltk::window::Window, view: Option<&str>, path: &Path) {
    match view {
        Some("about") => {
            let mut a = about::build();
            qsa_ui::propdialog::center_on(&mut a, Some(win));
            a.show();
            snap(&a, path);
        }
        Some("sysconfig") => {
            let mut d = qsaccess::sysconfig::build(state);
            qsa_ui::propdialog::center_on(&mut d.window, Some(win));
            d.window.show();
            snap(&d.window, path);
        }
        Some(v) if v.starts_with("properties") => {
            let tab = v
                .rsplit(':')
                .next()
                .and_then(|t| t.parse().ok())
                .unwrap_or(0);
            let sys = state
                .borrow()
                .systems
                .as_slice()
                .first()
                .cloned()
                .unwrap_or_default();
            let title = format!("System Properties - {}", sys.name);
            let (mut d, _form) = qsaccess::properties::build(&title, &sys);
            d.tabs().select(tab);
            d.place(Some(win));
            let mut w = d.window().clone();
            w.show();
            snap(&w, path);
        }
        _ => snap(win, path),
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
