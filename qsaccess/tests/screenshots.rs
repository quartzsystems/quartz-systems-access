// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Screenshot tests.
//!
//! Renders the main window, System Configurations and every tab of the
//! system properties dialog at 100 % scale from a fixed set of systems, and
//! compares each with the committed golden for this platform in
//! `tests/goldens/<platform>[-<QSA_SCREENSHOT_TAG>]/` (Linux keeps separate
//! goldens for X11 and for Wayland, whose text antialiasing differs slightly).
//!
//! - A pixel differs when any channel is off by more than the tolerance
//!   (`QSA_SCREENSHOT_TOLERANCE`, default 40); a view fails when more than
//!   `QSA_SCREENSHOT_MAX_DIFF` per mille of its pixels differ (default 2).
//! - Captures, and a diff image for each failing view, are written to
//!   `target/screenshots/<platform>[-<QSA_SCREENSHOT_TAG>]/`.
//! - `QSA_BLESS=1` writes the captures as the new goldens.
//!
//! FLTK needs one UI thread, so this runs as a plain program
//! (`harness = false`), not under the test harness.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fltk::{app, prelude::*, window::Window};
use qsa_config::KeyMap;
use qsa_config::credentials::Memory;
use qsa_config::settings::{ColorMap, ScreenSize};
use qsa_config::system::CaCertificate;
use qsa_config::{AddressLookup, CredentialMode, Credentials, Settings, System, Systems};
use qsa_ui::snapshot::{self, Rgb};
use qsaccess::app::{AppState, Shared};
use qsaccess::session::start::StartOptions;
use qsaccess::session::window::{Conn, SessionWindow};
use qsaccess::session::{cert, keymapdlg, start};
use qsaccess::{mainwin, properties, sysconfig};
use std::rc::Rc;

fn platform() -> &'static str {
    if cfg!(windows) { "windows" } else { "linux" }
}

/// `windows`, `linux`, or `linux-<tag>` when `QSA_SCREENSHOT_TAG` is set (for
/// example `linux-wayland`). Goldens and captures are kept per variant.
fn variant() -> String {
    match std::env::var("QSA_SCREENSHOT_TAG") {
        Ok(tag) if !tag.is_empty() => format!("{}-{tag}", platform()),
        _ => platform().to_owned(),
    }
}

fn fixture() -> Shared {
    let mut systems = Systems::new();
    systems.upsert(System {
        description: "Production".into(),
        use_tls: true,
        default_user: "QSECOFR".into(),
        ..System::new("sys1.example.net")
    });
    systems.upsert(System {
        description: "Development and test".into(),
        credential_mode: CredentialMode::PromptOnce,
        default_user: "DEVUSR".into(),
        address_lookup: AddressLookup::Never,
        fixed_ip: "192.0.2.20".into(),
        ca_certificates: vec![CaCertificate::Path("/etc/quartz/ca.pem".into())],
        ..System::new("dev.example.net")
    });
    systems.upsert(System {
        description: "Backup".into(),
        credential_mode: CredentialMode::PromptEveryTime,
        ..System::new("10.1.1.7")
    });
    AppState::new(
        None,
        systems,
        Settings {
            last_system: "sys1.example.net".into(),
            ..Settings::default()
        },
        Credentials::with(Box::new(Memory::default())),
    )
    .shared()
}

fn settle() {
    for _ in 0..15 {
        let _ = app::wait_for(0.02);
    }
}

/// Shows a window beyond the right edge of the screen (so the real pointer
/// cannot hover over it), parks focus on it (so no field shows a caret), lets
/// it draw, captures it and hides it. The capture draws offscreen, so the
/// window's screen position does not matter.
fn render(mut win: Window) -> Rgb {
    let (sw, _) = app::screen_size();
    win.set_pos(sw as i32 + 200, 0);
    win.show();
    settle();
    // FLTK may move focus to the first field as the window maps; keep parking
    // it on the window until it stays there.
    for _ in 0..20 {
        app::set_focus(&win);
        win.redraw();
        settle();
        if app::focus().is_some_and(|f| f.as_widget_ptr() == win.as_widget_ptr()) {
            break;
        }
    }
    let img = snapshot::capture(&win);
    win.hide();
    settle();
    img
}

/// The final screen of a recorded session.
fn replayed(name: &str) -> tn5250::terminal::Terminal {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../fixtures/replay/{name}.qsr"));
    let rec = tn5250::replay::Recording::read(&path).expect("fixture recording");
    tn5250::replay::replay(&rec).terminal
}

fn session(state: &Shared, keypad: bool, keep: &mut Vec<Rc<SessionWindow>>) -> Window {
    let opts = StartOptions {
        system: "sys1.example.net".into(),
        size: ScreenSize::S24x80,
        ccsid: 37,
        device: String::new(),
        tls: false,
        color: ColorMap::Console,
    };
    let s = SessionWindow::build(state, opts, Some(1));
    s.show_terminal(replayed("qs400-signon-24x80"), Conn::Connected);
    s.set_keypad(keypad);
    let w = s.window.clone();
    keep.push(s);
    w
}

fn certificate() -> Window {
    let info = tn5250::telnet::x509::CertInfo {
        subject: "CN=sys1.example.net, O=Example Corp".into(),
        issuer: "CN=sys1.example.net, O=Example Corp".into(),
        not_before: "2026-01-01 00:00:00 UTC".into(),
        not_after: "2036-01-01 00:00:00 UTC".into(),
        sha256: (0..32)
            .map(|i| format!("{:02X}", i * 7 % 256))
            .collect::<Vec<_>>()
            .join(":"),
        serial: "01".into(),
    };
    cert::build(
        "sys1.example.net",
        "invalid peer certificate: UnknownIssuer",
        &info,
    )
    .0
}

/// QAD0030 for a settings file with a long path, which must wrap.
fn long_path_message() -> Window {
    let p = r"C:\Users\someone\AppData\Local\Temp\claude\c--Users-someone-Documents-GitHub-example\e77d43e6-eac0-4c01-9379-4ad509a246ef\scratchpad\e2e\cfg\settings.toml";
    let m = qsa_msg::message(qsa_msg::id::SETTINGS_UNREADABLE, &[p, "expected `]`"]);
    qsa_ui::msgdetails::message_box(&m).build().0
}

fn views(state: &Shared, keep: &mut Vec<Rc<SessionWindow>>) -> Vec<(String, Window)> {
    let mut out = Vec::new();
    let (ui, _rx) = mainwin::build(state.clone());
    out.push(("main".to_owned(), ui.window.clone()));
    out.push((
        "system-configurations".to_owned(),
        sysconfig::build(state).window,
    ));
    let sys = state
        .borrow()
        .systems
        .get("dev.example.net")
        .cloned()
        .expect("fixture system");
    for (i, tab) in ["general", "connection", "ports", "security"]
        .iter()
        .enumerate()
    {
        let (mut d, _form) = properties::build("System Properties - dev.example.net", &sys);
        d.tabs().select(i);
        out.push((format!("properties-{tab}"), d.window().clone()));
    }
    out.push((
        "start-5250".to_owned(),
        start::build(state, Some("sys1.example.net")).window,
    ));
    out.push(("session".to_owned(), session(state, false, keep)));
    out.push(("session-keypad".to_owned(), session(state, true, keep)));
    out.push((
        "keyboard-map".to_owned(),
        keymapdlg::build("Keyboard Map - sys1.example.net", &KeyMap::defaults()).window,
    ));
    out.push(("certificate".to_owned(), certificate()));
    out.push(("message-long-path".to_owned(), long_path_message()));
    out
}

/// Pixels that differ by more than `tol` in any channel. `None` if the sizes
/// differ.
fn diff(a: &Rgb, b: &Rgb, tol: u8) -> Option<(usize, Rgb)> {
    if (a.w, a.h) != (b.w, b.h) {
        return None;
    }
    let mut count = 0;
    let mut px = Vec::with_capacity(a.px.len());
    for (pa, pb) in a.px.chunks(3).zip(b.px.chunks(3)) {
        let off = pa.iter().zip(pb).any(|(x, y)| x.abs_diff(*y) > tol);
        if off {
            count += 1;
            px.extend_from_slice(&[0xFF, 0x00, 0x00]);
        } else {
            // Faded copy of the capture, so the red marks stand out.
            px.extend(pa.iter().map(|v| 0xC0 + v / 4));
        }
    }
    Some((count, Rgb { w: a.w, h: a.h, px }))
}

fn env_num(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn out_dir(manifest: &Path) -> PathBuf {
    if let Ok(d) = std::env::var("QSA_SCREENSHOT_OUT") {
        return PathBuf::from(d);
    }
    manifest
        .join("..")
        .join("target")
        .join("screenshots")
        .join(variant())
}

fn main() -> ExitCode {
    qsa_ui::theme::pin_font_rendering();
    // `cargo test -- --list` and filters come through here; only run when
    // not asked merely to list.
    if std::env::args().any(|a| a == "--list") {
        println!("screenshots: test");
        return ExitCode::SUCCESS;
    }
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let goldens = manifest.join("tests").join("goldens").join(variant());
    let out = out_dir(&manifest);
    let bless = std::env::var("QSA_BLESS").is_ok_and(|v| v == "1");
    let tol = env_num("QSA_SCREENSHOT_TOLERANCE", 40) as u8;
    let max_per_mille = env_num("QSA_SCREENSHOT_MAX_DIFF", 2) as usize;

    let _app = app::App::default();
    app::set_screen_scale(0, 1.0);
    qsa_ui::theme::set_hover_effects(false);
    if let Err(e) = qsa_ui::theme::init() {
        eprintln!("screenshots: fonts not loaded: {e}");
        return ExitCode::FAILURE;
    }

    let state = fixture();
    let mut failed = Vec::new();
    let mut keep = Vec::new();
    for (name, win) in views(&state, &mut keep) {
        let img = render(win);
        let file = format!("{name}.png");
        if let Err(e) = snapshot::write_png(&out.join(&file), &img) {
            eprintln!("screenshots: {}: {e}", out.join(&file).display());
            return ExitCode::FAILURE;
        }
        let golden = goldens.join(&file);
        if bless {
            if let Err(e) = snapshot::write_png(&golden, &img) {
                eprintln!("screenshots: {}: {e}", golden.display());
                return ExitCode::FAILURE;
            }
            println!("blessed  {name}");
            continue;
        }
        let Ok(want) = snapshot::read_png(&golden) else {
            println!("MISSING  {name}: no golden at {}", golden.display());
            failed.push(name);
            continue;
        };
        match diff(&img, &want, tol) {
            None => {
                println!(
                    "FAILED   {name}: size {}x{}, golden {}x{}",
                    img.w, img.h, want.w, want.h
                );
                failed.push(name);
            }
            Some((n, d)) => {
                let total = (img.w * img.h) as usize;
                let per_mille = n * 1000 / total.max(1);
                if n * 1000 > max_per_mille * total {
                    let _ = snapshot::write_png(&out.join(format!("{name}.diff.png")), &d);
                    println!("FAILED   {name}: {n} pixels differ ({per_mille}\u{2030})");
                    failed.push(name);
                } else {
                    println!("ok       {name}: {n} pixels differ");
                }
            }
        }
    }
    println!("screenshots written to {}", out.display());
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        println!("{} view(s) failed: {}", failed.len(), failed.join(", "));
        println!("If the change is intended, run with QSA_BLESS=1 and commit the goldens.");
        ExitCode::FAILURE
    }
}
