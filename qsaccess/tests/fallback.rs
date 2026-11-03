// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Against a system whose QAP server speaks only version 1 (QS/400's engine
//! at 58a7e6d3, qap-client's `common::v1`): sign-on works, and every
//! version-2 function is turned off before anything is sent, with
//! `QAD5001` naming the version it needs and the one the system has. The
//! password expiry warning, which has nothing to turn off, is silent.
//! Against version 1 and version 2 alike, Printer Output (version 3) is
//! dimmed with `QAD5001`, and every earlier function still works.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use qap_client::Secret;
use qap_client::proto::Op;
use qsa_config::system::CaCertificate;
use qsa_config::{Settings, System, Systems};
use qsa_msg::id;
use qsaccess::app::AppState;
use qsaccess::qap::{
    self, auto_sign_on_blocked, expiry_due, function_name, lock, printer_output_blocked,
};

fn system(port: u16) -> System {
    let mut sys = System::new("127.0.0.1");
    sys.ports.qap = port;
    sys.ca_certificates = vec![CaCertificate::Path(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../qsa-net/tests/data/test-ca.pem"
        )
        .into(),
    )];
    sys
}

fn state(sys: &System) -> qsaccess::app::Shared {
    let mut systems = Systems::new();
    systems.upsert(sys.clone());
    AppState::new(None, systems, Settings::default()).shared()
}

#[test]
fn every_version_2_function_is_qad5001_and_sign_on_still_works() {
    let (port, _, closed) = common::v1::start_v1();
    let sys = system(port);
    let st = state(&sys);
    let h = qap::client(&st, &sys);
    lock(&h).sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    assert_eq!(qap::signed_on(&st, "127.0.0.1").as_deref(), Some("BOB"));
    assert_eq!(lock(&h).version(), Some(1));

    // Run Command, Check and Prompt: QAD5001, from the version Hello gave.
    for (op, function) in [
        (Op::RunCommand, "Run Command"),
        (Op::CheckCommand, "Check"),
        (Op::DescribeCommand, "Prompt"),
    ] {
        let e = lock(&h).supports(op).unwrap_err();
        let x = qap::explain(&sys, &e);
        assert_eq!(x.message.id, id::QAP_VERSION_NEEDED);
        assert_eq!(
            x.message.text,
            format!("{function} needs QAP version 2; system 127.0.0.1 has version 1.")
        );
        assert_eq!(function_name(op), function);
    }
    // Automatic sign-on: the Session Properties check box is off, and why.
    assert_eq!(qap::version_of(&st, "127.0.0.1", true), Some(1));
    let m = auto_sign_on_blocked("127.0.0.1", true, Some(1)).unwrap();
    assert_eq!(m.id, id::QAP_VERSION_NEEDED);
    assert_eq!(
        m.text,
        "Automatic sign-on needs QAP version 2; system 127.0.0.1 has version 1."
    );
    // The expiry warning: nothing asked, nothing shown.
    assert_eq!(expiry_due(&h).unwrap(), None);
    // Printer Output: the task pane's entry dimmed, and QAD5001 if opened.
    let m = printer_output_blocked("127.0.0.1", Some(1)).unwrap();
    assert_eq!(
        m.text,
        "Printer Output needs QAP version 3; system 127.0.0.1 has version 1."
    );
    let e = lock(&h).supports(Op::ListSpooledFiles).unwrap_err();
    assert_eq!(qap::explain(&sys, &e).message, m);

    // Nothing of version 2 went out: the connection and session are there.
    assert!(closed.lock().unwrap().is_empty());
    assert_eq!(lock(&h).system_info().unwrap().system_name, "QSV1");
}

#[test]
fn a_version_2_system_offers_them() {
    let (port, _, _) = common::v2::start_v2();
    let sys = system(port);
    let st = state(&sys);
    assert_eq!(qap::version_of(&st, "127.0.0.1", true), Some(2));
    assert!(auto_sign_on_blocked("127.0.0.1", true, Some(2)).is_none());
    assert_eq!(
        auto_sign_on_blocked("127.0.0.1", false, Some(2))
            .unwrap()
            .id,
        id::AUTO_SIGNON_NEEDS_TLS
    );
    // Not known (the system not reached): left to the connection.
    assert!(auto_sign_on_blocked("127.0.0.1", true, None).is_none());
}

#[test]
fn a_version_2_system_has_no_printer_output_and_everything_else_works() {
    let (port, _, closed) = common::v2::start_v2();
    let sys = system(port);
    let st = state(&sys);
    let h = qap::client(&st, &sys);
    lock(&h).sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    assert_eq!(qap::version_of(&st, "127.0.0.1", false), Some(2));
    // Printer Output: dimmed, and QAD5001 with the versions.
    let m = printer_output_blocked("127.0.0.1", Some(2)).unwrap();
    assert_eq!(m.id, id::QAP_VERSION_NEEDED);
    assert_eq!(
        m.text,
        "Printer Output needs QAP version 3; system 127.0.0.1 has version 2."
    );
    for op in [
        Op::ListSpooledFiles,
        Op::ReadSpooledFile,
        Op::HoldSpooledFile,
        Op::ListOutputQueues,
    ] {
        let e = lock(&h).supports(op).unwrap_err();
        assert_eq!(qap::explain(&sys, &e).message, m, "{op:?}");
    }
    // Everything of version 2 works: Run Command, Check, Prompt, the
    // expiry warning and the Telnet ticket.
    let p = lock(&h).run_command("CRTLIB LIB(QATEST)").unwrap();
    let o = qap_client::CommandOutcome::from_reply(p.wait_done().unwrap()).unwrap();
    assert_eq!(o.status, qap_client::Status::Ok);
    assert!(lock(&h).check_command("CRTLIB LIB(X)").unwrap().valid());
    let def = lock(&h)
        .describe_command(qap_client::proto::types::ObjectName {
            library: "*LIBL".into(),
            name: "CRTLIB".into(),
        })
        .unwrap();
    assert_eq!(def.text, "Create Library");
    assert_eq!(expiry_due(&h).unwrap(), None);
    assert!(lock(&h).telnet_ticket().is_ok());
    assert!(
        closed.lock().unwrap().is_empty(),
        "nothing of version 3 went out"
    );
}

#[test]
fn a_version_3_system_offers_printer_output() {
    let s = common::start(common::cfg());
    let sys = system(s.port);
    let st = state(&sys);
    assert_eq!(qap::version_of(&st, "127.0.0.1", true), Some(6));
    assert!(printer_output_blocked("127.0.0.1", Some(6)).is_none());
    assert!(printer_output_blocked("127.0.0.1", None).is_none());
}

/// Before version 4: Run SQL, Schemas and Data Transfer are not offered
/// (`QAD5001`), nothing of the `Sql` service or `Attach` is sent, and Run
/// Command stays on the shared connection and works.
#[test]
fn a_version_2_system_has_no_sql_and_run_command_keeps_the_shared_connection() {
    let (port, _, closed) = common::v2::start_v2();
    let sys = system(port);
    let st = state(&sys);
    let h = qap::client(&st, &sys);
    lock(&h).sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    for f in ["Run SQL", "Schemas", "Data Transfer"] {
        let m = qsaccess::runsql::sql_blocked(f, "127.0.0.1", Some(2)).unwrap();
        assert_eq!(m.id, id::QAP_VERSION_NEEDED);
        assert_eq!(
            m.text,
            format!("{f} needs QAP version 4; system 127.0.0.1 has version 2.")
        );
    }
    for op in [Op::Prepare, Op::ListSchemas, Op::Attach, Op::JobInfo] {
        assert!(lock(&h).supports(op).is_err(), "{op:?}");
    }
    // Nor the Integrated File System (version 5).
    let m = qsaccess::ifs::blocked("127.0.0.1", Some(2)).unwrap();
    assert_eq!(
        m.text,
        "Integrated File System needs QAP version 5; system 127.0.0.1 has version 2."
    );
    let e = qap_client::Files::get_info(&mut *lock(&h), "/home/BOB").unwrap_err();
    assert_eq!(
        qap::explain(&sys, &e).message.text,
        "Integrated File System needs QAP version 5; system 127.0.0.1 has version 2."
    );
    let e = lock(&h).sql_connection().unwrap_err();
    assert_eq!(qap::explain(&sys, &e).message.id, id::QAP_VERSION_NEEDED);
    assert!(lock(&h).attach().is_err());
    let p = lock(&h).run_command("CRTLIB LIB(QATEST)").unwrap();
    let o = qap_client::CommandOutcome::from_reply(p.wait_done().unwrap()).unwrap();
    assert_eq!(o.status, qap_client::Status::Ok);
    assert!(
        closed.lock().unwrap().is_empty(),
        "one connection, nothing refused"
    );
}

#[test]
fn a_version_5_system_has_no_messages() {
    for has in [1u16, 2, 5] {
        let m = qsaccess::messages::blocked("127.0.0.1", Some(has)).unwrap();
        assert_eq!(m.id, id::QAP_VERSION_NEEDED);
        assert_eq!(
            m.text,
            format!("Messages needs QAP version 6; system 127.0.0.1 has version {has}.")
        );
    }
    assert!(qsaccess::messages::blocked("127.0.0.1", Some(6)).is_none());
    assert!(qsaccess::messages::blocked("127.0.0.1", None).is_none());
    // Nothing of version 6 goes to a version-5 session.
    let s = common::start(common::cfg());
    let mut t = common::target(s.port);
    t.options.max_version = 5;
    let mut c = qap_client::Client::new(t);
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let e = c
        .list_messages(&qap_client::proto::ops::messages::Queue::Own, true, 0, 1)
        .unwrap_err();
    assert_eq!(
        qap::explain(&system(s.port), &e).message.text,
        format!(
            "Messages needs QAP version 6; system {} has version 5.",
            system(s.port).name
        )
    );
}

#[test]
fn a_version_4_system_has_no_integrated_file_system() {
    let m = qsaccess::ifs::blocked("127.0.0.1", Some(4)).unwrap();
    assert_eq!(m.id, id::QAP_VERSION_NEEDED);
    assert!(qsaccess::ifs::blocked("127.0.0.1", Some(5)).is_none());
    assert!(qsaccess::ifs::blocked("127.0.0.1", None).is_none());
}

#[test]
fn a_version_4_system_offers_sql() {
    for f in ["Run SQL", "Schemas", "Data Transfer"] {
        assert!(qsaccess::runsql::sql_blocked(f, "127.0.0.1", Some(4)).is_none());
        assert!(qsaccess::runsql::sql_blocked(f, "127.0.0.1", None).is_none());
    }
}
