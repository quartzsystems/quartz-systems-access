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

use fltk::prelude::*;
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
        auto_sign_on: false,
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
    session_properties_with(state, tab, true, false)
}

/// Session Properties for sys1 (QAP version 2), with TLS and automatic
/// sign-on as given.
fn session_properties_with(state: &Shared, tab: usize, tls: bool, auto: bool) -> Window {
    use qsa_config::qsd::SessionFile;
    use qsaccess::session::props;
    let st = state.borrow();
    let sys = st.systems.get("sys1.example.net").expect("fixture system");
    let mut f = SessionFile::new(sys);
    f.session.title = "Payroll".into();
    f.system.tls = tls;
    f.session.auto_sign_on = auto;
    let (mut d, _form) = props::build(
        "Session Properties - Payroll",
        &props::Props::from_file(&f),
        &st.systems,
        Some(2),
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

/// QS/400's test system's spooled files (`qapd::testing::fixtures`), in
/// the order the system keeps them.
fn spooled_files() -> Vec<qap_client::proto::ops::spool::SpooledFile> {
    let sys = qapd::testing::System::new();
    let sp = sys.spool.lock().unwrap();
    sp.files.values().map(|f| f.info()).collect()
}

fn spooled(name: &str) -> qap_client::proto::ops::spool::SpooledFile {
    spooled_files()
        .into_iter()
        .find(|f| f.id.file == name)
        .expect("a test system file")
}

/// Printer Output on sys1.example.net with `filter`, showing page one of
/// the files it matches.
fn printer_output(
    state: &Shared,
    filter: qsaccess::splf::model::Filter,
    files: Vec<qap_client::proto::ops::spool::SpooledFile>,
    total: u32,
) -> Window {
    use qsaccess::splf::{PrinterOutput, model};
    let st = state.borrow();
    let sys = st.systems.get("sys1.example.net").expect("fixture system");
    let p = PrinterOutput::build(sys, &st.settings.printer_output);
    p.show_files(&filter, files, total, &model::utc_time);
    p.window().clone()
}

/// The viewer on page `page` of the test system's file `name`, finding
/// `find`.
fn viewer(state: &Shared, name: &str, page: usize, find: &str) -> Window {
    use qsaccess::splf::{model, viewer};
    let data = {
        let sys = qapd::testing::System::new();
        let sp = sys.spool.lock().unwrap();
        sp.files
            .values()
            .find(|f| f.id.file == name)
            .expect("a test system file")
            .data
            .clone()
    };
    let options = model::scs_options(37, qsa_config::qsp::Paper::Letter);
    let doc = scs::parse(&data, &options);
    let st = state.borrow();
    let system = st.systems.get("sys1.example.net").expect("fixture system");
    let v = viewer::Viewer::build(system, name, options);
    v.show_doc(doc, page, viewer::Zoom::FitWidth, find);
    v.window.clone()
}

/// Output Queues on sys1.example.net: the test system's queues.
fn output_queues(state: &Shared) -> Window {
    let sys = qapd::testing::System::new();
    let queues: Vec<_> = {
        let sp = sys.spool.lock().unwrap();
        sp.queues
            .iter()
            .map(|q| qap_client::proto::ops::spool::OutputQueue {
                outq: q.outq.clone(),
                text: q.text.clone(),
                files: q.files.len() as u32,
                writer: q.writer.clone(),
                held: q.held,
            })
            .collect()
    };
    let st = state.borrow();
    let system = st.systems.get("sys1.example.net").expect("fixture system");
    let w = qsaccess::splf::queues::QueuesWindow::build(system, 25);
    let n = queues.len() as u32;
    w.show_queues("*ALL", queues, n);
    w.window.clone()
}

fn printer_output_views(state: &Shared) -> Vec<(String, Window)> {
    use qap_client::proto::ops::spool::SpoolStatus;
    use qsaccess::splf::{dialogs, model};
    let bobs: Vec<_> = spooled_files()
        .into_iter()
        .filter(|f| f.user == "BOB")
        .collect();
    let total = bobs.len() as u32;
    let first: Vec<_> = bobs.iter().take(25).cloned().collect();
    let held_filter = model::Filter {
        user: "*ALL".into(),
        status: Some(SpoolStatus::Held),
        ..model::Filter::default()
    };
    let held: Vec<_> = bobs
        .iter()
        .filter(|f| f.status == SpoolStatus::Held)
        .cloned()
        .collect();
    let empty_filter = model::Filter {
        user_data: "PAYROLL".into(),
        ..model::Filter::default()
    };
    let picked: Vec<_> = ["RPT001", "RPT002", "STS5"]
        .iter()
        .map(|n| spooled(n))
        .collect();
    let queues = vec![
        qap_client::proto::ops::spool::OutputQueue {
            outq: model::object_named("QGPL/QPRINT").unwrap(),
            text: "Default printer output".into(),
            files: 60,
            writer: String::new(),
            held: false,
        },
        qap_client::proto::ops::spool::OutputQueue {
            outq: model::object_named("QGPL/PRT01").unwrap(),
            text: "Printer PRT01".into(),
            files: 2,
            writer: "PRT01".into(),
            held: false,
        },
    ];
    let sent = |id: &str, text: &str| qap_client::proto::ServerMessage {
        id: id.into(),
        severity: 40,
        data: Vec::new(),
        text: text.into(),
        help: String::new(),
    };
    let done: Vec<model::Done> = picked
        .iter()
        .map(|f| model::Done {
            file: f.clone(),
            action: qap_client::spool::Action::Delete(f.id.clone()),
            outcome: if f.id.file == "STS5" {
                model::Outcome::Refused {
                    server: Some(sent(
                        "CPF3330",
                        "Spooled file STS5 is printing; it cannot be deleted.",
                    )),
                    line: String::new(),
                }
            } else {
                model::Outcome::Done
            },
        })
        .collect();
    vec![
        (
            "printer-output".to_owned(),
            printer_output(state, model::Filter::default(), first, total),
        ),
        (
            "printer-output-held".to_owned(),
            printer_output(state, held_filter, held, 1),
        ),
        (
            "printer-output-empty".to_owned(),
            printer_output(state, empty_filter, Vec::new(), 0),
        ),
        (
            "printer-output-hold".to_owned(),
            dialogs::hold_box(&picked).build().0,
        ),
        (
            "printer-output-move".to_owned(),
            dialogs::build_move(&picked[..1], &queues, "QGPL/QPRINT").window,
        ),
        (
            "printer-output-delete".to_owned(),
            dialogs::build_delete(&picked).window,
        ),
        (
            "printer-output-properties".to_owned(),
            dialogs::build_properties(&spooled("QSYSPRT"), &model::utc_time).0,
        ),
        (
            "printer-output-summary".to_owned(),
            dialogs::build_summary(&done).window,
        ),
        ("output-queues".to_owned(), output_queues(state)),
        ("viewer".to_owned(), viewer(state, "QSYSPRT", 0, "")),
        (
            "viewer-landscape".to_owned(),
            viewer(state, "LANDSCAPE", 1, ""),
        ),
        (
            "viewer-find".to_owned(),
            viewer(state, "QPJOBLOG", 0, "TEST0"),
        ),
    ]
}

/// Run Command after two runs of CRTLIB (created, then already there), a
/// check, and CHGAUT and CPYFRMSTMF with the test system's severities 10
/// and 20.
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
        Line::Ran("CHGAUT OBJ('/home/BOB') USER(ANN) DTAAUT(*RWXA)".into()),
        Line::Message(sent("CPDA091", 10, "Value for DTAAUT not valid.")),
        Line::Ran("CPYFRMSTMF FROMSTMF('/home/BOB/long.txt') TOMBR(X)".into()),
        Line::Message(sent("CPFA096", 20, "Data in record 3 truncated at 80.")),
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

/// Run SQL: empty; with two result tabs; with an error; with a result cut
/// short by a cancel; under a commit level with Commit and Rollback; and the
/// uncommitted-changes prompt.
fn run_sql_views() -> Vec<(String, Window)> {
    use qap_client::proto::ops::sql::{Column, CommitLevel, SetOptions, SqlValue};
    use qsaccess::runsql::RunSql;
    use qsaccess::runsql::worker::{Detail, Line, Standing};
    let sys = System::new("sys1.example.net");
    let job = qap_client::proto::types::JobId {
        name: "QAPCNN".into(),
        user: "QUSER".into(),
        number: "004311".into(),
    };
    let col = |name: &str, label: &str, t: &str, length: u32, scale: u16| Column {
        name: name.into(),
        label: label.into(),
        sql_type: t.into(),
        length,
        scale,
        nullable: true,
        ccsid: if t == "CHAR" || t == "VARCHAR" { 37 } else { 0 },
    };
    let asset_cols = vec![
        col("ASSET_TAG", "Asset tag", "CHAR", 10, 0),
        col("LOCATION", "Rack / site code", "CHAR", 10, 0),
        col("MANUFACTURER", "Manufacturer", "VARCHAR", 40, 0),
        col("MODEL", "Model", "VARCHAR", 40, 0),
        col("ACQUIRED", "Acquisition date", "DATE", 10, 0),
        col("COST", "Purchase cost", "DECIMAL", 11, 2),
        col("NOTES", "Short notes", "VARCHAR", 256, 0),
    ];
    let t = |s: &str| SqlValue::Text { text: s.into() };
    let day = |y, m, d| SqlValue::Date {
        ms: qsa_sql::datetime::day_number(y, m, d) * 86_400_000,
    };
    let asset_rows: Vec<Vec<SqlValue>> = [
        (
            "QS000101  ",
            "QS-HQ-R1  ",
            "QUANTA",
            "T4048-IX8",
            (2025, 3, 11),
            "1200.00",
        ),
        (
            "QS000110  ",
            "QS-HQ-R1  ",
            "EDGECORE",
            "AS5712-54X",
            (2024, 11, 2),
            "1650.00",
        ),
        (
            "QS000201  ",
            "QS-HQ-R2  ",
            "DELL",
            "R740XD",
            (2024, 6, 20),
            "1800.00",
        ),
        (
            "QS000302  ",
            "QS-HQ-R3  ",
            "WEIDMULLER",
            "UR20-FBC-EIP",
            (2026, 10, 1),
            "2400.00",
        ),
        (
            "QS000402  ",
            "QS-BR1-R1 ",
            "WATCHGUARD",
            "M370",
            (2024, 9, 8),
            "2700.00",
        ),
    ]
    .iter()
    .map(|(a, l, m, mo, (y, mm, d), c)| {
        vec![
            t(a),
            t(l),
            t(m),
            t(mo),
            day(*y, *mm, *d),
            SqlValue::Decimal { text: (*c).into() },
            SqlValue::Null,
        ]
    })
    .collect();
    let loc_cols = vec![
        col("LOCATION", "Rack / site code", "CHAR", 10, 0),
        col("SITE", "Site", "VARCHAR", 40, 0),
    ];
    let loc_rows = vec![
        vec![t("QS-HQ-R1  "), t("Headquarters")],
        vec![t("QS-BR1-R1 "), t("Branch 1")],
    ];
    let ok = |n: usize, line: usize, text: &str, rows: Option<i64>| Line {
        statement: n,
        script_line: line,
        verb: "SELECT".into(),
        id: String::new(),
        sqlstate: "00000".into(),
        sqlcode: Some(0),
        text: text.into(),
        rows,
        elapsed: std::time::Duration::from_millis(37 + n as u64 * 11),
        failed: false,
        detail: Detail::None,
    };
    let script = "SELECT * FROM HLINV.ASSETS;\nSELECT * FROM HLINV.LOCATIONS;\n";
    let none = SetOptions::default();
    let mut out = Vec::new();
    out.push((
        "run-sql".to_owned(),
        RunSql::build(&sys, Some(&job), "", &[], &[], &none, false)
            .window()
            .clone(),
    ));
    out.push((
        "run-sql-results".to_owned(),
        RunSql::build(
            &sys,
            Some(&job),
            script,
            &[
                ok(1, 1, "5 rows fetched.", Some(5)),
                ok(2, 2, "2 rows fetched.", Some(2)),
            ],
            &[
                (asset_cols.clone(), asset_rows.clone(), Standing::Complete),
                (loc_cols, loc_rows, Standing::Complete),
            ],
            &none,
            false,
        )
        .window()
        .clone(),
    ));
    let mut failed = ok(1, 1, "Token SELEKT was not valid.", None);
    failed.failed = true;
    failed.id = "SQL0104".into();
    failed.sqlstate = "42601".into();
    failed.sqlcode = Some(-104);
    let error = RunSql::build(
        &sys,
        Some(&job),
        "SELEKT * FROM HLINV.ASSETS;\n",
        &[failed],
        &[],
        &none,
        false,
    );
    error.w.tabs.clone().select(0);
    error.w.message.clone().set(
        "SQL0104: Token SELEKT was not valid.",
        qsa_ui::Status::Error,
    );
    out.push(("run-sql-error".to_owned(), error.window().clone()));
    let mut cancelled = ok(1, 1, "Processing of the SQL statement ended.", None);
    cancelled.id = "SQL0952".into();
    cancelled.sqlstate = "57014".into();
    cancelled.sqlcode = Some(-952);
    let incomplete = RunSql::build(
        &sys,
        Some(&job),
        "SELECT * FROM HLINV.ASSETS;\n",
        &[cancelled],
        &[(
            asset_cols,
            asset_rows[..3].to_vec(),
            Standing::Incomplete("cancelled".into()),
        )],
        &none,
        false,
    );
    incomplete.w.message.clone().set(
        &qsa_msg::message(qsa_msg::id::RESULT_INCOMPLETE, &["cancelled"]).to_string(),
        qsa_ui::Status::Error,
    );
    out.push(("run-sql-incomplete".to_owned(), incomplete.window().clone()));
    let mut changed = ok(1, 1, "1 rows changed.", Some(1));
    changed.id = "QAD7020".into();
    let committed = SetOptions {
        commit: CommitLevel::ReadCommitted,
        naming: qap_client::proto::ops::sql::Naming::Sql,
        default_schema: Some("HLTEST".into()),
    };
    out.push((
        "run-sql-commit".to_owned(),
        RunSql::build(
            &sys,
            Some(&job),
            "INSERT INTO JRN VALUES(1, 'one');\n",
            &[changed],
            &[],
            &committed,
            true,
        )
        .window()
        .clone(),
    ));
    out.push((
        "run-sql-uncommitted".to_owned(),
        qsa_ui::msgdetails::ask_message_box(
            &qsa_msg::message(
                qsa_msg::id::UNCOMMITTED_CHANGES,
                &["Run SQL - sys1.example.net (004311/QUSER/QAPCNN)"],
            ),
            &["Commit", "Roll Back", "Cancel"],
        )
        .build()
        .0,
    ));
    out
}

/// The columns of `HLINV.ASSETS`, as the catalog gives them.
fn asset_columns() -> Vec<qap_client::proto::ops::sql::Column> {
    let col = |name: &str, label: &str, t: &str, length: u32, scale: u16, nullable: bool| {
        qap_client::proto::ops::sql::Column {
            name: name.into(),
            label: label.into(),
            sql_type: t.into(),
            length,
            scale,
            nullable,
            ccsid: if t.contains("CHAR") { 37 } else { 0 },
        }
    };
    vec![
        col("ASSET_TAG", "Asset tag", "CHAR", 10, 0, false),
        col("LOCATION", "Rack / site code", "CHAR", 10, 0, false),
        col("MANUFACTURER", "Manufacturer", "VARCHAR", 40, 0, false),
        col("MODEL", "Model", "VARCHAR", 40, 0, false),
        col("SERIAL", "Serial number", "VARCHAR", 40, 0, true),
        col("ACQUIRED", "Acquisition date", "DATE", 10, 0, true),
        col("COST", "Purchase cost", "DECIMAL", 11, 2, true),
        col("CHANGED_TS", "Last change", "TIMESTAMP", 26, 6, false),
    ]
}

/// Data Transfer from the system: each page, the fixed-width widths, the
/// progress dialog and the summary.
fn download_views() -> Vec<(String, Window)> {
    use qsa_transfer::{Download, FileOptions, Format, Source};
    use qsaccess::transfer::download::DownloadWindow;
    let sys = System::new("sys1.example.net");
    let d = Download {
        source: Source::Table {
            schema: "HLINV".into(),
            table: "ASSETS".into(),
            where_clause: "STATUS = 'ACTIVE'".into(),
        },
        columns: vec!["ASSET_TAG".into(), "MODEL".into(), "COST".into()],
        limit: Some(1000),
        file: PathBuf::from("C:\\Users\\bob\\Documents\\assets.csv"),
        ..Download::default()
    };
    let mut out = Vec::new();
    for (i, page) in ["source", "columns", "output"].iter().enumerate() {
        let w = DownloadWindow::build(&sys, &d, &asset_columns());
        w.w.tabs.clone().select(i);
        out.push((format!("transfer-download-{page}"), w.w.window.clone()));
    }
    let fixed = Download {
        options: FileOptions {
            format: Format::Fixed,
            ..FileOptions::default()
        },
        file: PathBuf::from("C:\\Users\\bob\\Documents\\assets.txt"),
        ..d.clone()
    };
    let w = DownloadWindow::build(&sys, &fixed, &asset_columns());
    w.w.tabs.clone().select(2);
    out.push(("transfer-download-fixed".to_owned(), w.w.window.clone()));
    let query = Download {
        source: Source::Query {
            sql: "SELECT ASSET_TAG, COST\n  FROM HLINV.ASSETS\n WHERE COST > 1500".into(),
        },
        columns: Vec::new(),
        ..d
    };
    let w = DownloadWindow::build(&sys, &query, &asset_columns());
    out.push(("transfer-download-query".to_owned(), w.w.window.clone()));
    let mut p = qsaccess::transfer::progress_dialog("assets.csv");
    p.rows
        .set_label(&qsaccess::transfer::progress_text(qsa_transfer::Progress {
            rows: 1500,
            elapsed: std::time::Duration::from_millis(2500),
        }));
    out.push(("transfer-progress".to_owned(), p.window));
    out.push((
        "transfer-summary".to_owned(),
        qsa_ui::msgdetails::ask_message_box(
            &qsaccess::transfer::complete("assets.csv", 12, std::time::Duration::from_millis(400)),
            &["OK"],
        )
        .build()
        .0,
    ));
    out
}

/// Data Transfer to the system: each page, the mapping grid with a value
/// that does not fit, and the prompt to upload without commitment control.
fn upload_views() -> Vec<(String, Window)> {
    use qsa_transfer::reader::{Row, Sheet, Value};
    use qsa_transfer::{Commit, Map, Mode, SourceColumn, Upload};
    use qsaccess::transfer::upload::UploadWindow;
    let sys = System::new("sys1.example.net");
    let t = |s: &str| Value::Text(s.into());
    let preview = Sheet {
        header: Some(vec![
            "ASSET_TAG".into(),
            "LOCATION".into(),
            "MANUFACTURER".into(),
            "MODEL".into(),
            "COST".into(),
        ]),
        rows: vec![
            Row {
                values: vec![
                    t("QS000501"),
                    t("QS-HQ-R4"),
                    t("DELL"),
                    t("R650"),
                    t("2100.00"),
                ],
                line: 2,
            },
            Row {
                values: vec![
                    t("QS000502"),
                    t("QS-HQ-R4"),
                    t("DELL"),
                    t("R650"),
                    Value::Null,
                ],
                line: 3,
            },
            Row {
                values: vec![
                    t("QS000503"),
                    t("QS-BR1-R1"),
                    t("ARISTA"),
                    t("7050SX3"),
                    t("123456789012.50"),
                ],
                line: 4,
            },
        ],
    };
    let u = Upload {
        file: PathBuf::from("C:\\Users\\bob\\Documents\\new-assets.csv"),
        schema: "HLINV".into(),
        table: "ASSETS".into(),
        mode: Mode::Append,
        mapping: vec![
            Map {
                target: "ASSET_TAG".into(),
                from: SourceColumn::Name("ASSET_TAG".into()),
            },
            Map {
                target: "LOCATION".into(),
                from: SourceColumn::Name("LOCATION".into()),
            },
            Map {
                target: "MANUFACTURER".into(),
                from: SourceColumn::Name("MANUFACTURER".into()),
            },
            Map {
                target: "MODEL".into(),
                from: SourceColumn::Name("MODEL".into()),
            },
            Map {
                target: "COST".into(),
                from: SourceColumn::Name("COST".into()),
            },
        ],
        commit: Commit::Every { batches: 10 },
        ..Upload::default()
    };
    let mut out = Vec::new();
    for (i, page) in ["input", "target", "mapping", "commit"].iter().enumerate() {
        let w = UploadWindow::build(&sys, &u, &asset_columns(), preview.clone());
        w.w.tabs.clone().select(i);
        out.push((format!("transfer-upload-{page}"), w.w.window.clone()));
    }
    out.push((
        "transfer-no-commitment-control".to_owned(),
        qsa_ui::msgdetails::system_message_box(
            Some(&qsa_msg::message(
                qsa_msg::id::NO_COMMITMENT_CONTROL,
                &["HLTEST.NOJRN"],
            )),
            &qsa_ui::msgdetails::SystemMessage {
                id: "SQL7008",
                severity: 30,
                text: "NOJRN in HLTEST not valid for operation.",
                help: "",
            },
            &["Upload Without Commitment Control", "Cancel"],
        )
        .build()
        .0,
    ));
    out
}

/// Schemas: the tree of schemas, and a table's columns.
fn schemas_views() -> Vec<(String, Window)> {
    use qap_client::proto::ops::sql::{Table, TableKind};
    use qsaccess::schemas::Schemas;
    let sys = System::new("sys1.example.net");
    let schemas: Vec<(String, String)> = [
        ("HLINV", "Homelab inventory"),
        ("HLMANY", "Many tables"),
        ("HLPAY", "Payroll"),
        ("HLTEST", "QAP test tables"),
        ("QGPL", "General purpose library"),
        ("QSYS2", "System catalog"),
    ]
    .iter()
    .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
    .collect();
    let tree = Schemas::build(&sys, &schemas, None);
    let assets = Table {
        schema: "HLINV".into(),
        name: "ASSETS".into(),
        kind: TableKind::Table,
        text: "Hardware assets".into(),
    };
    let cols = asset_columns();
    let columns = Schemas::build(&sys, &schemas, Some((&assets, &cols)));
    vec![
        ("schemas".to_owned(), tree.w.window.clone()),
        ("schemas-columns".to_owned(), columns.w.window.clone()),
    ]
}

/// The Integrated File System window's views: a home folder with links
/// (one broken, in red), a large folder's first page, a folder the profile
/// may not read, and the two delete confirmations.
fn ifs_views() -> Vec<(String, Window)> {
    use qap_client::proto::ops::ifs::EntryKind::{Directory, File, Symlink};
    use qsaccess::ifs::model::{Row, Target};
    use qsaccess::ifs::{Ifs, entry};
    let sys = System::new("sys1.example.net");
    let home = "/home/BOB";
    let row = |name: &str, kind, size, ccsid| Row {
        path: format!("{home}/{name}"),
        entry: entry(name, kind, size, ccsid),
        link: None,
    };
    let link = |name: &str, to: Target| Row {
        link: Some(to),
        ..row(name, Symlink, 0, 0)
    };
    let mut rows = vec![
        row("many", Directory, 8192, 0),
        row("text", Directory, 8192, 0),
        row("tree", Directory, 8192, 0),
        row("big.bin", File, 200 << 20, 65535),
        row("checkedout.txt", File, 19, 1208),
        row("locked.txt", File, 20, 1208),
        row("notes.txt", File, 4211, 37),
        link(
            "link1208",
            Target::To(
                "/home/BOB/text/text1208.txt".into(),
                entry("text1208.txt", File, 80, 1208),
            ),
        ),
        link("broken", Target::Broken("/home/BOB/gone.txt".into())),
    ];
    for (i, r) in rows.iter_mut().enumerate() {
        r.entry.modified += i as i64 * 3_600_000;
    }
    let home_view = Ifs::build(&sys, home, home, rows, 9, None);
    let many: Vec<Row> = (0..500)
        .map(|i| Row {
            path: format!("/home/BOB/many/f{i:04}.txt"),
            entry: entry(&format!("f{i:04}.txt"), File, 10, 1208),
            link: None,
        })
        .collect();
    let large = Ifs::build(&sys, home, "/home/BOB/many", many, 1200, None);
    let private = Ifs::build(
        &sys,
        home,
        "/home/ANN/private",
        Vec::new(),
        0,
        Some("CPFA09C: Not authorized to object. Object is /home/ANN/private."),
    );
    let broken = Ifs::build(
        &sys,
        home,
        home,
        vec![
            link("broken", Target::Broken("/home/BOB/gone.txt".into())),
            link(
                "link1208",
                Target::To(
                    "/home/BOB/text/text1208.txt".into(),
                    entry("text1208.txt", File, 80, 1208),
                ),
            ),
        ],
        2,
        None,
    );
    let files = qsaccess::ifs::delete_files_text(&[
        row("notes.txt", File, 4211, 37),
        row("locked.txt", File, 20, 1208),
        row("big.bin", File, 200 << 20, 65535),
    ]);
    let folder = qsaccess::ifs::delete_folders_text(
        &[
            row("tree", Directory, 8192, 0),
            row("notes.txt", File, 4211, 37),
        ],
        &[(
            "/home/BOB/tree".into(),
            qsa_transfer::files::Counts {
                files: 7,
                folders: 4,
                bytes: 7 * 30,
            },
        )],
    );
    let confirm = |text: &str| {
        qsa_ui::MessageBox::new("Confirm Delete", text)
            .glyph(qsa_ui::Glyph::Cross)
            .buttons(&["Delete", "Cancel"], 1, 1)
            .build()
            .0
    };
    vec![
        ("ifs-home".to_owned(), home_view.w.window.clone()),
        ("ifs-large-folder".to_owned(), large.w.window.clone()),
        ("ifs-unauthorized".to_owned(), private.w.window.clone()),
        ("ifs-broken-link".to_owned(), broken.w.window.clone()),
        ("ifs-delete-files".to_owned(), confirm(&files)),
        ("ifs-delete-folder".to_owned(), confirm(&folder)),
    ]
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
        "session-properties-auto-sign-on".to_owned(),
        session_properties_with(state, 0, true, true),
    ));
    out.push((
        "session-properties-auto-sign-on-disabled".to_owned(),
        session_properties_with(state, 0, false, true),
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
    out.extend(run_sql_views());
    out.extend(download_views());
    out.extend(upload_views());
    out.extend(schemas_views());
    out.extend(ifs_views());
    out.extend(printer_output_views(state));
    out.extend(prompt_views());
    out.push((
        "password-expiry".to_owned(),
        qsa_ui::msgdetails::ask_message_box(
            &qsaccess::qap::expiry_message("QATEST", "sys1.example.net", 3),
            &["Change Password...", "Close"],
        )
        .build()
        .0,
    ));
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
