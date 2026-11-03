// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! No password, session token or Telnet ticket in any log, recording or
//! crash output. One full run against QS/400's test system, with every
//! trace line at every level going to the program's log file as `main` sends
//! it: sign-on (and a refused one), a password change, a quiet resume after
//! the idle close, commands run, checked and described, the password's
//! expiry, a Telnet ticket sent through a 5250 session to a stub Telnet
//! server with the session recorded. Then the log file, the recording and
//! the `Debug` text of what the run handled (what a panic would print) are
//! searched for every secret, as text, as hexadecimal and as raw bytes.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use common::{Server, start, until};
use qap_client::proto::types::ObjectName;
use qap_client::{CommandOutcome, Secret};
use qsa_config::System;
use qsa_config::settings::redact_command;
use qsa_config::system::CaCertificate;
use qsaccess::qap::{Registry, expiry_due, lock};
use tn5250::negotiate::{AutoSignOn, Event, Request, SEND, ScreenSize, Ticket, USERVAR, VAR};
use tn5250::record::{Record, opcode};
use tn5250::replay::Recorder;
use tn5250::session::{Session, Target};
use tn5250::telnet::{BINARY, DO, EOR, EOR_OPTION, IAC, NEW_ENVIRON, SB, SE, TERMINAL_TYPE, WILL};

const PASSWORDS: [&str; 4] = ["SECRET1", "SECRET2", "WRONGPW9", "HUNTER22"];

fn temp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("qsa-secrets-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

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

/// A Telnet server that asks for the environment with an RFC 4777 seed,
/// turns records on and sends one 5250 record; what the client sent it.
fn telnet_stub() -> (u16, std::thread::JoinHandle<Vec<u8>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let t = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        let mut got = Vec::new();
        let read = |s: &mut std::net::TcpStream, got: &mut Vec<u8>| {
            let mut b = [0u8; 4096];
            let end = Instant::now() + Duration::from_millis(400);
            while Instant::now() < end {
                if let Ok(n) = s.read(&mut b) {
                    if n == 0 {
                        break;
                    }
                    got.extend_from_slice(&b[..n]);
                }
            }
        };
        s.write_all(&[IAC, DO, NEW_ENVIRON, IAC, DO, TERMINAL_TYPE])
            .unwrap();
        let mut send = vec![IAC, SB, NEW_ENVIRON, SEND, USERVAR];
        send.extend_from_slice(tn5250::wire::RANDOM_SEED.as_bytes());
        send.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, VAR, USERVAR, IAC, SE]);
        s.write_all(&send).unwrap();
        read(&mut s, &mut got);
        s.write_all(&[
            IAC, DO, EOR_OPTION, IAC, WILL, EOR_OPTION, IAC, DO, BINARY, IAC, WILL, BINARY,
        ])
        .unwrap();
        let mut rec = Record::new(opcode::PUT_GET, vec![0x04, 0x40]).encode();
        rec.extend_from_slice(&[IAC, EOR]);
        s.write_all(&rec).unwrap();
        read(&mut s, &mut got);
        got
    });
    (port, t)
}

/// Every form a secret's bytes may take in text.
fn forms(bytes: &[u8]) -> Vec<String> {
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    let mut v = vec![hex.clone(), hex.to_lowercase()];
    if let Ok(s) = std::str::from_utf8(bytes) {
        v.push(s.to_owned());
    }
    v
}

#[test]
fn no_secret_in_logs_recordings_or_debug_output() {
    let dir = temp("run");
    qsaccess::log::init(Some(dir.clone()));
    tn5250::log::set_sink(tn5250::log::Level::Trace, |l, text| {
        qsaccess::log::line(&format!("{} {text}", l.name()));
    });
    let mut debug: Vec<String> = Vec::new();

    // Sign on, one refused first.
    let s = start(qapd::Config {
        idle_close: Duration::from_millis(300),
        ..common::cfg()
    });
    let sys = system(&s);
    let mut r = Registry::default();
    let h = r.client(&sys);
    let e = lock(&h)
        .sign_on("BOB", &Secret::from("WRONGPW9"))
        .unwrap_err();
    debug.push(format!("{e:?} {e}"));
    lock(&h).sign_on("BOB", &Secret::from("SECRET1")).unwrap();
    qsaccess::log!("signed on as {}", lock(&h).profile().unwrap().profile);
    // A password change while signed on.
    lock(&h)
        .change_password("BOB", &Secret::from("SECRET1"), &Secret::from("SECRET2"))
        .unwrap();
    // The idle close, then a request that resumes with the token.
    until("the idle close", || {
        s.ended().len() == 1 && lock(&h).connection().is_some_and(|c| c.closed().is_some())
    });
    lock(&h).system_info().unwrap();
    assert!(r.resumed("127.0.0.1"));
    // Commands: run, a password parameter logged as Run Command logs it,
    // checked, described; the expiry.
    let p = lock(&h).run_command("CRTLIB LIB(QATESTLIB)").unwrap();
    let o = CommandOutcome::from_reply(p.wait(Duration::from_secs(10)).unwrap()).unwrap();
    debug.push(format!("{o:?}"));
    qsaccess::log!(
        "Run Command on 127.0.0.1: {}",
        redact_command("CHGUSRPRF USRPRF(BOB) PASSWORD(HUNTER22)")
    );
    debug.push(format!("{:?}", lock(&h).check_command("CRTLIB").unwrap()));
    let d = lock(&h)
        .describe_command(ObjectName {
            library: "*LIBL".into(),
            name: "CRTLIB".into(),
        })
        .unwrap();
    debug.push(format!("{d:?}"));
    debug.push(format!("{:?}", expiry_due(&h).unwrap()));

    // A ticket, sent through a 5250 session that records.
    let t = lock(&h).telnet_ticket().unwrap();
    let ticket = t.ticket.0;
    debug.push(format!("{t:?}"));
    let mut req = Request::display(ScreenSize::S24x80, None, 37);
    req.auto_sign_on = Some(AutoSignOn {
        user: "BOB".into(),
        ticket: Ticket::from_bytes(&ticket),
    });
    debug.push(format!("{req:?}"));
    drop(t);
    let (port, server) = telnet_stub();
    let target = Target {
        host: "127.0.0.1".into(),
        port,
        tls: None,
    };
    let mut session = Session::connect(&target, req).unwrap();
    let recording = dir.join("session.qsr");
    let mut rec = Recorder::create(&recording, ScreenSize::S24x80, 37).unwrap();
    session.set_recorder(Box::new(move |dir, bytes| rec.record(dir, bytes)));
    let end = Instant::now() + Duration::from_secs(5);
    let mut records = 0;
    while records == 0 && Instant::now() < end {
        for ev in session.poll(Duration::from_millis(20)).unwrap_or_default() {
            if matches!(ev, Event::Record(_)) {
                records += 1;
            }
        }
    }
    assert_eq!(records, 1, "the stub's record arrived");
    session.close();
    let wire = server.join().unwrap();
    // It did go out, to the server and nowhere else.
    let hex: String = ticket.iter().map(|b| format!("{b:02X}")).collect();
    assert!(
        wire.windows(64).any(|w| w == hex.as_bytes()),
        "the ticket reached the Telnet server"
    );
    debug.push(format!("{:?}", *lock(&h)));
    debug.push(format!("{r:?}"));

    // Version 4: a window's own connection, attached with the token, and
    // SQL on it. (`qsaccess run` sends no token anywhere: see
    // run_request.rs.)
    let mut q = lock(&h).sql_connection().unwrap();
    debug.push(format!("{q:?}"));
    let p = q.prepare("SELECT * FROM HLINV.ASSETS").unwrap();
    debug.push(format!("{p:?}"));
    let o = q.execute_now(p.statement, Vec::new()).unwrap();
    debug.push(format!("{o:?}"));
    q.close();

    // Search.
    let log = std::fs::read_to_string(dir.join(qsaccess::log::LOG_FILE)).unwrap();
    assert!(
        log.contains("tn5250e: send SB NEW-ENVIRON IS"),
        "the trace was on"
    );
    assert!(
        log.contains("PASSWORD(*)"),
        "the password parameter was masked"
    );
    let recorded = std::fs::read_to_string(&recording).unwrap();
    assert!(recorded.lines().count() >= 2, "{recorded}");
    let token_tail = [0x5Au8; 28];
    let mut secrets: Vec<String> = PASSWORDS.iter().map(|p| (*p).to_owned()).collect();
    secrets.extend(forms(&ticket));
    secrets.extend(forms(&token_tail));
    for (what, text) in [
        ("the log", log.as_str()),
        ("the recording", recorded.as_str()),
        ("Debug output", &debug.join("\n")),
    ] {
        for secret in &secrets {
            assert!(!text.contains(secret.as_str()), "{what} holds {secret:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
