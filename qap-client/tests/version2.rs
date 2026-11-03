// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QAP version 2 against QS/400's engine and test system
//! (`qapd::testing`), every status each operation answers:
//!
//! - `RunCommand`: `Ok`, `Error`, `NotFound`, `NotAuthorized` (to the
//!   command and to what it names), `Cancelled`, and a command that runs
//!   only interactively;
//! - `CheckCommand`: `Ok`, `Error`, `NotFound`;
//! - `DescribeCommand`: `Ok`, `NotFound`, `NotAuthorized`;
//! - `PasswordStatus`;
//! - `TelnetTicket`: `Ok` and `NotAuthorized`, and the ticket as the Telnet
//!   server takes it: single use, expiry, another profile, plain Telnet.

mod common;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use common::{Server, cfg, start};
use qap_client::proto::Service;
use qap_client::proto::ops::command::{ParmKind, Role, Show};
use qap_client::proto::types::ObjectName;
use qap_client::{Client, CommandOutcome, Secret, Status};
use qapd::testing::EPOCH;

fn signed_on(s: &Server, user: &str, password: &str) -> Client {
    let mut c = Client::new(s.target());
    c.sign_on(user, &Secret::from(password)).unwrap();
    c
}

fn run(c: &mut Client, command: &str) -> CommandOutcome {
    let p = c.run_command(command).unwrap();
    CommandOutcome::from_reply(p.wait(Duration::from_secs(10)).unwrap()).unwrap()
}

fn ids(o: &CommandOutcome) -> Vec<&str> {
    o.messages.iter().map(|m| m.id.as_str()).collect()
}

fn name(lib: &str, n: &str) -> ObjectName {
    ObjectName {
        library: lib.into(),
        name: n.into(),
    }
}

#[test]
fn the_command_service_is_there_from_version_2() {
    let s = start(cfg());
    let mut c = Client::new(s.target());
    c.connect().unwrap();
    assert_eq!(c.version(), Some(5));
    assert!(c.hello().unwrap().services.contains(&Service::Command));
    // Not before sign-on.
    let e = c.check_command("CRTLIB LIB(X)").unwrap_err();
    assert_eq!(e.status(), Some(Status::NotSignedOn));
    assert_eq!(
        c.password_status().unwrap_err().status(),
        Some(Status::NotSignedOn)
    );
}

#[test]
fn run_command_ok_and_error() {
    let s = start(cfg());
    let mut c = signed_on(&s, "BOB", "SECRET1");
    let ok = run(&mut c, "CRTLIB LIB(QATESTLIB)");
    assert_eq!(ok.status, Status::Ok);
    assert_eq!(ids(&ok), ["CPC2102"]);
    assert!(
        !ok.messages[0].help.is_empty(),
        "second-level text included"
    );
    assert!(ok.escape.is_none());
    let job = ok.job.unwrap();
    // The connection job, under its own user, not the profile.
    assert_eq!((job.name.as_str(), job.user.as_str()), ("QAPCNN", "QUSER"));
    assert_eq!(job.number.len(), 6);
    assert_eq!(*s.c.libraries.lock().unwrap(), ["QATESTLIB"]);
    // Again: Error, the escape the reply's message, the body still there.
    let again = run(&mut c, "CRTLIB LIB(QATESTLIB)");
    assert_eq!(again.status, Status::Error);
    assert_eq!(again.escape.as_ref().unwrap().id, "CPF2111");
    assert_eq!(ids(&again), ["CPF2111"]);
    assert!(again.job.is_some());
    // A required parameter missing.
    let missing = run(&mut c, "CRTLIB");
    assert_eq!(missing.status, Status::Error);
    assert_eq!(ids(&missing), ["CPD0071", "CPF0006"]);
}

#[test]
fn run_command_not_found_not_authorized_and_interactive_only() {
    let s = start(cfg());
    let mut c = signed_on(&s, "BOB", "SECRET1");
    let nf = run(&mut c, "NOSUCH X(1)");
    assert_eq!(nf.status, Status::NotFound);
    assert_eq!(ids(&nf)[0], "CPD0030");
    // Not authorized to the command, and to what it names.
    let na = run(&mut c, "DLTUSRPRF USRPRF(ANN)");
    assert_eq!(na.status, Status::NotAuthorized);
    assert_eq!(na.escape.unwrap().id, "CPF9802");
    let obj = run(&mut c, "DSPOBJD OBJ(QSYS/SECRET) OBJTYPE(*FILE)");
    assert_eq!(obj.status, Status::NotAuthorized);
    assert_eq!(obj.escape.unwrap().id, "CPF9802");
    // Only interactively: Error, with the system's escape as it is.
    let seu = run(&mut c, "STRSEU");
    assert_eq!(seu.status, Status::Error);
    assert_eq!(ids(&seu), ["CPD0031", "CPF0006"]);
    assert_eq!(seu.escape.unwrap().id, "CPF0006");
}

#[test]
fn cancel_ends_a_running_command() {
    let s = start(cfg());
    let mut c = signed_on(&s, "BOB", "SECRET1");
    let p = c.run_command("DLYJOB DLY(60)").unwrap();
    std::thread::sleep(Duration::from_millis(150));
    let at = Instant::now();
    p.cancel();
    let o = CommandOutcome::from_reply(p.wait(Duration::from_secs(10)).unwrap()).unwrap();
    assert_eq!(o.status, Status::Cancelled);
    assert!(at.elapsed() < Duration::from_secs(5), "ended where it was");
    // Cancelled once running, the reply is still the command's: its job and
    // every message, the one the cancel caused last.
    assert_eq!(ids(&o), ["CPF1907"]);
    assert_eq!(o.job.unwrap().name, "QAPCNN");
    assert_eq!(o.escape.unwrap().id, "CPF1907");
    // The session and connection go on.
    assert_eq!(run(&mut c, "DLYJOB DLY(1)").status, Status::Ok);
}

#[test]
fn check_command_statuses() {
    let s = start(cfg());
    let mut c = signed_on(&s, "BOB", "SECRET1");
    let ok = c.check_command("CRTLIB LIB(X)").unwrap();
    assert!(ok.valid() && ok.messages.is_empty());
    assert!(s.c.libraries.lock().unwrap().is_empty(), "nothing ran");
    let bad = c.check_command("CRTLIB").unwrap();
    assert_eq!(bad.status, Status::Error);
    assert_eq!(bad.messages[0].id, "CPD0071");
    let nf = c.check_command("NOSUCH").unwrap();
    assert_eq!(nf.status, Status::NotFound);
    assert_eq!(nf.messages[0].id, "CPD0030");
    // Interactive-only commands pass: the check is the syntax only.
    assert!(c.check_command("STRSEU").unwrap().valid());
}

#[test]
fn describe_command_statuses() {
    let s = start(cfg());
    let mut c = signed_on(&s, "BOB", "SECRET1");
    let d = c.describe_command(name("*LIBL", "DSPOBJD")).unwrap();
    assert_eq!(d.name, name("QSYS", "DSPOBJD"));
    assert_eq!(d.text, "Display Object Description");
    assert!(d.help.is_empty(), "QS/400 ships no command help yet");
    // A qualified name: the parameter, then name and library.
    let obj = &d.nodes[0];
    assert_eq!(
        (obj.keyword.as_str(), obj.role, obj.parent),
        ("OBJ", Role::Parameter, None)
    );
    let q: Vec<_> = d
        .nodes
        .iter()
        .filter(|n| n.parent == Some(obj.index))
        .collect();
    assert_eq!(q.len(), 2);
    assert!(
        q.iter()
            .all(|n| n.role == Role::Qualifier && n.index > obj.index)
    );
    assert_eq!(q[1].default.as_deref(), Some("*LIBL"));
    let ty = d.nodes.iter().find(|n| n.keyword == "OBJTYPE").unwrap();
    assert_eq!(ty.max_values, 10);
    // An element list under Additional Parameters.
    let pf = c.describe_command(name("QSYS", "CRTPF")).unwrap();
    let size = pf.nodes.iter().find(|n| n.keyword == "SIZE").unwrap();
    assert_eq!(size.show, Show::Additional);
    let el: Vec<_> = pf
        .nodes
        .iter()
        .filter(|n| n.parent == Some(size.index))
        .collect();
    assert_eq!(el.len(), 3);
    assert!(
        el.iter()
            .all(|n| n.role == Role::Element && n.kind == ParmKind::Integer)
    );
    // NotFound and NotAuthorized, with the system's message.
    let e = c.describe_command(name("*LIBL", "NOSUCH")).unwrap_err();
    assert_eq!(e.status(), Some(Status::NotFound));
    assert_eq!(e.server_message().unwrap().id, "CPD0030");
    let e = c.describe_command(name("QGPL", "CRTLIB")).unwrap_err();
    assert_eq!(e.status(), Some(Status::NotFound), "not in that library");
    let e = c.describe_command(name("*LIBL", "DLTUSRPRF")).unwrap_err();
    assert_eq!(e.status(), Some(Status::NotAuthorized));
    assert_eq!(e.server_message().unwrap().id, "CPF9802");
}

#[test]
fn password_status() {
    let s = start(cfg());
    let mut warn = signed_on(&s, "WARN", "WARNPW1");
    let p = warn.password_status().unwrap();
    let days3 = i64::try_from(EPOCH + 3 * 86_400).unwrap() * 1000;
    assert_eq!(p.expires, Some(days3));
    assert_eq!(p.warning_days, 7);
    // The server's clock, as the client keeps it, is the test system's.
    let now = warn.server_now_ms().unwrap();
    let epoch = i64::try_from(EPOCH).unwrap() * 1000;
    assert!((now - epoch).abs() < 60_000, "{now} vs {epoch}");
    let mut bob = signed_on(&s, "BOB", "SECRET1");
    assert_eq!(bob.password_status().unwrap().expires, None);
}

#[test]
fn telnet_ticket_single_use_expiry_user_and_tls() {
    let s = start(cfg());
    let mut c = signed_on(&s, "BOB", "SECRET1");
    let hex = |t: &qap_client::Token| -> Vec<u8> {
        t.0.iter()
            .flat_map(|b| format!("{b:02X}").into_bytes())
            .collect()
    };
    let t = c.telnet_ticket().unwrap();
    assert_eq!(t.expires_secs, 60);
    assert!(!format!("{t:?}").contains(&format!("{:02X}", t.ticket.0[0]).repeat(2)));
    let pw = hex(&t.ticket);
    assert_eq!(pw.len(), 64);
    assert!(s.c.redeem_ticket("BOB", &pw, true));
    assert!(!s.c.redeem_ticket("BOB", &pw, true), "single use");
    let pw = hex(&c.telnet_ticket().unwrap().ticket);
    assert!(!s.c.redeem_ticket("BOB", &pw, false), "plain Telnet");
    assert!(
        !s.c.redeem_ticket("BOB", &pw, true),
        "used up by the plain try"
    );
    let pw = hex(&c.telnet_ticket().unwrap().ticket);
    assert!(!s.c.redeem_ticket("WARN", &pw, true), "another profile");
    let pw = hex(&c.telnet_ticket().unwrap().ticket);
    s.c.advance(60);
    assert!(!s.c.redeem_ticket("BOB", &pw, true), "expired");
}

#[test]
fn telnet_ticket_not_authorized() {
    let s = start(cfg());
    s.c.auto_signon.store(false, Ordering::SeqCst);
    let mut c = signed_on(&s, "BOB", "SECRET1");
    let e = c.telnet_ticket().unwrap_err();
    assert_eq!(e.status(), Some(Status::NotAuthorized));
    assert_eq!(e.server_message().unwrap().id, "TCP1A55");
    // A disabled profile may not either, whatever QRMTSIGN says.
    s.c.auto_signon.store(true, Ordering::SeqCst);
    s.c.edit("BOB", |u| u.disabled = true);
    assert_eq!(
        c.telnet_ticket().unwrap_err().status(),
        Some(Status::NotAuthorized)
    );
}
