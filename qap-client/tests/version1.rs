// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A QS/400 that speaks only QAP version 1 (`common::v1`). Against it, `Hello` agrees version 1 and lists the version-1 services,
//! sign-on, resume and sign-off work, and every version-2 operation is
//! refused by the client before it is sent.

mod common;

use common::v1::start_v1;
use common::{target, until};
use qap_client::proto::types::ObjectName;
use qap_client::proto::{Op, Service};
use qap_client::{Client, Connection, Error, Secret as ClientSecret};

/// Version 3's operations.
const SPOOL: [Op; 7] = [
    Op::ListSpooledFiles,
    Op::ReadSpooledFile,
    Op::HoldSpooledFile,
    Op::ReleaseSpooledFile,
    Op::DeleteSpooledFile,
    Op::MoveSpooledFile,
    Op::ListOutputQueues,
];

fn unsupported(e: Error) -> (Op, u16, u16) {
    match e {
        Error::Unsupported { op, needs, has } => (op, needs, has),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn hello_agrees_version_1() {
    let (port, _, _) = start_v1();
    let c = Connection::open(&target(port)).unwrap();
    assert_eq!(c.version(), 1);
    assert_eq!(
        c.hello().services,
        [Service::Connection, Service::SignOn, Service::System]
    );
    assert_eq!(c.supports(Op::SystemInfo).ok(), Some(()));
    for op in [
        Op::RunCommand,
        Op::CheckCommand,
        Op::DescribeCommand,
        Op::PasswordStatus,
        Op::TelnetTicket,
    ] {
        assert_eq!(unsupported(c.request(op, &()).unwrap_err()), (op, 2, 1));
    }
    for op in SPOOL {
        assert_eq!(unsupported(c.request(op, &()).unwrap_err()), (op, 3, 1));
    }
}

#[test]
fn sign_on_works_and_version_2_is_refused_before_sending() {
    let (port, sys, closed) = start_v1();
    let mut c = Client::new(target(port));
    assert_eq!(c.supports(Op::SystemInfo).ok(), Some(()));
    assert_eq!(c.version(), Some(1));
    let p = c.sign_on("bob", &ClientSecret::from("SECRET1")).unwrap();
    assert_eq!(p.profile, "BOB");
    assert_eq!(c.system_info().unwrap().system_name, "QSV1");

    assert_eq!(
        unsupported(c.supports(Op::RunCommand).unwrap_err()),
        (Op::RunCommand, 2, 1)
    );
    assert_eq!(
        unsupported(c.password_status().unwrap_err()),
        (Op::PasswordStatus, 2, 1)
    );
    assert_eq!(
        unsupported(c.telnet_ticket().unwrap_err()),
        (Op::TelnetTicket, 2, 1)
    );
    assert_eq!(
        unsupported(c.run_command("CRTLIB LIB(X)").unwrap_err()),
        (Op::RunCommand, 2, 1)
    );
    assert_eq!(
        unsupported(c.check_command("CRTLIB LIB(X)").unwrap_err()),
        (Op::CheckCommand, 2, 1)
    );
    let name = ObjectName {
        library: "*LIBL".into(),
        name: "CRTLIB".into(),
    };
    assert_eq!(
        unsupported(c.describe_command(name).unwrap_err()),
        (Op::DescribeCommand, 2, 1)
    );

    // Nothing went out: a version-1 engine closes on a service it cannot
    // decode, and the connection and session are still there.
    assert!(c.connection().unwrap().closed().is_none());
    assert!(closed.lock().unwrap().is_empty());
    assert_eq!(c.system_info().unwrap().system_name, "QSV1");
    c.sign_off().unwrap();
    until("the sign-off", || sys.sessions.lock().unwrap().is_empty());
}
