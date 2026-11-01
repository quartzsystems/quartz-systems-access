// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Telnet layer for 5250 sessions (RFC 854, 855, 856, 885, 1091, 1572,
//! 1205).
//!
//! - [`codec`]: bytes to records, option commands and subnegotiations.
//! - [`options`]: option negotiation with loop protection.
//! - [`Telnet`]: the two together, without I/O: bytes from the server go in,
//!   events and the bytes to send come out.
//! - [`transport`]: plain TCP; [`tls`]: TLS over it, with
//!   [`x509`] to show a certificate to the user.

pub mod codec;
pub mod options;
pub mod tls;
pub mod transport;
pub mod x509;

use std::time::Duration;

pub use codec::{DecodeError, Verb};
pub use options::{Change, Side};
pub use transport::{ConnectError, Security, Transport};

use crate::debug;

pub const IAC: u8 = 255;
pub const DONT: u8 = 254;
pub const DO: u8 = 253;
pub const WONT: u8 = 252;
pub const WILL: u8 = 251;
pub const SB: u8 = 250;
pub const GA: u8 = 249;
pub const NOP: u8 = 241;
pub const SE: u8 = 240;
pub const EOR: u8 = 239;

/// RFC 856.
pub const BINARY: u8 = 0;
/// RFC 1091.
pub const TERMINAL_TYPE: u8 = 24;
/// RFC 885. Named so as not to clash with the `EOR` command.
pub const EOR_OPTION: u8 = 25;
/// RFC 1572.
pub const NEW_ENVIRON: u8 = 39;

/// How long a connection may take to open, TLS handshake included.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long the session may be quiet before a Telnet `NOP` goes out. The
/// `NOP` keeps NAT and firewall state alive and lets a dead peer show as a
/// failed write.
pub const KEEPALIVE: Duration = Duration::from_secs(60);

/// Options we perform: binary, end of record, terminal type, environment.
pub const LOCAL_OPTIONS: &[u8] = &[BINARY, EOR_OPTION, TERMINAL_TYPE, NEW_ENVIRON];
/// Options we let the server perform: binary and end of record.
pub const REMOTE_OPTIONS: &[u8] = &[BINARY, EOR_OPTION];

/// A name for an option, for logs.
pub fn option_name(option: u8) -> String {
    match option {
        BINARY => "BINARY".into(),
        TERMINAL_TYPE => "TERMINAL-TYPE".into(),
        EOR_OPTION => "EOR".into(),
        NEW_ENVIRON => "NEW-ENVIRON".into(),
        6 => "TIMING-MARK".into(),
        31 => "NAWS".into(),
        o => format!("option {o}"),
    }
}

/// What the Telnet engine passes up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A whole record.
    Record(Vec<u8>),
    /// A subnegotiation for an option we perform.
    Subneg(u8, Vec<u8>),
    /// A side of an option was enabled or disabled.
    Changed(Change),
    /// A byte-stream error; decoding went on.
    Error(DecodeError),
}

/// The Telnet protocol for one connection, without I/O.
#[derive(Debug)]
pub struct Telnet {
    decoder: codec::Decoder,
    options: options::Options,
    out: Vec<u8>,
}

impl Default for Telnet {
    fn default() -> Self {
        Self::new()
    }
}

impl Telnet {
    /// The engine for a 5250 session: [`LOCAL_OPTIONS`] and
    /// [`REMOTE_OPTIONS`] agreed, everything else refused.
    pub fn new() -> Telnet {
        Telnet {
            decoder: codec::Decoder::new(),
            options: options::Options::new(LOCAL_OPTIONS, REMOTE_OPTIONS),
            out: Vec::new(),
        }
    }

    /// Bytes from the server; the events they complete.
    pub fn receive(&mut self, bytes: &[u8]) -> Vec<Event> {
        let mut tokens = Vec::new();
        self.decoder.feed(bytes, &mut tokens);
        let mut events = Vec::new();
        for t in tokens {
            match t {
                codec::Token::Record(r) => events.push(Event::Record(r)),
                codec::Token::Option(verb, option) => {
                    debug!("telnet: recv {} {}", verb.name(), option_name(option));
                    let outcome = self.options.receive(verb, option);
                    if let Some(note) = outcome.note {
                        debug!("telnet: {} {}: {note}", verb.name(), option_name(option));
                    }
                    self.send_options(&outcome.send);
                    events.extend(outcome.changes.into_iter().map(Event::Changed));
                }
                codec::Token::Subneg(option, data) => {
                    if self.options.enabled(option, Side::Local) {
                        events.push(Event::Subneg(option, data));
                    } else {
                        debug!(
                            "telnet: subnegotiation for {} which is not enabled; ignored",
                            option_name(option)
                        );
                    }
                }
                codec::Token::Command(c) => {
                    if c != NOP {
                        debug!("telnet: recv command {c}; ignored");
                    }
                }
                codec::Token::Error(e) => {
                    crate::warn!("telnet: {e:?}");
                    events.push(Event::Error(e));
                }
            }
        }
        events
    }

    /// Asks to enable or disable a side of an option.
    pub fn request(&mut self, option: u8, side: Side, enable: bool) {
        let outcome = self.options.request(option, side, enable);
        self.send_options(&outcome.send);
    }

    /// Whether a side of an option is on.
    pub fn enabled(&self, option: u8, side: Side) -> bool {
        self.options.enabled(option, side)
    }

    /// Binary and end-of-record on in both directions: 5250 records can
    /// flow.
    pub fn records_ready(&self) -> bool {
        [BINARY, EOR_OPTION]
            .iter()
            .all(|&o| self.enabled(o, Side::Local) && self.enabled(o, Side::Remote))
    }

    /// Queues a record.
    pub fn send_record(&mut self, data: &[u8]) {
        crate::trace!("telnet: send record {}", crate::log::hex(data));
        self.out.extend_from_slice(&codec::encode_record(data));
    }

    /// Queues a subnegotiation.
    pub fn send_subneg(&mut self, option: u8, payload: &[u8]) {
        self.out
            .extend_from_slice(&codec::encode_subneg(option, payload));
    }

    /// Queues a `NOP`, the keepalive.
    pub fn send_nop(&mut self) {
        self.out.extend_from_slice(&[IAC, NOP]);
    }

    /// Bytes waiting to be written; the queue is emptied.
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }

    fn send_options(&mut self, send: &[(Verb, u8)]) {
        for &(verb, option) in send {
            debug!("telnet: send {} {}", verb.name(), option_name(option));
            self.out
                .extend_from_slice(&codec::encode_option(verb, option));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The opening exchange as a TN5250E server sends it: DO NEW-ENVIRON,
    /// DO TERMINAL-TYPE, then DO/WILL EOR and DO/WILL BINARY.
    #[test]
    fn typical_opening_is_agreed_and_records_flow() {
        let mut t = Telnet::new();
        let ev = t.receive(&[IAC, DO, NEW_ENVIRON, IAC, DO, TERMINAL_TYPE]);
        assert_eq!(ev.len(), 2);
        assert_eq!(
            t.take_output(),
            vec![IAC, WILL, NEW_ENVIRON, IAC, WILL, TERMINAL_TYPE]
        );
        t.receive(&[
            IAC, DO, EOR_OPTION, IAC, WILL, EOR_OPTION, IAC, DO, BINARY, IAC, WILL, BINARY,
        ]);
        assert_eq!(
            t.take_output(),
            vec![
                IAC, WILL, EOR_OPTION, IAC, DO, EOR_OPTION, IAC, WILL, BINARY, IAC, DO, BINARY
            ]
        );
        assert!(t.records_ready());
        let ev = t.receive(&[0x00, 0x0A, 0x12, 0xA0, 0xFF, 0xFF, IAC, EOR]);
        assert_eq!(ev, vec![Event::Record(vec![0x00, 0x0A, 0x12, 0xA0, 0xFF])]);
    }

    #[test]
    fn subnegotiation_passes_only_for_enabled_options() {
        let mut t = Telnet::new();
        let sb = [IAC, SB, TERMINAL_TYPE, 1, IAC, SE];
        assert!(t.receive(&sb).is_empty());
        t.receive(&[IAC, DO, TERMINAL_TYPE]);
        assert_eq!(t.receive(&sb), vec![Event::Subneg(TERMINAL_TYPE, vec![1])]);
    }

    #[test]
    fn records_and_keepalive_are_framed() {
        let mut t = Telnet::new();
        t.send_record(&[0x01, 0xFF]);
        t.send_nop();
        assert_eq!(t.take_output(), vec![0x01, 0xFF, 0xFF, IAC, EOR, IAC, NOP]);
        assert!(t.take_output().is_empty());
    }

    #[test]
    fn refused_options_and_ignored_commands() {
        let mut t = Telnet::new();
        let ev = t.receive(&[IAC, DO, 31, IAC, WILL, 1, IAC, NOP, IAC, GA]);
        assert!(ev.is_empty());
        assert_eq!(t.take_output(), vec![IAC, WONT, 31, IAC, DONT, 1]);
    }
}
