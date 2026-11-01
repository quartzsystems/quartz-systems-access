// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! TN5250E negotiation (RFC 2877, RFC 4777) on top of [`crate::telnet`],
//! without I/O.
//!
//! The client answers the server's `DO TERMINAL-TYPE` and `DO NEW-ENVIRON`.
//! To the terminal type `SEND` it answers with the device's terminal type;
//! to the environment `SEND` with `DEVNAME` (when a device name was given),
//! `KBDTYPE`, `CODEPAGE` and `CHARSET`. It never sends `USER`, the random
//! seed or a password substitute: the user signs on at the system's own
//! sign-on display.
//!
//! Vendor-prefixed wire strings come from [`crate::wire`].
//!
//! A server that never enables NEW-ENVIRON gets plain TN5250
//! ([`Mode::Basic`]). With a device name the server answers with a start-up
//! response record whose code says whether the device could be used.
//!
//! Every step is logged at debug level.

use crate::codepage::{self, CodePage};
use crate::debug;
use crate::record::{Record, opcode};
use crate::telnet::{self, Event as TelnetEvent, NEW_ENVIRON, Side, TERMINAL_TYPE, Telnet};
use crate::wire;

/// RFC 1091 and RFC 1572 subnegotiation commands.
pub const IS: u8 = 0;
pub const SEND: u8 = 1;
pub const INFO: u8 = 2;
/// RFC 1572 item types.
pub const VAR: u8 = 0;
pub const VALUE: u8 = 1;
pub const ESC: u8 = 2;
pub const USERVAR: u8 = 3;

/// A display's screen size and the terminal type that asks for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScreenSize {
    /// 24 by 80, terminal type [`wire::TT_DISPLAY_24X80`].
    #[default]
    S24x80,
    /// 27 by 132, terminal type [`wire::TT_DISPLAY_27X132`].
    S27x132,
}

impl ScreenSize {
    pub fn rows(self) -> usize {
        match self {
            ScreenSize::S24x80 => 24,
            ScreenSize::S27x132 => 27,
        }
    }

    pub fn cols(self) -> usize {
        match self {
            ScreenSize::S24x80 => 80,
            ScreenSize::S27x132 => 132,
        }
    }

    pub fn terminal_type(self) -> &'static str {
        match self {
            ScreenSize::S24x80 => wire::TT_DISPLAY_24X80,
            ScreenSize::S27x132 => wire::TT_DISPLAY_27X132,
        }
    }
}

/// What the client asks the server for. A printer session later uses the
/// same request with a printer terminal type and its own variables in
/// `extra`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub terminal_type: String,
    /// `DEVNAME`, upper case; `None` lets the server choose.
    pub device_name: Option<String>,
    pub ccsid: u16,
    /// More `USERVAR`s, sent after the standard ones.
    pub extra: Vec<(String, String)>,
}

impl Request {
    /// A display session.
    pub fn display(size: ScreenSize, device_name: Option<&str>, ccsid: u16) -> Request {
        Request {
            terminal_type: size.terminal_type().to_owned(),
            device_name: device_name
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_ascii_uppercase),
            ccsid,
            extra: Vec::new(),
        }
    }

    /// The `USERVAR`s this request sends, in order.
    pub fn user_vars(&self) -> Vec<(String, String)> {
        let env = codepage::environ(self.ccsid);
        let mut v = Vec::new();
        if let Some(d) = &self.device_name {
            v.push(("DEVNAME".to_owned(), d.clone()));
        }
        v.push(("KBDTYPE".to_owned(), env.kbdtype.to_owned()));
        v.push(("CODEPAGE".to_owned(), env.codepage.to_string()));
        v.push(("CHARSET".to_owned(), env.charset.to_string()));
        v.extend(self.extra.iter().cloned());
        v
    }
}

/// Whether the server took up TN5250E.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// NEW-ENVIRON was negotiated.
    Enhanced,
    /// Plain TN5250 (RFC 1205): no device name, no start-up response.
    Basic,
}

/// What a start-up response code means for the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The session started (`I901`, `I902`, `I906`).
    Started,
    /// The device is in use or otherwise not available.
    InUse,
    /// The device name cannot be used for this session.
    NotValid,
    /// Any other failure.
    Failed,
}

/// A start-up response record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupResponse {
    /// Four characters, `I902` or `8902` and so on.
    pub code: String,
    /// The device the server gave, when it said.
    pub device: Option<String>,
}

impl StartupResponse {
    pub fn outcome(&self) -> Outcome {
        match self.code.as_str() {
            "I901" | "I902" | "I906" => Outcome::Started,
            "8901" | "8902" | "8930" => Outcome::InUse,
            "2702" | "2777" | "8903" | "8910" | "8916" | "8917" | "8920" => Outcome::NotValid,
            _ => Outcome::Failed,
        }
    }

    /// The meaning of the code, for messages and logs.
    pub fn meaning(&self) -> &'static str {
        response_text(&self.code)
    }
}

/// RFC 2877's start-up response codes.
pub fn response_text(code: &str) -> &'static str {
    match code {
        "I901" => "virtual device has less function than source device",
        "I902" => "session successfully started",
        "I904" => "source system at incompatible release",
        "I906" => "automatic sign-on requested but not allowed; sign-on display follows",
        "2702" => "device description not found",
        "2703" => "controller description not found",
        "2777" => "damaged device description",
        "8901" => "device not varied on",
        "8902" => "device not available",
        "8903" => "device not valid for session",
        "8906" => "session initiation failed",
        "8907" => "session failure",
        "8910" => "controller not valid for session",
        "8916" => "no matching device found",
        "8917" => "not authorized to object",
        "8918" => "job cancelled",
        "8920" => "object partially damaged",
        "8921" => "communications error",
        "8922" => "negative response received",
        "8923" => "start-up record built incorrectly",
        "8925" => "creation of device failed",
        "8928" => "change of device failed",
        "8929" => "vary on or vary off failed",
        "8930" => "message queue does not exist",
        "8934" => "start-up for S/36 work station function received",
        "8935" => "session rejected",
        "8936" => "security failure on session attempt",
        "8937" => "automatic sign-on rejected",
        "8940" => "automatic configuration failed or not allowed",
        _ => "unknown response code",
    }
}

/// Whether a start-up response record has RFC 4777's layout (the reserved
/// field `9000` and a variable header), rather than the older one QS/400
/// still sends: opcode 0, the code, and the device name padded to ten.
pub fn is_rfc_startup_layout(rec: &Record) -> bool {
    rec.reserved == 0x9000
}

/// Reads a start-up response from a record, if it is one: opcode 0 (or the
/// RFC's reserved field `9000`), then four EBCDIC characters, a letter
/// `I` or a digit followed by three digits. The device name follows: as
/// exactly ten characters (QS/400 sends that alone), else as the last
/// name-like run of characters after the code. Both layouts are read; the
/// older one is logged as a warning, since QS/400 is moving to RFC 4777's.
pub fn startup_response(rec: &Record, page: &dyn CodePage) -> Option<StartupResponse> {
    if rec.opcode != opcode::NO_OP && !is_rfc_startup_layout(rec) {
        return None;
    }
    let text = page.decode(&rec.data);
    let mut chars = text.chars();
    let code: String = chars.by_ref().take(4).collect();
    let c: Vec<char> = code.chars().collect();
    let shaped = c.len() == 4
        && (c[0] == 'I' || c[0].is_ascii_digit())
        && c[1..].iter().all(char::is_ascii_digit);
    if !shaped {
        return None;
    }
    if !is_rfc_startup_layout(rec) {
        crate::warn!(
            "tn5250: start-up response {code} is not in the RFC 4777 layout (no 9000 reserved field); read the older way"
        );
    }
    let rest: String = chars.collect();
    let device = if rest.chars().count() == 10 {
        Some(rest.trim().to_owned()).filter(|d| !d.is_empty())
    } else {
        rest.split(|ch: char| !(ch.is_ascii_alphanumeric() || "$#@_".contains(ch)))
            .rfind(|t| !t.is_empty() && t.len() <= 10)
            .map(str::to_owned)
    };
    Some(StartupResponse { code, device })
}

/// What the negotiator passes up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Records can flow; the mode is decided.
    Ready(Mode),
    /// The start-up response.
    Startup(StartupResponse),
    /// A 5250 record.
    Record(Record),
    /// Something in the stream was wrong; the session goes on.
    ProtocolError(String),
}

/// The client's side of the negotiation, then the record pipe.
#[derive(Debug)]
pub struct Negotiator {
    telnet: Telnet,
    request: Request,
    page: &'static dyn CodePage,
    environ_sent: bool,
    ready: Option<Mode>,
    records_seen: bool,
}

impl Negotiator {
    pub fn new(request: Request) -> Negotiator {
        debug!(
            "tn5250e: request terminal type {}, device {}, CCSID {}",
            request.terminal_type,
            request.device_name.as_deref().unwrap_or("(server chooses)"),
            request.ccsid
        );
        let page = codepage::by_ccsid(request.ccsid).unwrap_or_else(codepage::default_page);
        Negotiator {
            telnet: Telnet::new(),
            request,
            page,
            environ_sent: false,
            ready: None,
            records_seen: false,
        }
    }

    /// The mode, once records can flow.
    pub fn mode(&self) -> Option<Mode> {
        self.ready
    }

    /// Bytes from the server.
    pub fn receive(&mut self, bytes: &[u8]) -> Vec<Event> {
        let mut out = Vec::new();
        for ev in self.telnet.receive(bytes) {
            match ev {
                TelnetEvent::Record(bytes) => self.record(&bytes, &mut out),
                TelnetEvent::Subneg(TERMINAL_TYPE, data) => self.terminal_type(&data),
                TelnetEvent::Subneg(NEW_ENVIRON, data) => self.environ(&data),
                TelnetEvent::Subneg(o, _) => {
                    debug!(
                        "tn5250e: subnegotiation for {} ignored",
                        telnet::option_name(o)
                    );
                }
                TelnetEvent::Changed(c) => {
                    debug!(
                        "tn5250e: {} {} {}",
                        if c.side == Side::Local {
                            "our"
                        } else {
                            "server's"
                        },
                        telnet::option_name(c.option),
                        if c.enabled { "enabled" } else { "disabled" }
                    );
                }
                TelnetEvent::Error(e) => out.push(Event::ProtocolError(format!("Telnet: {e:?}"))),
            }
        }
        if self.ready.is_none() && self.telnet.records_ready() {
            let mode = if self.telnet.enabled(NEW_ENVIRON, Side::Local) && self.environ_sent {
                Mode::Enhanced
            } else {
                Mode::Basic
            };
            debug!("tn5250e: binary and end-of-record both ways; mode {mode:?}");
            self.ready = Some(mode);
            out.insert(0, Event::Ready(mode));
        }
        out
    }

    /// Queues a record to the server.
    pub fn send(&mut self, rec: &Record) {
        self.telnet.send_record(&rec.encode());
    }

    /// Queues a keepalive.
    pub fn keepalive(&mut self) {
        self.telnet.send_nop();
    }

    /// Bytes to write.
    pub fn take_output(&mut self) -> Vec<u8> {
        self.telnet.take_output()
    }

    fn record(&mut self, bytes: &[u8], out: &mut Vec<Event>) {
        crate::trace!("tn5250: recv record {}", crate::log::hex(bytes));
        let rec = match Record::parse(bytes) {
            Ok(r) => r,
            Err(e) => {
                crate::warn!("tn5250: {e}: {}", crate::log::hex(bytes));
                out.push(Event::ProtocolError(e.to_string()));
                return;
            }
        };
        if !self.records_seen {
            self.records_seen = true;
            if let Some(s) = startup_response(&rec, self.page) {
                debug!(
                    "tn5250e: start-up response {} ({}), device {}",
                    s.code,
                    s.meaning(),
                    s.device.as_deref().unwrap_or("(not given)")
                );
                debug!(
                    "tn5250e: start-up response bytes {}",
                    crate::log::hex(bytes)
                );
                out.push(Event::Startup(s));
                return;
            }
        }
        out.push(Event::Record(rec));
    }

    fn terminal_type(&mut self, data: &[u8]) {
        match data.first() {
            Some(&SEND) => {
                debug!("tn5250e: recv SB TERMINAL-TYPE SEND");
                let mut payload = vec![IS];
                payload.extend_from_slice(self.request.terminal_type.as_bytes());
                self.telnet.send_subneg(TERMINAL_TYPE, &payload);
                debug!(
                    "tn5250e: send SB TERMINAL-TYPE IS {}",
                    self.request.terminal_type
                );
            }
            _ => debug!(
                "tn5250e: recv SB TERMINAL-TYPE {}; ignored",
                crate::log::hex(data)
            ),
        }
    }

    fn environ(&mut self, data: &[u8]) {
        let Some((&cmd, body)) = data.split_first() else {
            debug!("tn5250e: empty NEW-ENVIRON subnegotiation ignored");
            return;
        };
        if cmd != SEND {
            debug!(
                "tn5250e: recv SB NEW-ENVIRON {} {}; ignored",
                if cmd == IS {
                    "IS"
                } else if cmd == INFO {
                    "INFO"
                } else {
                    "?"
                },
                crate::log::hex(body)
            );
            return;
        }
        let asked = parse_send(body);
        debug!(
            "tn5250e: recv SB NEW-ENVIRON SEND {}",
            describe_send(&asked)
        );
        let ours = self.request.user_vars();
        let mut answer: Vec<(String, String)> = Vec::new();
        let mut want = |name: &str| {
            if let Some(v) = ours.iter().find(|(n, _)| n == name)
                && !answer.iter().any(|(n, _)| n == name)
            {
                answer.push(v.clone());
            }
        };
        if asked.is_empty() {
            ours.iter().for_each(|(n, _)| want(n));
        }
        for item in &asked {
            match item {
                (USERVAR, name) if name.is_empty() => ours.iter().for_each(|(n, _)| want(n)),
                (USERVAR, name) => want(&String::from_utf8_lossy(name)),
                // VARs (USER and the like) are never sent.
                _ => {}
            }
        }
        let payload = encode_is(&answer);
        self.telnet.send_subneg(NEW_ENVIRON, &payload);
        self.environ_sent = true;
        let shown: Vec<String> = answer
            .iter()
            .map(|(n, v)| format!("USERVAR {n}={v}"))
            .collect();
        debug!("tn5250e: send SB NEW-ENVIRON IS {}", shown.join(" "));
    }
}

/// The items of a `SEND`: type and name. RFC 4777's random seed
/// ([`wire::RANDOM_SEED`]) carries its eight seed bytes in the name, which
/// is fine: the name is never used for it.
pub fn parse_send(body: &[u8]) -> Vec<(u8, Vec<u8>)> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let kind = body[i];
        i += 1;
        if kind != VAR && kind != USERVAR {
            continue;
        }
        let mut name = Vec::new();
        while i < body.len() && body[i] != VAR && body[i] != USERVAR {
            if body[i] == ESC && i + 1 < body.len() {
                i += 1;
            }
            name.push(body[i]);
            i += 1;
        }
        items.push((kind, name));
    }
    items
}

fn describe_send(items: &[(u8, Vec<u8>)]) -> String {
    if items.is_empty() {
        return "(everything)".to_owned();
    }
    items
        .iter()
        .map(|(k, n)| {
            let kind = if *k == VAR { "VAR" } else { "USERVAR" };
            let seed = wire::RANDOM_SEED.as_bytes();
            if n.starts_with(seed) {
                format!(
                    "{kind} {}(+{} seed bytes)",
                    wire::RANDOM_SEED,
                    n.len() - seed.len()
                )
            } else if n.is_empty() {
                kind.to_owned()
            } else {
                format!("{kind} {}", String::from_utf8_lossy(n))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_escaped(out: &mut Vec<u8>, s: &[u8]) {
    for &b in s {
        if b <= USERVAR {
            out.push(ESC);
        }
        out.push(b);
    }
}

/// `IS USERVAR name VALUE value ...`.
pub fn encode_is(vars: &[(String, String)]) -> Vec<u8> {
    let mut out = vec![IS];
    for (n, v) in vars {
        out.push(USERVAR);
        push_escaped(&mut out, n.as_bytes());
        out.push(VALUE);
        push_escaped(&mut out, v.as_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telnet::{BINARY, DO, EOR_OPTION, IAC, SB, SE, WILL, WONT};

    fn sb(option: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![IAC, SB, option];
        v.extend_from_slice(body);
        v.extend_from_slice(&[IAC, SE]);
        v
    }

    fn records_on() -> Vec<u8> {
        vec![
            IAC, DO, EOR_OPTION, IAC, WILL, EOR_OPTION, IAC, DO, BINARY, IAC, WILL, BINARY,
        ]
    }

    /// The server's environment SEND with an RFC 4777 seed.
    fn seeded_send() -> Vec<u8> {
        let mut body = vec![SEND, USERVAR];
        body.extend_from_slice(wire::RANDOM_SEED.as_bytes());
        body.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
        body.extend_from_slice(&[VAR, USERVAR]);
        sb(NEW_ENVIRON, &body)
    }

    fn opened(request: Request) -> (Negotiator, Vec<u8>) {
        let mut n = Negotiator::new(request);
        n.receive(&[IAC, DO, NEW_ENVIRON, IAC, DO, TERMINAL_TYPE]);
        n.take_output();
        n.receive(&seeded_send());
        let env = n.take_output();
        (n, env)
    }

    #[test]
    fn environment_answer_has_the_device_and_code_page_and_nothing_secret() {
        let (_, env) = opened(Request::display(ScreenSize::S24x80, Some("qsadsp01"), 37));
        let mut want = vec![IAC, SB, NEW_ENVIRON, IS];
        for (n, v) in [
            ("DEVNAME", "QSADSP01"),
            ("KBDTYPE", "USB"),
            ("CODEPAGE", "37"),
            ("CHARSET", "697"),
        ] {
            want.push(USERVAR);
            want.extend_from_slice(n.as_bytes());
            want.push(VALUE);
            want.extend_from_slice(v.as_bytes());
        }
        want.extend_from_slice(&[IAC, SE]);
        assert_eq!(env, want);
        let text = String::from_utf8_lossy(&env);
        assert!(
            !text.contains("USER\u{1}")
                && !text.contains(wire::RANDOM_SEED)
                && !text.contains(wire::PASSWORD_SUBSTITUTE)
        );
    }

    #[test]
    fn no_device_name_is_sent_when_none_given() {
        let (_, env) = opened(Request::display(ScreenSize::S27x132, Some("  "), 1140));
        let text = String::from_utf8_lossy(&env);
        assert!(!text.contains("DEVNAME"));
        assert!(text.contains("CODEPAGE\u{1}1140"));
        assert!(text.contains("CHARSET\u{1}695"));
    }

    #[test]
    fn terminal_type_answer() {
        let mut n = Negotiator::new(Request::display(ScreenSize::S27x132, None, 37));
        n.receive(&[IAC, DO, TERMINAL_TYPE]);
        n.take_output();
        n.receive(&sb(TERMINAL_TYPE, &[SEND]));
        let mut want = vec![IAC, SB, TERMINAL_TYPE, IS];
        want.extend_from_slice(wire::TT_DISPLAY_27X132.as_bytes());
        want.extend_from_slice(&[IAC, SE]);
        assert_eq!(n.take_output(), want);
    }

    #[test]
    fn enhanced_mode_with_startup_response() {
        let (mut n, _) = opened(Request::display(ScreenSize::S24x80, Some("QSADSP01"), 37));
        let ev = n.receive(&records_on());
        assert_eq!(ev, vec![Event::Ready(Mode::Enhanced)]);
        // QS/400's shape: opcode 0, code, device padded to ten.
        let page = codepage::default_page();
        let mut data = page.encode("I902");
        data.extend(page.encode("QSADSP01  "));
        let rec = Record::new(0, data).encode();
        let ev = n.receive(&telnet::codec::encode_record(&rec));
        assert_eq!(
            ev,
            vec![Event::Startup(StartupResponse {
                code: "I902".into(),
                device: Some("QSADSP01".into())
            })]
        );
        // Later records pass through.
        let rec = Record::new(opcode::PUT_GET, vec![0x04, 0x40]).encode();
        let ev = n.receive(&telnet::codec::encode_record(&rec));
        assert!(matches!(&ev[..], [Event::Record(r)] if r.opcode == opcode::PUT_GET));
    }

    #[test]
    fn rfc_shaped_startup_response() {
        // Reserved 9000, a five-byte variable header.
        let page = codepage::default_page();
        let mut b = vec![0, 0, 0x12, 0xA0, 0x90, 0x00, 5, 0, 0, 0, 0];
        b.extend(page.encode("8902"));
        b.extend_from_slice(&[0, 0]);
        b.extend(page.encode("QS400   "));
        b.extend(page.encode("QSADSP01  "));
        let len = b.len() as u16;
        b[..2].copy_from_slice(&len.to_be_bytes());
        let rec = Record::parse(&b).unwrap();
        let s = startup_response(&rec, page).unwrap();
        assert_eq!(s.code, "8902");
        assert_eq!(s.device.as_deref(), Some("QSADSP01"));
        assert_eq!(s.outcome(), Outcome::InUse);
        assert!(is_rfc_startup_layout(&rec));
        // QS/400's older layout is still read, and told apart.
        let mut data = page.encode("I902");
        data.extend(page.encode("QSADSP01  "));
        let old = Record::new(0, data);
        assert!(!is_rfc_startup_layout(&old));
        assert_eq!(startup_response(&old, page).unwrap().code, "I902");
    }

    #[test]
    fn a_first_data_record_is_not_a_startup_response() {
        let rec = Record::new(opcode::PUT_GET, vec![0x04, 0x40]);
        assert!(startup_response(&rec, codepage::default_page()).is_none());
        let rec = Record::new(0, vec![0x04, 0x40, 0x04, 0x11]);
        assert!(startup_response(&rec, codepage::default_page()).is_none());
    }

    #[test]
    fn a_server_refusing_new_environ_gives_basic_mode() {
        let mut n = Negotiator::new(Request::display(ScreenSize::S24x80, Some("QSADSP01"), 37));
        n.receive(&[IAC, DO, TERMINAL_TYPE]);
        n.receive(&sb(TERMINAL_TYPE, &[SEND]));
        let ev = n.receive(&records_on());
        assert_eq!(ev, vec![Event::Ready(Mode::Basic)]);
    }

    #[test]
    fn new_environ_withdrawn_before_records_gives_basic_mode() {
        let mut n = Negotiator::new(Request::display(ScreenSize::S24x80, None, 37));
        n.receive(&[IAC, DO, NEW_ENVIRON]);
        n.receive(&[IAC, crate::telnet::DONT, NEW_ENVIRON]);
        assert!(n.take_output().ends_with(&[IAC, WONT, NEW_ENVIRON]));
        assert_eq!(n.receive(&records_on()), vec![Event::Ready(Mode::Basic)]);
    }

    #[test]
    fn send_parsing_handles_escapes_and_empty_requests() {
        assert_eq!(
            parse_send(&[VAR, USERVAR, b'D', b'E', b'V', ESC, 0x01]),
            vec![(VAR, vec![]), (USERVAR, b"DEV\x01".to_vec())]
        );
        assert!(parse_send(&[]).is_empty());
        assert_eq!(
            encode_is(&[("A".into(), "\u{3}".into())]),
            vec![IS, USERVAR, b'A', VALUE, ESC, 3]
        );
    }

    /// Start-up responses as QS/400 sent them (2026-11-01).
    #[test]
    fn qs400_startup_responses_as_captured() {
        let page = codepage::default_page();
        let parse = |hex: &str| {
            let b: Vec<u8> = hex
                .split(' ')
                .map(|h| u8::from_str_radix(h, 16).unwrap())
                .collect();
            startup_response(&Record::parse(&b).unwrap(), page).unwrap()
        };
        let ok = parse("00 18 12 A0 00 00 04 00 00 00 C9 F9 F0 F2 D8 E2 C1 E3 C5 E2 E3 F0 F1 40");
        assert_eq!(
            (ok.code.as_str(), ok.device.as_deref()),
            ("I902", Some("QSATEST01"))
        );
        let busy = parse("00 18 12 A0 00 00 04 00 00 00 F8 F9 F0 F2 D8 E2 C1 E3 C5 E2 E3 F0 F2 40");
        assert_eq!(busy.outcome(), Outcome::InUse);
        let bad = parse("00 18 12 A0 00 00 04 00 00 00 F8 F9 F0 F3 C2 C1 C4 4B D5 C1 D4 C5 5A 40");
        assert_eq!(bad.outcome(), Outcome::NotValid);
        assert_eq!(bad.device.as_deref(), Some("BAD.NAME!"));
    }

    #[test]
    fn response_outcomes() {
        let r = |c: &str| StartupResponse {
            code: c.into(),
            device: None,
        };
        assert_eq!(r("I902").outcome(), Outcome::Started);
        assert_eq!(r("8902").outcome(), Outcome::InUse);
        assert_eq!(r("8903").outcome(), Outcome::NotValid);
        assert_eq!(r("8940").outcome(), Outcome::Failed);
        assert_eq!(
            r("8940").meaning(),
            "automatic configuration failed or not allowed"
        );
    }
}
