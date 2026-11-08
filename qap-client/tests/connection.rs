// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The connection against QS/400's engine: TLS 1.3 only and the trust
//! flow, Hello as request 1, version mismatch, `NotSignedOn`, 64
//! outstanding requests and `Busy` at the 65th, replies over 1 MiB split
//! with `More`, requests over 1 MiB refused, `Cancel` before and after a
//! request started, replies matched by ID whatever order they come in,
//! `Ping` after a quiet spell, and `ServerEnding`.

mod common;

use std::time::Duration;

use common::{cfg, start, start_listening, start_with, until};
use qap_client::proto::ops::command::CommandText;
use qap_client::proto::ops::connection::PingReply;
use qap_client::proto::ops::signon::SignOn;
use qap_client::proto::ops::system::SystemInfo;
use qap_client::proto::{MAX_FRAME, MAX_OUTSTANDING, Op, Service};
use qap_client::{
    Client, Closed, CommandOutcome, Connection, Error, Notice, Reply, Secret, Status,
};
use qapd::{Config, Ending};

#[test]
fn hello_over_tls13() {
    let s = start(cfg());
    let c = Connection::open(&s.target()).unwrap();
    let h = c.hello();
    assert_eq!(h.version, 6);
    assert_eq!(h.system_name, "QSHQ400");
    assert_eq!(h.release, "V1R0M0");
    assert_eq!(h.services, Service::of_version(6));
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
    // The server speaks versions 1 to 9.
    t.options.min_version = 10;
    t.options.max_version = 12;
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
    let c = s.signed_on();
    // The gate holds SystemInfo; each runs on a thread of its own.
    s.c.gate.set(false);
    let held: Vec<_> = (0..MAX_OUTSTANDING)
        .map(|_| c.request(Op::SystemInfo, &()).unwrap())
        .collect();
    let r = c
        .request(Op::SystemInfo, &())
        .unwrap()
        .wait(Duration::from_secs(5))
        .unwrap();
    assert_eq!(r.status, Status::Busy);
    let e = r.decode::<SystemInfo>().unwrap_err();
    assert!(e.is_busy(), "{e:?}");

    // call() waits and tries again until there is room.
    let gate = std::thread::spawn({
        let c = std::sync::Arc::clone(&s.c);
        move || {
            std::thread::sleep(Duration::from_millis(300));
            c.gate.set(true);
        }
    });
    let info: SystemInfo = c.call(Op::SystemInfo, &()).unwrap();
    assert_eq!(info.system_name, "QSHQ400");
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

fn dlyjob(secs: u32) -> CommandText {
    CommandText {
        command: format!("DLYJOB DLY({secs})"),
    }
}

#[test]
fn cancel() {
    let s = start(cfg());
    let c = s.signed_on();
    // Commands run one at a time: the first runs, the second waits behind
    // it and has not started.
    let running = c.request(Op::RunCommand, &dlyjob(30)).unwrap();
    let queued = c.request(Op::RunCommand, &dlyjob(30)).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    queued.cancel();
    running.cancel();
    // Cancelled before it started: no body.
    let r = queued.wait(Duration::from_secs(5)).unwrap();
    assert_eq!(r.status, Status::Cancelled);
    assert!(r.body.is_empty());
    // Cancelled once started: the command's own reply, the cancel's
    // message in it.
    let o = CommandOutcome::from_reply(running.wait(Duration::from_secs(5)).unwrap()).unwrap();
    assert_eq!(o.status, Status::Cancelled);
    assert_eq!(o.job.unwrap().name, "QAPCNN");
    assert_eq!(o.messages.last().unwrap().id, "CPF1907");
    assert_eq!(o.escape.unwrap().id, "CPF1907");
    // A cancel naming nothing outstanding is ignored.
    let done = c.request(Op::Ping, &()).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    done.cancel();
    let _: PingReply = c.call(Op::Ping, &()).unwrap();
}

#[test]
fn a_reply_too_slow_is_cancelled() {
    let s = start(cfg());
    let c = s.signed_on();
    s.c.gate.set(false);
    let p = c.request(Op::SystemInfo, &()).unwrap();
    assert!(matches!(
        p.wait(Duration::from_millis(100)),
        Err(Error::TimedOut)
    ));
    s.c.gate.set(true);
    let _: SystemInfo = c.call(Op::SystemInfo, &()).unwrap();
}

#[test]
fn replies_are_matched_by_id_not_by_order() {
    let s = start(cfg());
    let c = s.signed_on();
    // The first is held at the gate; the second runs beside it and its
    // reply comes back first.
    s.c.gate.set(false);
    let first = c.request(Op::SystemInfo, &()).unwrap();
    let second = c.request(Op::Ping, &()).unwrap();
    let ping: PingReply = second
        .wait(Duration::from_secs(5))
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(ping.server_time, 1_793_000_000_000);
    assert!(first.try_wait().is_none(), "the first is still held");
    s.c.gate.set(true);
    let info: SystemInfo = first
        .wait(Duration::from_secs(5))
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(info.system_name, "QSHQ400");
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

/// Whether a new connection to `t` is refused, as a connect to an ended
/// server's closed port is.
fn refused(t: &qap_client::Target) -> bool {
    matches!(
        Connection::open(t),
        Err(Error::Connect(qsa_net::ConnectError::Refused(_)))
    )
}

#[test]
fn server_ending() {
    let s = start_listening(cfg());
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
    assert!(matches!(
        s.next_close(),
        Some((_, qapd::Closed::ServerEnding))
    ));
    // While the server is ended (ENDTCPSVR) its port is closed: connects
    // are refused, and so is a reconnect when asked.
    assert!(!s.listener.accepting());
    until("connects refused", || refused(&s.target()));
    assert!(matches!(
        c.reconnect(),
        Err(Error::Connect(qsa_net::ConnectError::Refused(_)))
    ));
    // Started again (STRTCPSVR) on the same port: reconnect when asked.
    s.c.restart().unwrap();
    assert!(s.listener.accepting());
    let mut hello = None;
    until("the port open again", || {
        hello = c.reconnect().ok().map(|h| h.system_name.clone());
        hello.is_some()
    });
    assert_eq!(hello.as_deref(), Some("QSHQ400"));
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    assert_eq!(c.system_info().unwrap().system_name, "QSHQ400");
}

#[test]
fn a_connection_opened_before_an_end_ends_with_it_after_a_quick_restart() {
    let s = start_listening(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    // The restart comes before the connection's next poll: it still ends
    // with the server, as ENDTCPSVR then STRTCPSVR has it.
    s.c.end(Ending::Server);
    s.c.restart().unwrap();
    let mut notices = Vec::new();
    until("ServerEnding", || {
        notices.extend(c.poll());
        !notices.is_empty()
    });
    assert!(
        matches!(&notices[..], [Notice::ServerEnding(Some(m))] if m.id == "TCP1A0A"),
        "{notices:?}"
    );
    assert!(!c.is_signed_on());
    // A new connection is taken at once.
    let n = Connection::open(&s.target()).unwrap();
    assert_eq!(n.version(), 6);
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

#[test]
fn a_long_request_is_never_cancelled_by_a_timeout() {
    let s = start(cfg());
    let c = s.signed_on();
    let body = CommandText {
        command: "DLYJOB DLY(1)".into(),
    };
    // The wait runs out; the command does not end with it.
    let p = c.request(Op::RunCommand, &body).unwrap();
    assert!(p.wait_for(Duration::from_millis(100)).is_none());
    assert!(matches!(
        p.wait(Duration::from_millis(100)),
        Err(Error::TimedOut)
    ));
    // Had it been cancelled, the next command would end Cancelled or wait
    // for nothing; it runs after the first, which ran to its end.
    let at = std::time::Instant::now();
    let next = c.request(Op::RunCommand, &body).unwrap();
    let o = CommandOutcome::from_reply(next.wait_done().unwrap()).unwrap();
    assert_eq!(o.status, Status::Ok);
    assert!(
        at.elapsed() >= Duration::from_millis(1700),
        "it waited behind the first"
    );
    // call() waits for a long one however long it takes.
    let mut short = s.target();
    short.options.reply_timeout = Duration::from_millis(100);
    let mut cl = Client::new(short);
    cl.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let r: qap_client::proto::ops::command::CommandReply = cl.call(Op::RunCommand, &body).unwrap();
    assert_eq!(r.job.name, "QAPCNN");
}
