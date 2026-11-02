// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! TN5250E negotiation (RFC 2877, RFC 4777) on top of [`crate::telnet`],
//! without I/O.
//!
//! The client answers the server's `DO TERMINAL-TYPE` and `DO NEW-ENVIRON`.
//! To the terminal type `SEND` it answers with the device's terminal type;
//! to the environment `SEND` with `DEVNAME` (when a device name was given),
//! `KBDTYPE`, `CODEPAGE` and `CHARSET`.
//!
//! Automatic sign-on ([`AutoSignOn`]) is by a one-time QAP Telnet ticket,
//! never by a password: the `USER` variable, the random-seed variable
//! present and empty, and the password variable carrying the ticket as 64
//! hexadecimal characters. The caller asks for it only over TLS. The ticket
//! is sent once, then wiped; it never appears in the trace. Without it the
//! user signs on at the system's own sign-on display.
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

/// A one-time QAP Telnet ticket as automatic sign-on sends it: 64
/// hexadecimal characters. It does not print, and is wiped when dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct Ticket([u8; 64]);

impl Ticket {
    /// The ticket's 32 bytes, written as hexadecimal.
    pub fn from_bytes(b: &[u8; 32]) -> Ticket {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut t = [0u8; 64];
        for (i, v) in b.iter().enumerate() {
            t[2 * i] = HEX[usize::from(v >> 4)];
            t[2 * i + 1] = HEX[usize::from(v & 0x0F)];
        }
        Ticket(t)
    }

    /// The characters, for the password variable.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for Ticket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ticket(..)")
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&self.0);
    }
}

/// Automatic sign-on by ticket: the profile and its ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoSignOn {
    pub user: String,
    pub ticket: Ticket,
}

/// What the client asks the server for. A printer session uses the same
/// request with a printer terminal type and its own variables in `extra`
/// (see [`crate::printer`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub terminal_type: String,
    /// `DEVNAME`, upper case; `None` lets the server choose.
    pub device_name: Option<String>,
    pub ccsid: u16,
    /// More `USERVAR`s, sent after the standard ones. Values are bytes:
    /// some printer variables are binary.
    pub extra: Vec<(String, Vec<u8>)>,
    /// Sign on automatically with a ticket (only over TLS).
    pub auto_sign_on: Option<AutoSignOn>,
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
            auto_sign_on: None,
        }
    }

    /// The `USERVAR`s this request sends, in order.
    pub fn user_vars(&self) -> Vec<(String, Vec<u8>)> {
        let env = codepage::environ(self.ccsid);
        let mut v = Vec::new();
        if let Some(d) = &self.device_name {
            v.push(("DEVNAME".to_owned(), d.clone().into_bytes()));
        }
        v.push(("KBDTYPE".to_owned(), env.kbdtype.as_bytes().to_vec()));
        v.push(("CODEPAGE".to_owned(), env.codepage.to_string().into_bytes()));
        v.push(("CHARSET".to_owned(), env.charset.to_string().into_bytes()));
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
    /// The server's system name, when the record has RFC 4777's layout.
    pub system: Option<String>,
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

/// RFC 4777 section 10: in the start-up response record, after twelve bytes
/// of fixed fields, the response code (4 bytes), the system name (8) and
/// the device name (10), all EBCDIC.
const RFC_CODE_AT: usize = 16;
const RFC_SYSTEM_LEN: usize = 8;
const RFC_DEVICE_LEN: usize = 10;

/// Whether four characters are a response code: a letter `I` or a digit,
/// then three digits.
fn is_code(code: &str) -> bool {
    let c: Vec<char> = code.chars().collect();
    c.len() == 4
        && (c[0] == 'I' || c[0].is_ascii_digit())
        && c[1..].iter().all(char::is_ascii_digit)
}

/// An EBCDIC name field, trimmed of blanks and padding nulls.
fn name_field(bytes: &[u8], page: &dyn CodePage) -> Option<String> {
    let text = page.decode(bytes);
    Some(
        text.trim_matches(|c: char| c == ' ' || c == '\0')
            .to_owned(),
    )
    .filter(|t| !t.is_empty())
}

/// Reads a start-up response from a whole record (before Telnet framing),
/// if it is one.
///
/// - RFC 4777's layout (reserved field `9000`): the code, system name and
///   device name at their fixed places (section 10).
/// - The older layout QS/400 still sends: opcode 0, then the code, then the
///   device name as exactly ten characters, else as the last name-like run
///   of characters after the code. Logged as a warning, since QS/400 is
///   moving to RFC 4777's layout.
///
/// Anything else, a first data record say, is not a start-up response.
pub fn startup_response(bytes: &[u8], page: &dyn CodePage) -> Option<StartupResponse> {
    let rec = Record::parse(bytes).ok()?;
    if is_rfc_startup_layout(&rec) {
        if let Some(code_bytes) = bytes.get(RFC_CODE_AT..RFC_CODE_AT + 4) {
            let code = page.decode(code_bytes);
            if is_code(&code) {
                let sys_at = RFC_CODE_AT + 4;
                let dev_at = sys_at + RFC_SYSTEM_LEN;
                let field = |at: usize, len: usize| {
                    bytes
                        .get(at.min(bytes.len())..(at + len).min(bytes.len()))
                        .and_then(|b| name_field(b, page))
                };
                return Some(StartupResponse {
                    code,
                    system: field(sys_at, RFC_SYSTEM_LEN),
                    device: field(dev_at, RFC_DEVICE_LEN),
                });
            }
        }
    } else if rec.opcode != opcode::NO_OP {
        return None;
    }
    let text = page.decode(&rec.data);
    let mut chars = text.chars();
    let code: String = chars.by_ref().take(4).collect();
    if !is_code(&code) {
        return None;
    }
    if !is_rfc_startup_layout(&rec) {
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
    Some(StartupResponse {
        code,
        device,
        system: None,
    })
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

    /// Queues a record already encoded, before Telnet framing.
    pub fn send_bytes(&mut self, bytes: &[u8]) {
        self.telnet.send_record(bytes);
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
            if let Some(s) = startup_response(bytes, self.page) {
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
        let mut ours = self.request.user_vars();
        // Automatic sign-on: the seed present and empty, the ticket where
        // the password goes. Sent once.
        let auto = self.request.auto_sign_on.take();
        if let Some(a) = &auto {
            ours.push((wire::RANDOM_SEED.to_owned(), Vec::new()));
            ours.push((
                wire::PASSWORD_SUBSTITUTE.to_owned(),
                a.ticket.expose().to_vec(),
            ));
        }
        let mut answer: Vec<(String, Vec<u8>)> = Vec::new();
        let mut want = |name: &str| {
            if let Some(v) = ours.iter().find(|(n, _)| n == name)
                && !answer.iter().any(|(n, _)| n == name)
            {
                answer.push(v.clone());
            }
        };
        let mut user = asked.is_empty();
        if asked.is_empty() {
            ours.iter().for_each(|(n, _)| want(n));
        }
        let seed = wire::RANDOM_SEED.as_bytes();
        for item in &asked {
            match item {
                (USERVAR, name) if name.is_empty() => ours.iter().for_each(|(n, _)| want(n)),
                // The seed's name carries the server's seed bytes.
                (USERVAR, name) if name.starts_with(seed) => want(wire::RANDOM_SEED),
                (USERVAR, name) => want(&String::from_utf8_lossy(name)),
                (VAR, name) if name.is_empty() || name.as_slice() == b"USER" => user = true,
                _ => {}
            }
        }
        let user = auto
            .as_ref()
            .filter(|_| user)
            .map(|a| a.user.trim().to_ascii_uppercase());
        let mut payload = encode_is_with_user(user.as_deref(), &answer);
        self.telnet.send_subneg(NEW_ENVIRON, &payload);
        payload.fill(0);
        self.environ_sent = true;
        let mut shown: Vec<String> = Vec::new();
        if let Some(u) = &user {
            shown.push(format!("VAR USER={u}"));
        }
        shown.extend(answer.iter().map(|(n, v)| {
            if n == wire::PASSWORD_SUBSTITUTE {
                format!("USERVAR {n}=(ticket, {} characters)", v.len())
            } else {
                format!("USERVAR {n}={}", show_value(v))
            }
        }));
        for (n, v) in &mut answer {
            if n == wire::PASSWORD_SUBSTITUTE {
                v.fill(0);
            }
        }
        for (n, v) in &mut ours {
            if n == wire::PASSWORD_SUBSTITUTE {
                v.fill(0);
            }
        }
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

/// The variables of an `IS` body (after `IS`): type, name and value, RFC
/// 1572 escapes undone. A variable without `VALUE` has an empty one.
pub fn parse_is(body: &[u8]) -> Vec<(u8, Vec<u8>, Vec<u8>)> {
    let mut out: Vec<(u8, Vec<u8>, Vec<u8>)> = Vec::new();
    let mut i = 0;
    let mut in_value = false;
    while i < body.len() {
        match body[i] {
            k @ (VAR | USERVAR) => {
                out.push((k, Vec::new(), Vec::new()));
                in_value = false;
            }
            VALUE => in_value = true,
            b => {
                let b = if b == ESC && i + 1 < body.len() {
                    i += 1;
                    body[i]
                } else {
                    b
                };
                if let Some(last) = out.last_mut() {
                    if in_value {
                        last.2.push(b);
                    } else {
                        last.1.push(b);
                    }
                }
            }
        }
        i += 1;
    }
    out
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

/// A value for the debug trace: text when it is printable ASCII, else hex.
fn show_value(v: &[u8]) -> String {
    if v.iter().all(|b| (0x20..0x7F).contains(b)) {
        String::from_utf8_lossy(v).into_owned()
    } else {
        format!("x'{}'", crate::log::hex(v).replace(' ', ""))
    }
}

fn push_escaped(out: &mut Vec<u8>, s: &[u8]) {
    for &b in s {
        if b <= USERVAR {
            out.push(ESC);
        }
        out.push(b);
    }
}

/// `IS USERVAR name VALUE value ...`, escaped as RFC 1572 says. Telnet's
/// own `IAC` doubling happens later, in the codec.
pub fn encode_is(vars: &[(String, Vec<u8>)]) -> Vec<u8> {
    encode_is_with_user(None, vars)
}

/// [`encode_is`], with `VAR USER VALUE user` first when `user` is given.
pub fn encode_is_with_user(user: Option<&str>, vars: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![IS];
    if let Some(u) = user {
        out.push(VAR);
        push_escaped(&mut out, b"USER");
        out.push(VALUE);
        push_escaped(&mut out, u.as_bytes());
    }
    for (n, v) in vars {
        out.push(USERVAR);
        push_escaped(&mut out, n.as_bytes());
        out.push(VALUE);
        push_escaped(&mut out, v);
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

    /// The variables of the `IS` in `out`.
    fn is_vars(out: &[u8]) -> Vec<(u8, Vec<u8>, Vec<u8>)> {
        let start = out
            .windows(4)
            .position(|w| w == [IAC, SB, NEW_ENVIRON, IS])
            .expect("an IS")
            + 4;
        let end = start
            + out[start..]
                .windows(2)
                .position(|w| w == [IAC, SE])
                .unwrap();
        parse_is(&out[start..end])
    }

    #[test]
    fn automatic_sign_on_sends_the_user_an_empty_seed_and_the_ticket_once() {
        let mut r = Request::display(ScreenSize::S24x80, Some("qsadsp01"), 37);
        r.auto_sign_on = Some(AutoSignOn {
            user: "bob".into(),
            ticket: Ticket::from_bytes(&[0xAB; 32]),
        });
        assert!(!format!("{r:?}").contains("ABAB"), "{r:?}");
        let (mut n, env) = opened(r);
        let vars = is_vars(&env);
        assert_eq!(vars[0], (VAR, b"USER".to_vec(), b"BOB".to_vec()));
        let get = |vars: &[(u8, Vec<u8>, Vec<u8>)], name: &str| {
            vars.iter()
                .find(|(k, n, _)| *k == USERVAR && n == name.as_bytes())
                .map(|(_, _, v)| v.clone())
        };
        assert_eq!(get(&vars, wire::RANDOM_SEED), Some(Vec::new()));
        assert_eq!(
            get(&vars, wire::PASSWORD_SUBSTITUTE),
            Some(b"AB".repeat(32))
        );
        assert_eq!(get(&vars, "DEVNAME"), Some(b"QSADSP01".to_vec()));
        // Used once: a second SEND gets no ticket and no user.
        n.receive(&seeded_send());
        let again = is_vars(&n.take_output());
        assert!(again.iter().all(|(k, _, _)| *k == USERVAR));
        assert_eq!(get(&again, wire::PASSWORD_SUBSTITUTE), None);
        assert_eq!(
            parse_is(&encode_is(&[("A".into(), vec![1, 2])])[1..]),
            [(USERVAR, b"A".to_vec(), vec![1, 2])]
        );
    }

    #[test]
    fn ticket_hex() {
        let mut b = [0u8; 32];
        b[0] = 0x0F;
        b[31] = 0xF0;
        let t = Ticket::from_bytes(&b);
        assert_eq!(&t.expose()[..4], b"0F00");
        assert_eq!(&t.expose()[60..], b"00F0");
        assert_eq!(format!("{t:?}"), "Ticket(..)");
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
                device: Some("QSADSP01".into()),
                system: None,
            })]
        );
        // Later records pass through.
        let rec = Record::new(opcode::PUT_GET, vec![0x04, 0x40]).encode();
        let ev = n.receive(&telnet::codec::encode_record(&rec));
        assert!(matches!(&ev[..], [Event::Record(r)] if r.opcode == opcode::PUT_GET));
    }

    #[test]
    fn rfc_shaped_startup_response() {
        let page = codepage::default_page();
        // RFC 4777 section 10.2, figure 2: device PCPRINTER not available
        // on system TARGET.
        let b = hex(concat!(
            "004912A09000056006008200003D0000F8F9F0F2E3C1D9C7C5E34040D7C3D7D9",
            "C9D5E3C5D9400000000000000000000000000000000000000000000000000000",
            "000000000000000000"
        ));
        let s = startup_response(&b, page).unwrap();
        assert_eq!(s.code, "8902");
        assert_eq!(s.system.as_deref(), Some("TARGET"));
        assert_eq!(s.device.as_deref(), Some("PCPRINTER"));
        assert_eq!(s.outcome(), Outcome::InUse);
        assert!(is_rfc_startup_layout(&Record::parse(&b).unwrap()));
        // Section 12: success for printer DUMMYPRT on ELCRTP06.
        let b = hex(concat!(
            "004912A090000560060020C0003D0000C9F9F0F2C5D3C3D9E3D7F0F6C4E4D4D4",
            "E8D7D9E340400000000000000000000000000000000000000000000000000000",
            "000000000000000000"
        ));
        let s = startup_response(&b, page).unwrap();
        assert_eq!(
            (s.code.as_str(), s.system.as_deref(), s.device.as_deref()),
            ("I902", Some("ELCRTP06"), Some("DUMMYPRT"))
        );
        assert_eq!(s.outcome(), Outcome::Started);
        // QS/400's older layout is still read, and told apart.
        let mut data = page.encode("I902");
        data.extend(page.encode("QSADSP01  "));
        let old = Record::new(0, data);
        assert!(!is_rfc_startup_layout(&old));
        let s = startup_response(&old.encode(), page).unwrap();
        assert_eq!((s.code.as_str(), s.system), ("I902", None));
        // Cut short: no panic, and what is there is read.
        let b = hex("001412A090000560060020C0003D0000C9F9F0F2");
        assert_eq!(startup_response(&b, page).unwrap().device, None);
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn a_first_data_record_is_not_a_startup_response() {
        let page = codepage::default_page();
        let rec = Record::new(opcode::PUT_GET, vec![0x04, 0x40]);
        assert!(startup_response(&rec.encode(), page).is_none());
        let rec = Record::new(0, vec![0x04, 0x40, 0x04, 0x11]);
        assert!(startup_response(&rec.encode(), page).is_none());
        // A print record (data flow 0101) is not one either.
        let b = hex("001112A001010A08000100000000000000");
        assert!(startup_response(&b, page).is_none());
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
            encode_is(&[("A".into(), vec![3])]),
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
            startup_response(&b, page).unwrap()
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
            system: None,
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
