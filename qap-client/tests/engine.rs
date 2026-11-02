// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The pinned crates fit together: QS/400's connection engine answers a
//! `Hello` built from the pinned `qap-proto`.

use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use qap_client::proto::ops::connection::{Hello, HelloReply};
use qap_client::proto::{Envelope, Op, Status, VERSION};

#[test]
fn the_engine_answers_hello() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (s, _) = l.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
        qapd::serve(s, &mut NoHost, qapd::Config::default())
    });
    let mut s = TcpStream::connect(addr).unwrap();
    let hello = Hello {
        min_version: VERSION,
        max_version: VERSION,
        ..Hello::default()
    };
    Envelope::request(1, Op::Hello, &hello)
        .write_to(&mut s)
        .unwrap();
    let r = Envelope::read_from(&mut s).unwrap();
    assert_eq!(r.status, Status::Ok);
    assert_eq!(r.body_as::<HelloReply>().unwrap().version, VERSION);
    drop(s);
    assert_eq!(server.join().unwrap(), qapd::Closed::Peer);
}

/// A host that only answers what `Hello` asks.
struct NoHost;

impl qapd::Host for NoHost {
    fn system_id(&mut self) -> qapd::SystemId {
        qapd::SystemId::default()
    }
    fn now_ms(&mut self) -> i64 {
        0
    }
    fn message(&mut self, id: &str, _: &[&str]) -> qap_client::proto::ServerMessage {
        qap_client::proto::ServerMessage {
            id: id.into(),
            ..Default::default()
        }
    }
    fn system_info(&mut self) -> qap_client::proto::ops::system::SystemInfo {
        unreachable!()
    }
    fn ending(&mut self) -> Option<qapd::Ending> {
        None
    }
    fn sign_on(
        &mut self,
        _: &str,
        _: &qap_client::proto::Secret,
    ) -> Result<qap_client::proto::ops::signon::SignOnReply, qapd::Refusal> {
        unreachable!()
    }
    fn change_password(
        &mut self,
        _: &str,
        _: &qap_client::proto::Secret,
        _: &qap_client::proto::Secret,
    ) -> Result<qap_client::proto::ops::signon::SignOnReply, qapd::Refusal> {
        unreachable!()
    }
    fn resume(
        &mut self,
        _: &qap_client::proto::Token,
    ) -> Result<qap_client::proto::ops::signon::SignOnReply, qapd::Refusal> {
        unreachable!()
    }
    fn sign_off(&mut self, _: &qap_client::proto::Token) {}
    fn detach(&mut self, _: &qap_client::proto::Token) {}
}
