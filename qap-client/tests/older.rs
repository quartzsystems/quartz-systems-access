// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A QS/400 that speaks QAP versions 1 and 2 only (`common::v2`): `Hello`
//! agrees version 2 without `Spool`, every version-2 operation works, and
//! every `Spool` operation is refused by the client before it is sent.

mod common;

use std::sync::atomic::AtomicBool;

use common::target;
use common::v2::start_v2;
use qap_client::proto::Op;
use qap_client::proto::Service;
use qap_client::proto::ops::spool::{ReadFormat, SpooledFileId};
use qap_client::spool::{self, Action};
use qap_client::{Client, Error, Secret};

fn unsupported(e: Error) -> (Op, u16, u16) {
    match e {
        Error::Unsupported { op, needs, has } => (op, needs, has),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn spool_is_refused_before_sending_and_version_2_works() {
    let (port, _, closed) = start_v2();
    let mut c = Client::new(target(port));
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    assert_eq!(c.version(), Some(2));
    assert!(!c.hello().unwrap().services.contains(&Service::Spool));
    let list = Default::default();
    assert_eq!(
        unsupported(c.list_spooled_files(&list).unwrap_err()),
        (Op::ListSpooledFiles, 3, 2)
    );
    assert_eq!(
        unsupported(c.all_output_queues("*ALL").unwrap_err()),
        (Op::ListOutputQueues, 3, 2)
    );
    let id = SpooledFileId::default();
    for a in [
        Action::Hold {
            id: id.clone(),
            immediately: false,
        },
        Action::Release(id.clone()),
        Action::Delete(id.clone()),
        Action::Move {
            id: id.clone(),
            outq: Default::default(),
            first: false,
        },
    ] {
        assert_eq!(unsupported(c.act(&a).unwrap_err()), (a.op(), 3, 2));
    }
    let mut sink = Vec::new();
    let e = spool::read(
        |r| c.read_spooled_file(r),
        &id,
        ReadFormat::Scs,
        spool::BATCH_PAGES,
        &mut sink,
        |_| {},
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert_eq!(unsupported(e), (Op::ReadSpooledFile, 3, 2));
    // Nothing went out; version 2 goes on.
    assert!(closed.lock().unwrap().is_empty());
    assert!(c.connection().unwrap().closed().is_none());
    let p = c.run_command("CRTLIB LIB(QATEST)").unwrap();
    let o = qap_client::CommandOutcome::from_reply(p.wait_done().unwrap()).unwrap();
    assert_eq!(o.status, qap_client::Status::Ok);
    assert!(c.password_status().is_ok());
}
