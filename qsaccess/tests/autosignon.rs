// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! 5250 automatic sign-on by QAP ticket, end to end in this process: a
//! ticket from QS/400's test system (`TelnetTicket` over qap-client), put
//! by tn5250's negotiator where RFC 4777 puts the password, and taken by
//! the test system as its Telnet server takes one
//! (`System::redeem_ticket`): accepted once over TLS, refused when used
//! again, over plain Telnet, after a minute or for another profile. The
//! ticket never appears in the trace.

#[path = "../../qap-client/tests/common/mod.rs"]
mod common;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use common::{cfg, start};
use qap_client::{Client, Secret, Status};
use tn5250::negotiate::{
    AutoSignOn, IS, Negotiator, Request, SEND, ScreenSize, Ticket, USERVAR, VAR, parse_is,
};
use tn5250::telnet::{DO, IAC, NEW_ENVIRON, SB, SE, TERMINAL_TYPE};
use tn5250::wire;

/// What the session sends when the server asks for its environment with an
/// RFC 4777 seed: the user, the seed variable and the password variable.
fn sent(user: &str, ticket: &[u8; 32]) -> (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>) {
    let mut r = Request::display(ScreenSize::S24x80, None, 37);
    r.auto_sign_on = Some(AutoSignOn {
        user: user.into(),
        ticket: Ticket::from_bytes(ticket),
    });
    let mut n = Negotiator::new(r);
    n.receive(&[IAC, DO, NEW_ENVIRON, IAC, DO, TERMINAL_TYPE]);
    n.take_output();
    let mut body = vec![IAC, SB, NEW_ENVIRON, SEND, USERVAR];
    body.extend_from_slice(wire::RANDOM_SEED.as_bytes());
    body.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, VAR, USERVAR, IAC, SE]);
    n.receive(&body);
    let out = n.take_output();
    let at = out
        .windows(4)
        .position(|w| w == [IAC, SB, NEW_ENVIRON, IS])
        .expect("an IS")
        + 4;
    let end = at
        + out[at..]
            .windows(2)
            .position(|w| w == [IAC, SE])
            .expect("SE");
    let vars = parse_is(&out[at..end]);
    let get = |kind: u8, name: &str| {
        vars.iter()
            .find(|(k, n, _)| *k == kind && n == name.as_bytes())
            .map(|(_, _, v)| v.clone())
    };
    (
        get(VAR, "USER").unwrap_or_default(),
        get(USERVAR, wire::RANDOM_SEED),
        get(USERVAR, wire::PASSWORD_SUBSTITUTE),
    )
}

fn signed_on(s: &common::Server, user: &str, password: &str) -> Client {
    let mut c = Client::new(s.target());
    c.sign_on(user, &Secret::from(password)).unwrap();
    c
}

/// Every trace line from here on, at every level.
fn capture() -> Arc<Mutex<Vec<String>>> {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let l = Arc::clone(&lines);
    qsa_net::log::set_sink(qsa_net::log::Level::Trace, move |_, text| {
        l.lock().unwrap().push(text.to_owned());
    });
    lines
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|v| format!("{v:02X}")).collect()
}

#[test]
fn a_ticket_signs_on_once_over_tls_and_never_shows() {
    let log = capture();
    let s = start(cfg());
    let mut c = signed_on(&s, "BOB", "SECRET1");
    let t = c.telnet_ticket().unwrap();
    assert_eq!(t.expires_secs, 60);
    let bytes = t.ticket.0;
    let (user, seed, pw) = sent("bob", &bytes);
    assert_eq!(user, b"BOB");
    assert_eq!(
        seed,
        Some(Vec::new()),
        "the seed variable, present and empty"
    );
    let pw = pw.expect("the password variable");
    assert_eq!(pw, hex(&bytes).into_bytes());
    assert!(s.c.redeem_ticket("BOB", &pw, true), "accepted over TLS");
    assert!(!s.c.redeem_ticket("BOB", &pw, true), "used up");
    assert!(
        s.c.audit.lock().unwrap().iter().any(|a| a == "PS P BOB"),
        "the issue is audited"
    );
    // Neither the ticket nor its hexadecimal form is in any trace line.
    let lines = log.lock().unwrap().clone();
    assert!(!lines.is_empty(), "the negotiation was traced");
    let h = hex(&bytes);
    for l in &lines {
        assert!(!l.contains(&h) && !l.contains(&h.to_lowercase()), "{l}");
    }
    assert!(!format!("{t:?}").contains(&h));
}

#[test]
fn a_ticket_is_refused_over_plain_telnet_late_or_for_another_profile() {
    let s = start(cfg());
    let mut c = signed_on(&s, "BOB", "SECRET1");
    // Plain Telnet: refused, and used up all the same.
    let (_, _, pw) = sent("BOB", &c.telnet_ticket().unwrap().ticket.0);
    let pw = pw.unwrap();
    assert!(!s.c.redeem_ticket("BOB", &pw, false));
    assert!(!s.c.redeem_ticket("BOB", &pw, true));
    // A minute on: expired.
    let (_, _, pw) = sent("BOB", &c.telnet_ticket().unwrap().ticket.0);
    s.c.advance(61);
    assert!(!s.c.redeem_ticket("BOB", &pw.unwrap(), true));
    // Another profile's name with BOB's ticket.
    let (_, _, pw) = sent("WARN", &c.telnet_ticket().unwrap().ticket.0);
    assert!(!s.c.redeem_ticket("WARN", &pw.unwrap(), true));
    // A password where the ticket goes is never taken.
    assert!(!s.c.redeem_ticket("BOB", b"SECRET1", true));
}

#[test]
fn no_ticket_when_automatic_sign_on_is_not_allowed() {
    let s = start(cfg());
    s.c.auto_signon.store(false, Ordering::SeqCst);
    let mut c = signed_on(&s, "BOB", "SECRET1");
    let e = c.telnet_ticket().unwrap_err();
    assert_eq!(e.status(), Some(Status::NotAuthorized));
    assert_eq!(e.server_message().unwrap().id, "TCP1A55");
}
