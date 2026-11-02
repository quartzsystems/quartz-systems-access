// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The connection against QS/400's engine: TLS 1.3 only and the trust
//! flow, Hello as request 1, version mismatch, `NotSignedOn`, 64
//! outstanding requests and `Busy` at the 65th, replies over 1 MiB split
//! with `More`, requests over 1 MiB refused, `Cancel`, `Ping` after a quiet
//! spell, and `ServerEnding`.

mod common;

use std::time::Duration;

use common::{cfg, start, start_with, until};
use qap_client::proto::ops::connection::PingReply;
use qap_client::proto::ops::signon::SignOn;
use qap_client::proto::ops::system::SystemInfo;
use qap_client::proto::{MAX_FRAME, MAX_OUTSTANDING, Op, Service};
use qap_client::{Client, Closed, Connection, Error, Notice, Reply, Secret, Status};
use qapd::{Config, Ending};

#[test]
fn hello_over_tls13() {
    let s = start(cfg());
    let c = Connection::open(&s.target()).unwrap();
    let h = c.hello();
    assert_eq!(h.version, 2);
    assert_eq!(h.system_name, "QSHQ400");
    assert_eq!(h.release, "V1R0M0");
    assert_eq!(h.services, Service::ALL.to_vec());
    assert!(
        matches!(c.security(), qsa_net::Security::Tls { protocol, .. } if protocol == "TLSv1_3")
    );
    // Hello was request 1 (the engine closes on anything else); the next is 2.
    let p = c.request(Op::Ping, &()).unwrap();
    assert_eq!(p.id(), 2);
    let ping: PingReply = p.wait(Duration::from_secs(5)).unwrap().decode().unwrap();
    assert_eq!(ping.server_time, 1_793_000_000_000);
    drop(c);
    until("the server to see the close", || {
        s.ended() == [qapd::Closed::Peer]
    });
}

#[test]
fn a_tls12_server_is_refused() {
    const TLS12: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS12];
    let s = start_with(cfg(), qapd::testing::System::new(), TLS12);
    match Connection::open(&s.target()) {
        Err(Error::Connect(qsa_net::ConnectError::Tls(qsa_net::TlsError::Handshake(_)))) => {}
        other => panic!("expected a handshake failure, got {other:?}"),
    }
}

#[test]
fn an_untrusted_certificate_goes_to_the_trust_flow() {
    let s = start(cfg());
    let mut t = s.target();
    t.trust = qsa_net::Trust::default();
    match Connection::open(&t) {
        Err(Error::Connect(qsa_net::ConnectError::Tls(qsa_net::TlsError::Untrusted {
            info,
            ..
        }))) => assert_eq!(info.subject, "CN=localhost, O=Quartz Systems Test"),
        other => panic!("expected untrusted, got {other:?}"),
    }
}

#[test]
fn version_mismatch() {
    let s = start(cfg());
    let mut t = s.target();
    // The server speaks versions 1 and 2.
    t.options.min_version = 3;
    t.options.max_version = 9;
    let e = Connection::open(&t).unwrap_err();
    assert_eq!(e.status(), Some(Status::VersionUnsupported), "{e:?}");
    until("the server to close", || {
        s.ended() == [qapd::Closed::Refused]
    });
}

#[test]
fn signed_on_operations_need_a_sign_on() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    let e = c.system_info().unwrap_err();
    assert_eq!(e.status(), Some(Status::NotSignedOn), "{e:?}");
    let e = c.sign_off().unwrap_err();
    assert_eq!(e.status(), Some(Status::NotSignedOn), "{e:?}");
    // The connection stays.
    assert!(c.connection().unwrap().closed().is_none());
}

#[test]
fn busy_at_the_65th_request() {
    let s = start(cfg());
    let c = Connection::open(&s.target()).unwrap();
    s.c.gate.set(false);
    let held: Vec<_> = (0..MAX_OUTSTANDING)
        .map(|_| c.request(Op::Ping, &()).unwrap())
        .collect();
    let r = c
        .request(Op::Ping, &())
        .unwrap()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(r.status, Status::Busy);
    let e = r.decode::<PingReply>().unwrap_err();
    assert!(e.is_busy(), "{e:?}");

    // call() waits and tries again until there is room.
    let gate = std::thread::spawn({
        let c = std::sync::Arc::clone(&s.c);
        move || {
            std::thread::sleep(Duration::from_millis(300));
            c.gate.set(true);
        }
    });
    let ping: PingReply = c.call(Op::Ping, &()).unwrap();
    assert_eq!(ping.server_time, 1_793_000_000_000);
    gate.join().unwrap();
    for p in held {
        assert_eq!(p.wait(Duration::from_secs(5)).unwrap().status, Status::Ok);
    }
}

#[test]
fn a_reply_over_1_mib_is_split_and_joined() {
    let s = start(cfg());
    *s.c.products.lock().unwrap() = 50_000;
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let before = c.connection().unwrap().frames_received();
    let info = c.system_info().unwrap();
    assert_eq!(info.products.len(), 50_000);
    assert_eq!(info.products[49_999].id, "QS49999");
    let frames = c.connection().unwrap().frames_received() - before;
    assert!(frames >= 2, "{frames} frame(s)");
}

#[test]
fn a_request_over_1_mib_is_refused_here() {
    let s = start(cfg());
    let c = Connection::open(&s.target()).unwrap();
    let body = SignOn {
        user: "BOB".into(),
        password: Secret::new("x".repeat(MAX_FRAME)),
    };
    assert!(matches!(
        c.request(Op::SignOn, &body),
        Err(Error::TooLarge(n)) if n > MAX_FRAME
    ));
    // Nothing was sent; the connection goes on.
    let _: PingReply = c.call(Op::Ping, &()).unwrap();
}

#[test]
fn cancel() {
    let s = start(cfg());
    let c = Connection::open(&s.target()).unwrap();
    s.c.gate.set(false);
    // The first runs and waits at the gate; the second waits behind it.
    let running = c.request(Op::Ping, &()).unwrap();
    let queued = c.request(Op::SystemInfo, &()).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    queued.cancel();
    running.cancel();
    std::thread::sleep(Duration::from_millis(100));
    s.c.gate.set(true);
    for p in [running, queued] {
        let r = p.wait(Duration::from_secs(5)).unwrap();
        assert_eq!(r.status, Status::Cancelled);
        assert!(r.body.is_empty());
    }
    // A cancel naming nothing outstanding is ignored.
    let done = c.request(Op::Ping, &()).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    done.cancel();
    let _: PingReply = c.call(Op::Ping, &()).unwrap();
}

#[test]
fn a_reply_too_slow_is_cancelled() {
    let s = start(cfg());
    let c = Connection::open(&s.target()).unwrap();
    s.c.gate.set(false);
    let p = c.request(Op::Ping, &()).unwrap();
    assert!(matches!(
        p.wait(Duration::from_millis(100)),
        Err(Error::TimedOut)
    ));
    s.c.gate.set(true);
    let _: PingReply = c.call(Op::Ping, &()).unwrap();
}

#[test]
fn ping_after_a_quiet_spell_keeps_the_connection() {
    let s = start(Config {
        idle_close: Duration::from_millis(600),
        ..cfg()
    });
    let mut t = s.target();
    t.options.ping_after = Duration::from_millis(100);
    let c = Connection::open(&t).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(c.pings_sent() >= 5, "{} pings", c.pings_sent());
    assert_eq!(c.closed(), None);
    assert!(s.ended().is_empty());
}

#[test]
fn the_idle_close_is_quiet() {
    let s = start(Config {
        idle_close: Duration::from_millis(200),
        ..cfg()
    });
    let c = Connection::open(&s.target()).unwrap();
    until("the idle close", || c.closed().is_some());
    assert_eq!(c.closed(), Some(Closed::Quietly));
    assert_eq!(c.next_event(), None);
    assert!(matches!(
        c.request(Op::Ping, &()).map(|_| ()),
        Err(Error::Closed(Closed::Quietly))
    ));
}

#[test]
fn server_ending() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    s.c.end(Ending::Server);
    let mut notices = Vec::new();
    until("ServerEnding", || {
        notices.extend(c.poll());
        !notices.is_empty() && c.connection().is_none()
    });
    match &notices[..] {
        [Notice::ServerEnding(Some(m))] => {
            assert_eq!(m.id, "TCP1A0A");
            assert_eq!(m.text, "TCP1A0A QAP (lang of BOB)");
        }
        other => panic!("{other:?}"),
    }
    // The session ended with the server, and nothing reconnects by itself.
    assert!(!c.is_signed_on());
    assert!(c.server_ending());
    assert!(matches!(
        c.system_info(),
        Err(Error::Closed(Closed::ServerEnding))
    ));
    assert_eq!(s.ended(), [qapd::Closed::ServerEnding]);
    // Reconnect when asked.
    assert_eq!(c.reconnect().unwrap().system_name, "QSHQ400");
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    assert_eq!(c.system_info().unwrap().system_name, "QSHQ400");
}

#[test]
fn a_raw_reply_keeps_status_and_message() {
    let s = start(cfg());
    let c = Connection::open(&s.target()).unwrap();
    let r: Reply = c
        .request(
            Op::SignOn,
            &SignOn {
                user: "NOBODY".into(),
                password: Secret::from("x"),
            },
        )
        .unwrap()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(r.status, Status::SignOnFailed);
    assert_eq!(r.message.as_ref().unwrap().id, "CPF1107");
    let e = r.decode::<SystemInfo>().unwrap_err();
    assert_eq!(e.server_message().unwrap().id, "CPF1107");
}
