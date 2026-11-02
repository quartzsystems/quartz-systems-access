// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Sign-on and sessions against QS/400's engine: the display's checks
//! (`QMAXSIGN`, disabled and `*NONE` profiles, audit), an expired password
//! then the change, a change while signed on, resume within and after the
//! idle timeout, sign-off, and `SessionEnded`.

mod common;

use std::time::Duration;

use common::{cfg, start, until};
use qap_client::{Client, Error, Notice, Secret, Status};
use qapd::{Config, Ending};

fn refused(e: &Error) -> (Status, String) {
    (e.status().unwrap(), e.server_message().unwrap().id.clone())
}

#[test]
fn sign_on_and_off() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    let p = c.sign_on("bob", &Secret::from("SECRET1")).unwrap();
    assert_eq!(p.profile, "BOB");
    assert_eq!(p.idle_timeout_secs, 600);
    assert_eq!(p.home_directory, "/home/BOB");
    // The token is held apart, never in the profile kept for display.
    assert_eq!(p.token, qap_client::Token::default());
    assert!(c.is_signed_on());
    assert_eq!(c.system_info().unwrap().serial, "1234567");
    assert!(matches!(
        c.sign_on("BOB", &Secret::from("SECRET1")),
        Err(Error::AlreadySignedOn)
    ));
    c.sign_off().unwrap();
    assert!(!c.is_signed_on());
    assert!(s.c.sessions.lock().unwrap().is_empty());
    assert_eq!(
        c.system_info().unwrap_err().status(),
        Some(Status::NotSignedOn)
    );
}

#[test]
fn qmaxsign_disables_the_profile() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    let bad = Secret::from("WRONG");
    let got: Vec<_> = (0..3)
        .map(|_| refused(&c.sign_on("BOB", &bad).unwrap_err()))
        .collect();
    assert_eq!(
        got,
        [
            (Status::SignOnFailed, "CPF1107".to_string()),
            (Status::SignOnFailed, "CPF1392".to_string()),
            (Status::SignOnFailed, "CPF1393".to_string()),
        ]
    );
    // Disabled now: the right password does not help.
    let e = c.sign_on("BOB", &Secret::from("SECRET1")).unwrap_err();
    assert_eq!(refused(&e), (Status::SignOnFailed, "CPF1394".into()));
    assert_eq!(e.server_message().unwrap().text, "CPF1394 BOB (system)");
    assert!(s.c.get("BOB").disabled);
    assert_eq!(
        *s.c.audit.lock().unwrap(),
        ["PW P BOB", "PW P BOB", "PW P BOB", "PW Q BOB"]
    );
}

#[test]
fn disabled_and_none_profiles() {
    let s = start(cfg());
    s.c.user("ANN", "PASSWD1");
    s.c.edit("ANN", |u| u.disabled = true);
    s.c.user("SVC", "UNUSED1");
    s.c.edit("SVC", |u| u.none = true);
    let mut c = Client::new(s.target());
    let e = c.sign_on("ANN", &Secret::from("PASSWD1")).unwrap_err();
    assert_eq!(refused(&e), (Status::SignOnFailed, "CPF1394".into()));
    let e = c.sign_on("SVC", &Secret::from("UNUSED1")).unwrap_err();
    assert_eq!(refused(&e), (Status::SignOnFailed, "CPF1118".into()));
    let e = c.sign_on("NOBODY", &Secret::from("X")).unwrap_err();
    assert_eq!(refused(&e), (Status::SignOnFailed, "CPF1107".into()));
    assert!(!c.is_signed_on());
}

#[test]
fn expired_password_then_change() {
    let s = start(cfg());
    s.c.edit("BOB", |u| u.expired = true);
    let mut c = Client::new(s.target());
    let e = c.sign_on("BOB", &Secret::from("SECRET1")).unwrap_err();
    assert_eq!(refused(&e), (Status::PasswordExpired, "CPD9BAE".into()));
    assert!(!c.is_signed_on());
    // The system's rules refuse a short one, with its message.
    let e = c
        .change_password("BOB", &Secret::from("SECRET1"), &Secret::from("NEW"))
        .unwrap_err();
    assert_eq!(refused(&e), (Status::Error, "CPF2362".into()));
    // A password validation program's refusal comes back as it is.
    *s.c.validation_program_refuses.lock().unwrap() = true;
    let e = c
        .change_password("BOB", &Secret::from("SECRET1"), &Secret::from("SECRET2"))
        .unwrap_err();
    assert_eq!(refused(&e), (Status::Error, "CPF22C6".into()));
    *s.c.validation_program_refuses.lock().unwrap() = false;
    let p = c
        .change_password("BOB", &Secret::from("SECRET1"), &Secret::from("SECRET2"))
        .unwrap();
    assert_eq!(p.profile, "BOB");
    assert!(c.is_signed_on());
    assert_eq!(s.c.get("BOB").password, "SECRET2");
    assert!(!s.c.get("BOB").expired);
    assert_eq!(c.system_info().unwrap().system_name, "QSHQ400");
}

#[test]
fn change_password_while_signed_on() {
    let s = start(cfg());
    s.c.user("ANN", "PASSWD1");
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    // Another profile is refused here, before anything is sent.
    assert!(matches!(
        c.change_password("ANN", &Secret::from("PASSWD1"), &Secret::from("PASSWD2")),
        Err(Error::OtherProfile(u)) if u == "ANN"
    ));
    let e = c
        .change_password("BOB", &Secret::from("WRONG"), &Secret::from("SECRET2"))
        .unwrap_err();
    assert_eq!(refused(&e), (Status::Error, "CPF22E2".into()));
    c.change_password("bob", &Secret::from("SECRET1"), &Secret::from("SECRET2"))
        .unwrap();
    assert_eq!(s.c.get("BOB").password, "SECRET2");
    // The session goes on with its own token.
    assert!(c.is_signed_on());
    assert_eq!(c.system_info().unwrap().system_name, "QSHQ400");
    assert_eq!(s.c.sessions.lock().unwrap().len(), 1);
}

/// A server that closes a connection after 200 ms without frames, and a
/// client that would not ping before then.
fn idle_close_soon() -> (common::Server, Client) {
    let s = start(Config {
        idle_close: Duration::from_millis(200),
        ..cfg()
    });
    let c = Client::new(s.target());
    (s, c)
}

#[test]
fn resume_within_the_idle_timeout() {
    let (s, mut c) = idle_close_soon();
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    until("the idle close", || s.ended().len() == 1);
    s.c.advance(599);
    let mut notices = Vec::new();
    until("the resume", || {
        notices.extend(c.poll());
        !notices.is_empty()
    });
    assert_eq!(notices, [Notice::Resumed]);
    assert!(c.is_signed_on());
    assert_eq!(c.profile().unwrap().profile, "BOB");
    assert_eq!(c.system_info().unwrap().system_name, "QSHQ400");
    assert_eq!(s.c.sessions.lock().unwrap().len(), 1);
}

#[test]
fn a_request_after_the_idle_close_resumes_quietly() {
    let (s, mut c) = idle_close_soon();
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    until("the idle close", || {
        c.connection().unwrap().closed().is_some() && s.ended().len() == 1
    });
    assert_eq!(c.system_info().unwrap().system_name, "QSHQ400");
    assert!(c.poll().is_empty());
}

#[test]
fn resume_after_the_idle_timeout_needs_a_sign_on() {
    let (s, mut c) = idle_close_soon();
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    until("the idle close", || s.ended().len() == 1);
    s.c.advance(601);
    let mut notices = Vec::new();
    until("the resume", || {
        notices.extend(c.poll());
        !notices.is_empty()
    });
    match &notices[..] {
        [Notice::SignOnNeeded(Some(m))] => assert_eq!(m.id, "TCP1A53"),
        other => panic!("{other:?}"),
    }
    assert!(!c.is_signed_on());
    assert!(s.c.sessions.lock().unwrap().is_empty());
    // The new connection is there for the new sign-on.
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    assert_eq!(c.system_info().unwrap().system_name, "QSHQ400");
}

#[test]
fn an_operator_ends_the_session() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    s.c.end(Ending::Operator);
    let mut notices = Vec::new();
    until("SessionEnded", || {
        notices.extend(c.poll());
        !notices.is_empty()
    });
    match &notices[..] {
        [
            Notice::SessionEnded {
                by_operator: true,
                message: Some(m),
            },
        ] => {
            assert_eq!(m.id, "TCP1A54");
            assert_eq!(m.text, "TCP1A54 BOB (lang of BOB)");
        }
        other => panic!("{other:?}"),
    }
    assert!(!c.is_signed_on());
    assert!(s.c.sessions.lock().unwrap().is_empty());
    // Not ServerEnding: a new sign-on reconnects by itself.
    until("the close", || s.ended().len() == 1);
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    assert!(c.is_signed_on());
}

#[test]
fn secrets_do_not_print() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    let shown = format!("{c:?}");
    assert!(!shown.contains("SECRET1"), "{shown}");
    assert!(shown.contains("Token(..)"), "{shown}");
}
