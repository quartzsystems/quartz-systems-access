// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Option negotiation by the "Q method" of RFC 1143, for both sides of
//! each option: ours (`WILL`/`WONT` from us, answered by `DO`/`DONT`) and
//! the server's (`WILL`/`WONT` from it, answered by our `DO`/`DONT`).
//!
//! The Q method never answers an answer, so two sides cannot loop. On top
//! of it a budget per option stops a server that keeps changing its mind:
//! past [`MAX_CHANGES`] the option is left as it is and further requests
//! for it are ignored.

use super::codec::Verb;

/// Times an option may change state before negotiation of it is frozen.
pub const MAX_CHANGES: u16 = 32;

/// Which side of an option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// We perform the option (we sent `WILL`, the server `DO`).
    Local,
    /// The server performs it (it sent `WILL`, we `DO`).
    Remote,
}

/// RFC 1143's states for one side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Q {
    #[default]
    No,
    Yes,
    /// We asked to disable; `true` if we want it enabled again afterwards.
    WantNo(bool),
    /// We asked to enable; `true` if we want it disabled again afterwards.
    WantYes(bool),
}

#[derive(Clone, Copy, Debug, Default)]
struct Entry {
    local: Q,
    remote: Q,
    changes: u16,
    frozen: bool,
}

/// A change of state the session layer acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub option: u8,
    pub side: Side,
    pub enabled: bool,
}

/// What handling one received command produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Commands to send back.
    pub send: Vec<(Verb, u8)>,
    /// Sides that were enabled or disabled.
    pub changes: Vec<Change>,
    /// Set when the command broke the protocol (an answer to nothing we
    /// asked, or past the budget); for the log.
    pub note: Option<&'static str>,
}

/// The negotiation state of every option.
#[derive(Clone, Debug)]
pub struct Options {
    table: Box<[Entry; 256]>,
    local_ok: [bool; 256],
    remote_ok: [bool; 256],
}

impl Options {
    /// Options we agree to perform (`local`) and to let the server perform
    /// (`remote`). Every other request is refused.
    pub fn new(local: &[u8], remote: &[u8]) -> Options {
        let mut local_ok = [false; 256];
        let mut remote_ok = [false; 256];
        for &o in local {
            local_ok[o as usize] = true;
        }
        for &o in remote {
            remote_ok[o as usize] = true;
        }
        Options {
            table: Box::new([Entry::default(); 256]),
            local_ok,
            remote_ok,
        }
    }

    /// Whether a side of an option is enabled.
    pub fn enabled(&self, option: u8, side: Side) -> bool {
        let e = &self.table[option as usize];
        match side {
            Side::Local => e.local == Q::Yes,
            Side::Remote => e.remote == Q::Yes,
        }
    }

    /// Asks to enable a side: `WILL` for ours, `DO` for the server's.
    pub fn request(&mut self, option: u8, side: Side, enable: bool) -> Outcome {
        let mut out = Outcome::default();
        let e = &mut self.table[option as usize];
        let q = match side {
            Side::Local => &mut e.local,
            Side::Remote => &mut e.remote,
        };
        let (ask_yes, ask_no) = match side {
            Side::Local => (Verb::Will, Verb::Wont),
            Side::Remote => (Verb::Do, Verb::Dont),
        };
        match (enable, *q) {
            (true, Q::No) => {
                *q = Q::WantYes(false);
                out.send.push((ask_yes, option));
            }
            (true, Q::WantNo(_)) => *q = Q::WantNo(true),
            (true, Q::WantYes(_)) => *q = Q::WantYes(false),
            (false, Q::Yes) => {
                *q = Q::WantNo(false);
                out.send.push((ask_no, option));
            }
            (false, Q::WantNo(_)) => *q = Q::WantNo(false),
            (false, Q::WantYes(_)) => *q = Q::WantYes(true),
            (true, Q::Yes) | (false, Q::No) => {}
        }
        out
    }

    /// Handles `IAC verb option` from the server.
    pub fn receive(&mut self, verb: Verb, option: u8) -> Outcome {
        let mut out = Outcome::default();
        let i = option as usize;
        if self.table[i].frozen {
            out.note = Some("negotiation of this option is frozen; ignored");
            return out;
        }
        let (side, yes) = match verb {
            Verb::Will => (Side::Remote, true),
            Verb::Wont => (Side::Remote, false),
            Verb::Do => (Side::Local, true),
            Verb::Dont => (Side::Local, false),
        };
        let allowed = match side {
            Side::Local => self.local_ok[i],
            Side::Remote => self.remote_ok[i],
        };
        // Our answers: agree, refuse.
        let (agree, refuse) = match side {
            Side::Local => (Verb::Will, Verb::Wont),
            Side::Remote => (Verb::Do, Verb::Dont),
        };
        let e = &mut self.table[i];
        let q = match side {
            Side::Local => &mut e.local,
            Side::Remote => &mut e.remote,
        };
        let before = *q == Q::Yes;
        if yes {
            match *q {
                Q::No => {
                    if allowed {
                        *q = Q::Yes;
                        out.send.push((agree, option));
                    } else {
                        out.send.push((refuse, option));
                    }
                }
                Q::Yes => {}
                Q::WantNo(false) => {
                    *q = Q::No;
                    out.note = Some("enabled in answer to our disable");
                }
                Q::WantNo(true) => *q = Q::Yes,
                Q::WantYes(false) => *q = Q::Yes,
                Q::WantYes(true) => {
                    *q = Q::WantNo(false);
                    out.send.push((refuse, option));
                }
            }
        } else {
            match *q {
                Q::No => {}
                Q::Yes => {
                    *q = Q::No;
                    out.send.push((refuse, option));
                }
                Q::WantNo(false) => *q = Q::No,
                Q::WantNo(true) => {
                    *q = Q::WantYes(false);
                    out.send.push((agree, option));
                }
                Q::WantYes(_) => *q = Q::No,
            }
        }
        let after = *q == Q::Yes;
        if before != after {
            out.changes.push(Change {
                option,
                side,
                enabled: after,
            });
            e.changes += 1;
            if e.changes >= MAX_CHANGES {
                e.frozen = true;
                out.note = Some("option changed too often; negotiation of it frozen");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telnet::{BINARY, EOR_OPTION, NEW_ENVIRON, TERMINAL_TYPE};

    const TIMING_MARK: u8 = 6;

    fn options() -> Options {
        let both = [BINARY, EOR_OPTION];
        let local = [BINARY, EOR_OPTION, TERMINAL_TYPE, NEW_ENVIRON];
        Options::new(&local, &both)
    }

    #[test]
    fn server_do_is_agreed_once() {
        let mut o = options();
        let r = o.receive(Verb::Do, TERMINAL_TYPE);
        assert_eq!(r.send, vec![(Verb::Will, TERMINAL_TYPE)]);
        assert_eq!(
            r.changes,
            vec![Change {
                option: TERMINAL_TYPE,
                side: Side::Local,
                enabled: true
            }]
        );
        assert!(o.enabled(TERMINAL_TYPE, Side::Local));
        // A repeated DO is not answered: no loop.
        assert_eq!(o.receive(Verb::Do, TERMINAL_TYPE), Outcome::default());
    }

    #[test]
    fn unknown_options_are_refused_both_ways() {
        let mut o = options();
        assert_eq!(
            o.receive(Verb::Do, TIMING_MARK).send,
            vec![(Verb::Wont, TIMING_MARK)]
        );
        assert_eq!(
            o.receive(Verb::Will, TIMING_MARK).send,
            vec![(Verb::Dont, TIMING_MARK)]
        );
        // The server's answering WONT/DONT to our refusal needs nothing.
        assert_eq!(o.receive(Verb::Wont, TIMING_MARK), Outcome::default());
        assert!(!o.enabled(TIMING_MARK, Side::Remote));
    }

    #[test]
    fn server_will_binary_and_eor_get_do() {
        let mut o = options();
        assert_eq!(o.receive(Verb::Will, BINARY).send, vec![(Verb::Do, BINARY)]);
        assert_eq!(
            o.receive(Verb::Will, EOR_OPTION).send,
            vec![(Verb::Do, EOR_OPTION)]
        );
        assert!(o.enabled(BINARY, Side::Remote));
        assert!(o.enabled(EOR_OPTION, Side::Remote));
        // NEW-ENVIRON is ours to perform, not the server's.
        assert_eq!(
            o.receive(Verb::Will, NEW_ENVIRON).send,
            vec![(Verb::Dont, NEW_ENVIRON)]
        );
    }

    #[test]
    fn our_request_is_not_answered_twice() {
        let mut o = options();
        let r = o.request(BINARY, Side::Local, true);
        assert_eq!(r.send, vec![(Verb::Will, BINARY)]);
        // The server's DO answers our WILL: nothing goes back.
        let r = o.receive(Verb::Do, BINARY);
        assert!(r.send.is_empty());
        assert!(o.enabled(BINARY, Side::Local));
        // Our request while it is already on sends nothing.
        assert!(o.request(BINARY, Side::Local, true).send.is_empty());
    }

    #[test]
    fn refusal_of_our_request() {
        let mut o = options();
        o.request(EOR_OPTION, Side::Remote, true);
        let r = o.receive(Verb::Wont, EOR_OPTION);
        assert!(r.send.is_empty());
        assert!(!o.enabled(EOR_OPTION, Side::Remote));
    }

    #[test]
    fn server_disabling_is_acknowledged() {
        let mut o = options();
        o.receive(Verb::Do, NEW_ENVIRON);
        let r = o.receive(Verb::Dont, NEW_ENVIRON);
        assert_eq!(r.send, vec![(Verb::Wont, NEW_ENVIRON)]);
        assert_eq!(
            r.changes,
            vec![Change {
                option: NEW_ENVIRON,
                side: Side::Local,
                enabled: false
            }]
        );
        assert!(o.receive(Verb::Dont, NEW_ENVIRON).send.is_empty());
    }

    #[test]
    fn queued_opposite_request() {
        let mut o = options();
        o.request(BINARY, Side::Remote, true);
        // Changed our mind before the answer came.
        o.request(BINARY, Side::Remote, false);
        let r = o.receive(Verb::Will, BINARY);
        assert_eq!(r.send, vec![(Verb::Dont, BINARY)]);
        assert!(o.receive(Verb::Wont, BINARY).send.is_empty());
        assert!(!o.enabled(BINARY, Side::Remote));
    }

    #[test]
    fn a_flapping_server_is_frozen() {
        let mut o = options();
        let mut sent = 0;
        for _ in 0..1000 {
            sent += o.receive(Verb::Will, BINARY).send.len();
            sent += o.receive(Verb::Wont, BINARY).send.len();
        }
        assert_eq!(sent, MAX_CHANGES as usize);
        let r = o.receive(Verb::Will, BINARY);
        assert!(r.send.is_empty());
        assert!(r.note.is_some());
    }
}
