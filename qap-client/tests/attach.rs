// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QAP version 4's `Attach` and `JobInfo` against QS/400's engine and test
//! system: a window's own connection joins the session without a sign-on
//! and has a job of its own; servers before version 4 are refused before
//! anything is sent.

mod common;

use common::v2::start_v2;
use common::{cfg, start, target};
use qap_client::proto::Op;
use qap_client::{Client, Error, Secret, Status};

#[test]
fn an_attached_connection_has_its_own_job_as_the_sessions_profile() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    assert_eq!(c.version(), Some(6));
    let shared = c.job_info().unwrap();
    assert_eq!(shared.user, "BOB");

    let a = c.attach().unwrap();
    assert_eq!(a.job().user, "BOB");
    assert_eq!(a.profile().profile, "BOB");
    assert!(
        a.profile().token.0.iter().all(|b| *b == 0),
        "token not kept"
    );
    assert_ne!(a.job().job.number, shared.job.number);
    assert_eq!(a.attaches(), 1);
    // The shared connection still works beside it.
    assert_eq!(c.system_info().unwrap().system_name, "QSHQ400");
}

#[test]
fn attach_needs_a_session_and_version_4() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    let e = c.attach().unwrap_err();
    assert_eq!(e.status(), Some(Status::NotSignedOn));

    let (port, _, _) = start_v2();
    let mut c = Client::new(target(port));
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    match c.attach().unwrap_err() {
        Error::Unsupported { op, needs, has } => assert_eq!((op, needs, has), (Op::Attach, 4, 2)),
        other => panic!("{other:?}"),
    }
    match c.job_info().unwrap_err() {
        Error::Unsupported { op, .. } => assert_eq!(op, Op::JobInfo),
        other => panic!("{other:?}"),
    }
}
