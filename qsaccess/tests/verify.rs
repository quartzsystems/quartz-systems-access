// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Verify Connection's checks against QS/400's QAP engine (qap-client's
//! test server) and a plain listener standing in for Telnet.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::net::TcpListener;
use std::time::Duration;

use common::{Server, cfg, start, start_with};
use qap_client::Secret;
use qsa_config::System;
use qsa_msg::id;
use qsaccess::verify::checks::{self, Check, Outcome, Plan};

/// A system on loopback: Telnet on `telnet`, QAP on the server's port.
fn plan(s: &Server, telnet: u16, sign_on: Option<(&str, &str)>) -> Plan {
    let mut system = System::new("127.0.0.1");
    system.ports.telnet = telnet;
    system.ports.qap = s.port;
    let target = s.target();
    Plan {
        telnet_tls: target.tls(),
        timeout: Duration::from_secs(5),
        sign_on: sign_on.map(|(u, p)| (u.to_owned(), Secret::from(p))),
        system,
        target,
    }
}

fn run(p: &Plan) -> Vec<(Check, Outcome)> {
    let mut out = Vec::new();
    checks::run(p, |c, o| out.push((c, o)));
    assert_eq!(
        out.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        Check::ALL,
        "every check reported once, in order"
    );
    out
}

fn result(r: &[(Check, Outcome)], c: Check) -> &Outcome {
    &r.iter().find(|(x, _)| *x == c).unwrap().1
}

fn word(o: &Outcome) -> &'static str {
    match o {
        Outcome::Ok(_) => "OK",
        Outcome::Failed { .. } => "Failed",
        Outcome::Skipped(_) => "Skipped",
    }
}

fn words(r: &[(Check, Outcome)]) -> Vec<&'static str> {
    r.iter().map(|(_, o)| word(o)).collect()
}

fn failed_id(o: &Outcome) -> &str {
    match o {
        Outcome::Failed { message, .. } => &message.id,
        other => panic!("{other:?}"),
    }
}

fn telnet() -> (TcpListener, u16) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    (l, port)
}

#[test]
fn everything_passes_with_a_test_sign_on() {
    let s = start(cfg());
    let (_l, port) = telnet();
    let r = run(&plan(&s, port, Some(("BOB", "SECRET1"))));
    assert_eq!(
        words(&r),
        ["OK", "OK", "Skipped", "OK", "OK", "OK", "OK"],
        "{r:#?}"
    );
    assert!(result(&r, Check::QapTls).detail().starts_with("TLSv1_3"));
    assert_eq!(
        result(&r, Check::Hello).detail(),
        "Version 2, system QSHQ400, release V1R0M0."
    );
    assert_eq!(
        result(&r, Check::SignOn).detail(),
        "Sign-on to 127.0.0.1 as BOB succeeded. Signed off."
    );
    common::until("the sign-off", || s.c.sessions.lock().unwrap().is_empty());
    assert_eq!(checks::summary("127.0.0.1", &r).id, id::VERIFIED);
}

#[test]
fn a_failed_test_sign_on_is_a_warning_and_shows_the_system_message() {
    let s = start(cfg());
    let (_l, port) = telnet();
    let r = run(&plan(&s, port, Some(("BOB", "WRONG"))));
    let o = result(&r, Check::SignOn);
    assert_eq!(failed_id(o), id::SIGNON_FAILED);
    assert_eq!(o.detail(), "CPF1107: CPF1107  (system)");
    let Outcome::Failed { server, .. } = o else {
        unreachable!()
    };
    assert_eq!(server.as_ref().unwrap().id, "CPF1107");
    // It was a real sign-on attempt: counted and audited.
    assert_eq!(s.c.get("BOB").invalid, 1);
    assert_eq!(*s.c.audit.lock().unwrap(), ["PW P BOB"]);
    assert_eq!(
        checks::summary("127.0.0.1", &r).id,
        id::VERIFIED_WITH_WARNINGS
    );
}

#[test]
fn an_expired_password_is_reported() {
    let s = start(cfg());
    s.c.edit("BOB", |u| u.expired = true);
    let (_l, port) = telnet();
    let r = run(&plan(&s, port, Some(("BOB", "SECRET1"))));
    assert_eq!(failed_id(result(&r, Check::SignOn)), id::PASSWORD_EXPIRED);
}

#[test]
fn the_qap_server_not_started() {
    let s = start(cfg());
    let (_l, port) = telnet();
    let mut p = plan(&s, port, None);
    let (closed, closed_port) = telnet();
    drop(closed);
    p.system.ports.qap = closed_port;
    p.target.port = closed_port;
    let r = run(&p);
    assert_eq!(
        words(&r),
        [
            "OK", "OK", "Skipped", "Failed", "Skipped", "Skipped", "Skipped"
        ]
    );
    let o = result(&r, Check::Qap);
    assert_eq!(failed_id(o), id::QAP_NOT_REACHED);
    let Outcome::Failed { message, .. } = o else {
        unreachable!()
    };
    assert!(message.recovery.contains("STRTCPSVR SERVER(*QAP)"));
    assert!(message.recovery.contains("QS400_ACCESS_SERVER"));
    assert_eq!(checks::summary("127.0.0.1", &r).id, id::VERIFY_FAILED);
}

#[test]
fn a_server_without_tls13_is_refused_and_says_so() {
    const TLS12: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS12];
    let s = start_with(cfg(), qapd::testing::System::new(), TLS12);
    let (_l, port) = telnet();
    let r = run(&plan(&s, port, None));
    let o = result(&r, Check::QapTls);
    assert_eq!(failed_id(o), id::QAP_TLS_FAILED);
    let Outcome::Failed { message, .. } = o else {
        unreachable!()
    };
    assert!(message.recovery.contains("TLS 1.3 only"));
    assert_eq!(word(result(&r, Check::Hello)), "Skipped");
}

#[test]
fn an_untrusted_certificate_is_kept_for_the_trust_flow() {
    let s = start(cfg());
    let (_l, port) = telnet();
    let mut p = plan(&s, port, None);
    p.target.trust = qsa_net::Trust::default();
    let r = run(&p);
    let o = result(&r, Check::QapTls);
    assert_eq!(failed_id(o), id::CERT_UNTRUSTED);
    let Outcome::Failed { untrusted, .. } = o else {
        unreachable!()
    };
    assert_eq!(
        untrusted.as_ref().unwrap().info.subject,
        "CN=localhost, O=Quartz Systems Test"
    );
}

#[test]
fn telnet_tls_is_checked_when_the_system_uses_it() {
    let s = start(cfg());
    let (_l, port) = telnet();
    let mut p = plan(&s, port, None);
    p.system.use_tls = true;
    // Any TLS server will do for the handshake: use the QAP one.
    p.system.ports.telnet_tls = s.port;
    let r = run(&p);
    assert_eq!(word(result(&r, Check::TelnetTls)), "OK", "{r:#?}");
    p.telnet_tls.trust = qsa_net::Trust::default();
    let r = run(&p);
    assert_eq!(failed_id(result(&r, Check::TelnetTls)), id::CERT_UNTRUSTED);
}

#[test]
fn telnet_closed_and_name_not_found() {
    let s = start(cfg());
    let (closed, port) = telnet();
    drop(closed);
    let r = run(&plan(&s, port, None));
    assert_eq!(failed_id(result(&r, Check::Telnet)), id::CONNECTION_REFUSED);
    assert_eq!(word(result(&r, Check::Hello)), "OK");

    let mut p = plan(&s, port, None);
    p.target.host = "no-such-host.invalid".into();
    let r = run(&p);
    assert_eq!(failed_id(result(&r, Check::Dns)), id::HOST_NOT_FOUND);
    assert!(r[1..].iter().all(|(_, o)| word(o) == "Skipped"));
}

#[test]
fn a_version_the_server_does_not_speak() {
    let s = start(cfg());
    let (_l, port) = telnet();
    let mut p = plan(&s, port, None);
    p.target.options.min_version = 3;
    p.target.options.max_version = 4;
    let r = run(&p);
    let o = result(&r, Check::Hello);
    assert_eq!(failed_id(o), id::QAP_VERSION_UNSUPPORTED);
    assert_eq!(
        o.detail(),
        "QAD4055: QAP server on 127.0.0.1 supports none of protocol versions 3 to 4."
    );
}
