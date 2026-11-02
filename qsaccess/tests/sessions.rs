// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! One QAP session per system per process: the registry against QS/400's
//! test system (qap-client's test server). Every tool gets the same client;
//! a long command and another tool's request share its connection; the GUI
//! thread's look at the profile never waits on a client in use.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Server, cfg, start};
use qap_client::{CommandOutcome, Secret, Status};
use qsa_config::System;
use qsa_config::system::CaCertificate;
use qsaccess::qap::{Registry, lock};

/// The test server as a system configuration: its port, the test CA.
fn system(s: &Server) -> System {
    let mut sys = System::new("127.0.0.1");
    sys.ports.qap = s.port;
    sys.ca_certificates = vec![CaCertificate::Path(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../qsa-net/tests/data/test-ca.pem"
        )
        .into(),
    )];
    sys
}

fn signed_on(r: &mut Registry, sys: &System) {
    let h = r.client(sys);
    lock(&h).sign_on("BOB", &Secret::from("SECRET1")).unwrap();
}

#[test]
fn every_tool_gets_the_same_signed_on_client() {
    let s = start(cfg());
    let sys = system(&s);
    let mut r = Registry::default();
    assert!(
        r.signed_on_client("127.0.0.1").is_none(),
        "none before sign-on"
    );
    signed_on(&mut r, &sys);
    let a = r.signed_on_client("127.0.0.1").unwrap();
    let b = r.signed_on_client("127.0.0.1").unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    assert!(Arc::ptr_eq(&a, &r.client(&sys)), "kept while signed on");
    assert_eq!(r.profile("127.0.0.1").as_deref(), Some("BOB"));
    // One session on the system, whatever asked.
    assert_eq!(s.c.sessions.lock().unwrap().len(), 1);
}

#[test]
fn a_running_command_and_another_tool_share_the_connection() {
    let s = start(cfg());
    let sys = system(&s);
    let mut r = Registry::default();
    signed_on(&mut r, &sys);
    let h = r.signed_on_client("127.0.0.1").unwrap();
    // The command tool holds the client only to send.
    let pending = lock(&h).run_command("DLYJOB DLY(2)").unwrap();
    let at = Instant::now();
    // Another tool, meanwhile, on the same client and connection. The test
    // system's connection job runs one operation at a time, as QS/400's
    // does, so this one answers once the delay ends; it went out at once,
    // with its own request ID, while the command was outstanding.
    let other = lock(&h).request(qap_client::proto::Op::Ping, &()).unwrap();
    assert_ne!(other.id(), pending.id());
    let done = CommandOutcome::from_reply(pending.wait(Duration::from_secs(10)).unwrap()).unwrap();
    assert_eq!(done.status, Status::Ok);
    assert!(at.elapsed() >= Duration::from_secs(2));
    other.wait(Duration::from_secs(10)).unwrap();
    assert_eq!(s.ended(), [], "one connection, still open");
}

#[test]
fn the_profile_is_what_was_last_seen_while_a_tool_holds_the_client() {
    let s = start(cfg());
    let sys = system(&s);
    let mut r = Registry::default();
    signed_on(&mut r, &sys);
    assert_eq!(r.profile("127.0.0.1").as_deref(), Some("BOB"));
    let h = r.signed_on_client("127.0.0.1").unwrap();
    let held = lock(&h);
    // Would block if it waited for the lock.
    assert_eq!(r.profile("127.0.0.1").as_deref(), Some("BOB"));
    assert!(r.signed_on_client("127.0.0.1").is_some());
    drop(held);
}

#[test]
fn a_client_not_signed_on_follows_the_configuration() {
    let s = start(cfg());
    let mut sys = system(&s);
    let mut r = Registry::default();
    let first = r.client(&sys);
    sys.ports.qap = 1;
    let second = r.client(&sys);
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(lock(&second).target().port, 1);
    // Signed on, it is kept until it signs off.
    sys.ports.qap = s.port;
    signed_on(&mut r, &sys);
    let on = r.client(&sys);
    sys.ports.qap = 2;
    assert!(Arc::ptr_eq(&on, &r.client(&sys)));
    lock(&on).sign_off().unwrap();
    assert!(r.signed_on_client("127.0.0.1").is_none());
    assert!(!Arc::ptr_eq(&on, &r.client(&sys)));
}
