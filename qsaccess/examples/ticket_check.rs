// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Live check of 5250 automatic sign-on by QAP ticket against a real
//! system, for TESTING.md's Phase 5 list. It uses the system as configured
//! in this user's System Configurations (address, ports, trust):
//!
//! 1. signs on over QAP and asks for a ticket;
//! 2. connects Telnet over TLS with it: should reach a screen without a
//!    password field (the profile's initial menu);
//! 3. connects again with the same ticket: should get the sign-on display
//!    (a ticket is used up by its first use);
//! 4. asks for a new ticket and offers it over plain Telnet: should get the
//!    sign-on display (tickets are taken only over TLS).
//!
//! ```text
//! QSA_CHECK_PASSWORD=... cargo run -p qsaccess --example ticket_check -- 10.42.70.150 QATEST
//! ```
//!
//! The password is read from `QSA_CHECK_PASSWORD` and never printed; no
//! ticket is printed either. The QAP session is signed off at the end.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use qap_client::{Client, Secret};
use qsa_config::ConfigDir;
use qsa_net::TlsOptions;
use tn5250::negotiate::{AutoSignOn, Event, Request, ScreenSize, Ticket};
use tn5250::session::{Session, Target};
use tn5250::telnet::CONNECT_TIMEOUT;
use tn5250::terminal::Terminal;

/// What a connection showed first.
#[derive(Debug, PartialEq, Eq)]
enum Shown {
    /// A screen with input fields and no password field.
    SignedOn,
    /// The sign-on display: a non-display (password) field.
    SignOnDisplay,
    /// Nothing usable in time, or the connection failed.
    Nothing(String),
}

fn connect(host: &str, port: u16, tls: Option<TlsOptions>, auto: AutoSignOn) -> Shown {
    let mut request = Request::display(ScreenSize::S24x80, None, 37);
    request.auto_sign_on = Some(auto);
    let target = Target {
        host: host.to_owned(),
        port,
        tls,
    };
    let mut s = match Session::connect(&target, request) {
        Ok(s) => s,
        Err(e) => return Shown::Nothing(e.to_string()),
    };
    let mut term = Terminal::new(ScreenSize::S24x80, 37);
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        let events = match s.poll(Duration::from_millis(100)) {
            Ok(e) => e,
            Err(e) => return Shown::Nothing(e.to_string()),
        };
        for ev in events {
            if let Event::Record(r) = ev {
                let applied = term.receive(&r);
                for reply in &applied.replies {
                    let _ = s.send(reply);
                }
                let fields: Vec<bool> = term
                    .screen
                    .input_fields()
                    .map(|(_, f)| f.nondisplay())
                    .collect();
                if !fields.is_empty() {
                    s.close();
                    return if fields.contains(&true) {
                        Shown::SignOnDisplay
                    } else {
                        Shown::SignedOn
                    };
                }
            }
        }
    }
    s.close();
    Shown::Nothing("no screen with input fields in 20 seconds".into())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [system, user] = args.as_slice() else {
        eprintln!("usage: ticket_check SYSTEM USER (password in QSA_CHECK_PASSWORD)");
        return ExitCode::from(2);
    };
    let Ok(password) = std::env::var("QSA_CHECK_PASSWORD") else {
        eprintln!("set QSA_CHECK_PASSWORD to {user}'s password");
        return ExitCode::from(2);
    };
    let Some(dir) = ConfigDir::default_location() else {
        eprintln!("no settings folder");
        return ExitCode::from(2);
    };
    let (systems, _) = dir.load_systems();
    let Some(sys) = systems.get(system).cloned() else {
        eprintln!("{system} is not in System Configurations");
        return ExitCode::from(2);
    };
    let host = qsaccess::qap::target_for(&sys).host;
    let tls = Some(TlsOptions {
        trust: qsaccess::trust::for_system(&sys),
        system_roots: true,
        timeout: CONNECT_TIMEOUT,
        tls13_only: false,
    });

    let mut c = Client::new(qsaccess::qap::target_for(&sys));
    if let Err(e) = c.sign_on(user, &Secret::new(password)) {
        eprintln!("QAP sign-on: {e}");
        return ExitCode::FAILURE;
    }
    println!("QAP: signed on to {} (version {:?})", sys.name, c.version());
    let mut ok = true;
    let mut check = |what: &str, got: Shown, want: Shown| {
        let pass = got == want;
        ok &= pass;
        println!(
            "{} {what}: {got:?} (expected {want:?})",
            if pass { "PASS" } else { "FAIL" }
        );
    };

    let first = match c.telnet_ticket() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("TelnetTicket: {e}");
            return ExitCode::FAILURE;
        }
    };
    let auto = |t: &[u8; 32]| AutoSignOn {
        user: user.clone(),
        ticket: Ticket::from_bytes(t),
    };
    let tls_port = sys.ports.telnet_tls;
    check(
        "ticket over TLS",
        connect(&host, tls_port, tls.clone(), auto(&first.ticket.0)),
        Shown::SignedOn,
    );
    check(
        "the same ticket again",
        connect(&host, tls_port, tls.clone(), auto(&first.ticket.0)),
        Shown::SignOnDisplay,
    );
    drop(first);
    match c.telnet_ticket() {
        Ok(t) => check(
            "a new ticket over plain Telnet",
            connect(&host, sys.ports.telnet, None, auto(&t.ticket.0)),
            Shown::SignOnDisplay,
        ),
        Err(e) => {
            println!("FAIL a new ticket: {e}");
            ok = false;
        }
    }
    let _ = c.sign_off();
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
