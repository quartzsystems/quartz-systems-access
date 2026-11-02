// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Against a system whose QAP server speaks only version 1 (QS/400's engine
//! at 58a7e6d3, qap-client's `common::v1`): sign-on works, and every
//! version-2 function is turned off before anything is sent, with
//! `QAD5001` naming the version it needs and the one the system has. The
//! password expiry warning, which has nothing to turn off, is silent.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use qap_client::Secret;
use qap_client::proto::Op;
use qsa_config::system::CaCertificate;
use qsa_config::{Settings, System, Systems};
use qsa_msg::id;
use qsaccess::app::AppState;
use qsaccess::qap::{self, auto_sign_on_blocked, expiry_due, function_name, lock};

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
