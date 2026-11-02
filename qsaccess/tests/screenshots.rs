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

use fltk::window::Window;
use qsa_config::KeyMap;
use qsa_config::settings::{ColorMap, ScreenSize};
use qsa_config::system::CaCertificate;
use qsa_config::{AddressLookup, Settings, System, Systems};
use qsa_ui::goldens::Goldens;
use qsaccess::app::{AppState, Shared};
use qsaccess::session::start::StartOptions;
use qsaccess::session::window::{Conn, SessionWindow};
use qsaccess::session::{cert, keymapdlg, loadmap, start};
use qsaccess::{mainwin, properties, sysconfig};
use std::rc::Rc;

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
        default_user: "DEVUSR".into(),
        address_lookup: AddressLookup::Never,
        fixed_ip: "192.0.2.20".into(),
        ca_certificates: vec![CaCertificate::Path("/etc/quartz/ca.pem".into())],
        ..System::new("dev.example.net")
    });
    systems.upsert(System {
        description: "Backup".into(),
        ..System::new("10.1.1.7")
    });
    AppState::new(
        None,
        systems,
        Settings {
            last_system: "sys1.example.net".into(),
            ..Settings::default()
        },
    )
    .shared()
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
    let info = qsa_net::x509::CertInfo {
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

/// Session Manager showing a fixed set of files (none ever used, so the
/// time zone does not matter).
fn session_manager(state: &Shared) -> Window {
    use qsa_config::qsd::SessionFile;
    use qsaccess::manager::{self, Doc, Entry};
    let dir = std::path::PathBuf::from("/home/user/Documents/Quartz Access/Sessions");
    let mut entries = Vec::new();
    for (title, sys, wide, dev) in [
        ("Payroll", "sys1.example.net", false, ""),
        ("Operations console", "sys1.example.net", true, "QSAOPS01"),
        ("Development", "dev.example.net", false, "DEVDSP01"),
    ] {
        let mut f = SessionFile::new(&System::new(sys));
        f.session.title = title.into();
        if wide {
            f.session.screen_size = ScreenSize::S27x132;
        }
        f.session.device_name = dev.into();
        entries.push(Entry {
            path: dir.join(format!("{title}.qsd")),
            folder: dir.clone(),
            file: Ok(Doc::Display(f)),
            last_used: None,
        });
    }
    let mut p = qsa_config::qsp::PrinterFile::new(&System::new("sys1.example.net"), "QSAPRT01");
    p.printer.title = "Office output".into();
    entries.push(Entry {
        path: dir.join("Office output.qsp"),
        folder: dir.clone(),
        file: Ok(Doc::Printer(p)),
        last_used: None,
    });
    entries.push(Entry {
        path: dir.join("old.qsd"),
        folder: dir.clone(),
        file: Err("not valid".into()),
        last_used: None,
    });
    let m = manager::build(state);
    let pick = [dir.join("Payroll.qsd"), dir.join("Office output.qsp")];
    m.show(entries, &pick, 1);
    m.window.clone()
}

/// Session Properties for a session of sys1.
fn session_properties(state: &Shared, tab: usize) -> Window {
    use qsa_config::qsd::SessionFile;
    use qsaccess::session::props;
    let st = state.borrow();
    let sys = st.systems.get("sys1.example.net").expect("fixture system");
    let mut f = SessionFile::new(sys);
    f.session.title = "Payroll".into();
    let (mut d, _form) = props::build(
        "Session Properties - Payroll",
        &props::Props::from_file(&f),
        &st.systems,
    );
    d.tabs().select(tab);
    d.window().clone()
}

/// The keyboard map in `tests/data/operations.qkm`, as loaded into a
/// session.
fn loaded_keymap() -> KeyMap {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/operations.qkm");
    qsa_config::qkm::KeymapFile::load(&p)
        .expect("fixture keyboard map")
        .map
}

/// The Macro editor with the sign-on and WRKACTJOB macro.
fn macro_editor() -> Window {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/macros/sign-on-wrkactjob-f3.qsm");
    let m = qsa_config::qsm::MacroFile::load(&path).expect("fixture macro");
    let e = qsaccess::macros::editor::build(m, None);
    let mut list = e.list.clone();
    list.select(Some(3));
    e.window.clone()
}

/// The Step dialog for a step that waits for text.
fn macro_step() -> Window {
    use qsa_config::qsm::{Action, Step};
    let step = Step::new(Action::WaitText {
        row: 1,
        col: 36,
        text: "Sign On".into(),
        timeout_ms: 15000,
    });
    qsaccess::macros::editor::build_step("Edit Step", &step).0
}

/// QAD0030 for a settings file with a long path, which must wrap.
fn long_path_message() -> Window {
    let p = r"C:\Users\someone\AppData\Local\Temp\claude\c--Users-someone-Documents-GitHub-example\e77d43e6-eac0-4c01-9379-4ad509a246ef\scratchpad\e2e\cfg\settings.toml";
    let m = qsa_msg::message(qsa_msg::id::SETTINGS_UNREADABLE, &[p, "expected `]`"]);
    qsa_ui::msgdetails::message_box(&m).build().0
}

/// Verify Connection for sys1 after a run: Telnet's TLS certificate not
/// trusted, the rest passed, no test sign-on.
fn verify(state: &Shared) -> Window {
    use qsaccess::verify::checks::{Check, Outcome};
    let sys = state
        .borrow()
        .systems
        .get("sys1.example.net")
        .cloned()
        .expect("fixture system");
    let ok = |s: &str| Outcome::Ok(s.into());
    let results = [
        (Check::Dns, ok("sys1.example.net is 192.0.2.10")),
        (Check::Telnet, ok("Port 23 accepts connections.")),
        (
            Check::TelnetTls,
            Outcome::Failed {
                message: qsa_msg::message(
                    qsa_msg::id::CERT_UNTRUSTED,
                    &[
                        "sys1.example.net",
                        "invalid peer certificate: UnknownIssuer",
                    ],
                ),
                server: None,
                untrusted: None,
            },
        ),
        (Check::Qap, ok("Port 2320 accepts connections.")),
        (
            Check::QapTls,
            ok("TLSv1_3, TLS13_AES_256_GCM_SHA384; certificate trusted"),
        ),
        (Check::Hello, ok("Version 2, system SYS1, release V1R0M0.")),
        (
            Check::SignOn,
            Outcome::Skipped("Not tested: no user ID entered.".into()),
        ),
    ];
    qsaccess::verify::build(&sys, &results).window
}

/// A refusal from the system's password validation program, as sent.
fn system_message() -> Window {
    let m = qsa_ui::msgdetails::SystemMessage {
        id: "CPF22C6",
        severity: 40,
        text: "Password validation program PWDVLDPGM in QGPL rejected the new password.",
        help: "",
    };
    qsa_ui::msgdetails::system_message_box(None, &m, &["OK"])
        .build()
        .0
}

/// A printer session on sys1.example.net, device QSAPRT01.
fn printer_file(state: &Shared) -> qsa_config::qsp::PrinterFile {
    let st = state.borrow();
    let sys = st.systems.get("sys1.example.net").expect("fixture system");
    let mut f = qsa_config::qsp::PrinterFile::new(sys, "QSAPRT01");
    f.printer.title = "Office output".into();
    f.output.pdf_folder = "/home/user/Documents/Quartz Access/Printer Output/QSAPRT01".into();
    f.output.also_save_text = true;
    f
}

/// The printer window, connected over TLS, with three jobs received.
fn printer_session(state: &Shared) -> Window {
    use qsaccess::printer::spool::{JobRecord, JobStatus, Problem};
    use qsaccess::printer::window::{PrinterWindow, State};
    let p = PrinterWindow::build(state, printer_file(state), None);
    let dir =
        std::path::PathBuf::from("/home/user/Documents/Quartz Access/Printer Output/QSAPRT01");
    let job = |time: i64, pages: usize, bytes: u64, name: &str, status: JobStatus| JobRecord {
        time,
        title: "QSAPRT01".into(),
        pages,
        bytes,
        target: qsa_config::qsp::Target::Pdf,
        pdf: Some(dir.join(name)),
        text: None,
        printer: None,
        status,
        spool: None,
        problems: Vec::new(),
        incomplete: false,
    };
    let mut failed = job(
        1_793_570_400,
        3,
        9_412,
        "QSAPRT01-20261101-140000-0003.pdf",
        JobStatus::Failed,
    );
    failed.pdf = None;
    failed.problems = vec![Problem::Folder(dir.clone(), "access denied".into())];
    p.show_as(
        State::Ready,
        true,
        vec![
            job(
                1_793_563_200,
                2,
                3_210,
                "QSAPRT01-20261101-120000-0001.pdf",
                JobStatus::Done,
            ),
            job(
                1_793_566_800,
                14,
                48_660,
                "QSAPRT01-20261101-130000-0002.pdf",
                JobStatus::Done,
            ),
            failed,
        ],
    );
    let w = p.window.clone();
    std::mem::forget(p);
    w
}

/// A system's message as the test system sends one.
fn sent(id: &str, severity: u8, text: &str) -> qap_client::ServerMessage {
    qap_client::ServerMessage {
        id: id.into(),
        severity,
        data: Vec::new(),
        text: text.into(),
        help: format!("Cause . . . . . :   {text}"),
    }
}

/// Run Command after two runs of CRTLIB: created, then already there.
fn run_command_results() -> Window {
    use qsaccess::runcmd::{self, Line};
    let job = qap_client::proto::types::JobId {
        name: "QAPCNN".into(),
        user: "QUSER".into(),
        number: "004217".into(),
    };
    let lines = [
        Line::Ran("CRTLIB LIB(QATESTLIB)".into()),
        Line::Message(sent("CPC2102", 0, "Library QATESTLIB created.")),
        Line::Ran("CRTLIB LIB(QATESTLIB)".into()),
        Line::Message(sent("CPF2111", 40, "Library QATESTLIB already exists.")),
        Line::Checked("DSPOBJD OBJ(QGPL/X)".into()),
        Line::Message(sent("CPD0071", 30, "Value for parameter OBJTYPE required.")),
        Line::Message(sent("CPF0006", 30, "Errors occurred in command.")),
    ];
    let m = qsa_msg::message(qsa_msg::id::COMMAND_FAILED, &["CRTLIB"]);
    runcmd::build(
        "sys1.example.net",
        "CRTLIB LIB(QATESTLIB)",
        Some(&job),
        &lines,
        Some(&m),
    )
    .window
}

/// A Prompt form for one of QS/400's test commands.
fn prompt_form(name: &str) -> qsaccess::prompt::form::Form {
    let c = qapd::testing::commands::find(name).expect("a test command");
    qsaccess::prompt::form::Form::new(qapd::testing::commands::definition(c))
}

/// Node `kwd`'s index, or its `nth` child's.
fn node(f: &qsaccess::prompt::form::Form, kwd: &str, nth: Option<usize>) -> u16 {
    let p = f
        .parameters()
        .find(|p| p.keyword == kwd)
        .expect("a parameter")
        .index;
    match nth {
        Some(n) => f.children(p).nth(n).expect("a child").index,
        None => p,
    }
}

/// The Prompt views: (name, window).
fn prompt_views() -> Vec<(String, Window)> {
    use qsaccess::prompt;
    let mut out = Vec::new();
    let f = prompt_form("CRTLIB");
    out.push(("prompt-simple".to_owned(), prompt::build(&f, false).window));

    let mut f = prompt_form("DSPOBJD");
    f.set(node(&f, "OBJ", Some(0)), 0, "ORDERS");
    f.set(node(&f, "OBJ", Some(1)), 0, "QGPL");
    out.push((
        "prompt-qualifiers".to_owned(),
        prompt::build(&f, false).window,
    ));

    let ty = node(&f, "OBJTYPE", None);
    f.set(ty, 0, "*FILE");
    f.add_entry(ty);
    f.set(ty, 1, "*PGM");
    out.push(("prompt-list".to_owned(), prompt::build(&f, false).window));

    let mut f = prompt_form("CRTPF");
    f.set(node(&f, "FILE", Some(0)), 0, "ORDERS");
    out.push((
        "prompt-additional".to_owned(),
        prompt::build(&f, true).window,
    ));
    f.set(node(&f, "SIZE", Some(0)), 0, "20000");
    f.set(node(&f, "SIZE", Some(2)), 0, "10");
    out.push(("prompt-elements".to_owned(), prompt::build(&f, true).window));

    let f = prompt_form("DSPOBJD");
    out.push((
        "prompt-checked".to_owned(),
        prompt::build_checked(
            &f,
            &[
                sent("CPD0071", 30, "Value for parameter OBJ required."),
                sent("CPF0006", 30, "Errors occurred in command."),
            ],
        )
        .window,
    ));
    let lib = node(&f, "OBJ", Some(1));
    out.push((
        "prompt-help".to_owned(),
        prompt::help_box("Help - OBJ", &prompt::node_help(&f, lib))
            .build()
            .0,
    ));
    out
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
    out.push(("session-manager".to_owned(), session_manager(state)));
    out.push((
        "session-properties-general".to_owned(),
        session_properties(state, 0),
    ));
    out.push((
        "session-properties-hotspots".to_owned(),
        session_properties(state, 1),
    ));
    out.push((
        "keyboard-map-loaded".to_owned(),
        keymapdlg::build("Keyboard Map - sys1.example.net", &loaded_keymap()).window,
    ));
    out.push((
        "load-keyboard-map".to_owned(),
        loadmap::build(
            Path::new("operations.qkm"),
            &[
                ("Payroll".to_owned(), "sys1.example.net".to_owned()),
                ("Development".to_owned(), "dev.example.net".to_owned()),
            ],
        )
        .window,
    ));
    out.push((
        "start-5250-printer".to_owned(),
        qsaccess::printer::start::build(state, Some("sys1.example.net")).window,
    ));
    out.push(("printer-session".to_owned(), printer_session(state)));
    out.push((
        "printer-settings".to_owned(),
        qsaccess::printer::settings::build(&printer_file(state)).window,
    ));
    out.push(("macro-editor".to_owned(), macro_editor()));
    out.push(("macro-step".to_owned(), macro_step()));
    out.push((
        "sign-on".to_owned(),
        qsaccess::signon::build("sys1.example.net", "QSECOFR")
            .0
            .window,
    ));
    out.push((
        "change-password".to_owned(),
        qsaccess::chgpwd::build("sys1.example.net", "QSECOFR", true)
            .0
            .window,
    ));
    out.push(("message-system".to_owned(), system_message()));
    out.push(("verify-connection".to_owned(), verify(state)));
    out.push((
        "run-command".to_owned(),
        qsaccess::runcmd::build("sys1.example.net", "", None, &[], None).window,
    ));
    out.push(("run-command-results".to_owned(), run_command_results()));
    out.extend(prompt_views());
    out.push((
        "run-macro".to_owned(),
        qsaccess::macros::build_run(
            "Sign on and WRKACTJOB",
            &[
                ("Payroll".to_owned(), "sys1.example.net".to_owned()),
                ("Development".to_owned(), "dev.example.net".to_owned()),
            ],
        )
        .0
        .window,
    ));
    out
}

fn main() -> ExitCode {
    if Goldens::listing("screenshots") {
        return ExitCode::SUCCESS;
    }
    let Some((_app, mut g)) = Goldens::start(Path::new(env!("CARGO_MANIFEST_DIR"))) else {
        return ExitCode::FAILURE;
    };
    let state = fixture();
    let mut keep = Vec::new();
    for (name, win) in views(&state, &mut keep) {
        g.check(&name, win);
    }
    g.finish()
}
