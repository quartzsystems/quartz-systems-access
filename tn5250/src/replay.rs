// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Recording sessions and replaying them without a network.
//!
//! A recording is a JSON-lines file. The first line describes the display;
//! each further line is one event with its time in milliseconds from the
//! start:
//!
//! ```text
//! {"v":1,"size":"24x80","ccsid":37}
//! {"t":12,"d":"in","x":"0378 12A0 ..."}
//! {"t":4031,"d":"key","k":"Char:U+0051"}
//! {"t":4950,"d":"out","x":"..."}
//! ```
//!
//! `in` and `out` are records (header and data, before Telnet framing) in
//! hex; `key` is a 5250 key as [`key_name`] writes it.
//!
//! A printer session's recording has `"kind":"printer"` and the device in
//! its first line, and only `in` and `out` records:
//!
//! ```text
//! {"v":1,"kind":"printer","device":"QSAPRT01","ccsid":37}
//! ```
//!
//! [`replay_printer`] feeds its records to a [`Printer`], checks the answers
//! the client sent, and gives back each job's print data.
//!
//! Nothing typed into a nondisplay (password) field reaches the file: such
//! keys are written as `X` (blanks stay blanks), and the same field's bytes
//! in outbound records are rewritten to match, so a scrubbed recording
//! still replays exactly. The length of what was typed remains visible.
//!
//! [`replay`] drives a [`Terminal`] with the events and checks that it
//! sends what was recorded; [`Golden`] is the final screen as text, to
//! compare with a stored one. [`Host`] plays the host's side of a
//! recording to a terminal driven by something else (a macro).

use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use crate::input::{Key, Outcome};
use crate::negotiate::ScreenSize;
use crate::printer::{Handled, PrintEvent, Printer, Response};
use crate::record::Record;
use crate::screen::{Read, Screen, attr, byte};
use crate::session::Direction;
use crate::stream::order;
use crate::terminal::Terminal;

/// One recorded event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    In(Vec<u8>),
    Out(Vec<u8>),
    Key(Key),
}

/// A whole recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recording {
    pub size: ScreenSize,
    pub ccsid: u16,
    /// A printer session's device; `None` for a display session.
    pub printer: Option<String>,
    pub events: Vec<(u64, Event)>,
}

/// Writes a recording as the session runs.
pub struct Recorder {
    out: BufWriter<File>,
    start: Instant,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Recorder")
    }
}

impl Recorder {
    /// Creates `path` and writes the header line.
    pub fn create(path: &Path, size: ScreenSize, ccsid: u16) -> io::Result<Recorder> {
        let mut out = BufWriter::new(File::create(path)?);
        writeln!(
            out,
            "{{\"v\":1,\"size\":\"{}\",\"ccsid\":{ccsid}}}",
            size_name(size)
        )?;
        out.flush()?;
        Ok(Recorder {
            out,
            start: Instant::now(),
        })
    }

    /// Creates `path` for a printer session's recording.
    pub fn create_printer(path: &Path, device: &str, ccsid: u16) -> io::Result<Recorder> {
        let mut out = BufWriter::new(File::create(path)?);
        let device: String = device
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || "#$@_".contains(*c))
            .collect();
        writeln!(
            out,
            "{{\"v\":1,\"kind\":\"printer\",\"device\":\"{device}\",\"ccsid\":{ccsid}}}"
        )?;
        out.flush()?;
        Ok(Recorder {
            out,
            start: Instant::now(),
        })
    }

    /// A record either way, as [`crate::session::Session`]'s recorder hook
    /// gives it (a printer session has no screen, so nothing to scrub).
    pub fn record(&mut self, direction: Direction, bytes: &[u8]) {
        let d = match direction {
            Direction::In => "in",
            Direction::Out => "out",
        };
        self.line(d, "x", &hex(bytes));
    }

    fn line(&mut self, d: &str, field: &str, value: &str) {
        let t = self.start.elapsed().as_millis();
        let _ = writeln!(
            self.out,
            "{{\"t\":{t},\"d\":\"{d}\",\"{field}\":\"{value}\"}}"
        );
        let _ = self.out.flush();
    }

    /// A record from the host.
    pub fn inbound(&mut self, rec: &Record) {
        self.line("in", "x", &hex(&rec.encode()));
    }

    /// A key, given the screen as it was before the key: typing into a
    /// nondisplay field is scrubbed.
    pub fn key(&mut self, before: &Screen, k: Key) {
        let k = scrub_key(before, k);
        self.line("key", "k", &key_name(k));
    }

    /// A record to the host, given the screen it was built from (for a
    /// key's record, the screen after the key).
    pub fn outbound(&mut self, screen: &Screen, rec: &Record) {
        let mut rec = rec.clone();
        scrub_record(screen, &mut rec);
        self.line("out", "x", &hex(&rec.encode()));
    }
}

/// The scrubbed form of a key typed with the cursor in a nondisplay field.
pub fn scrub_key(before: &Screen, k: Key) -> Key {
    match k {
        Key::Char(c) if in_nondisplay_field(before, before.cursor) => {
            Key::Char(if c == ' ' { ' ' } else { 'X' })
        }
        k => k,
    }
}

fn in_nondisplay_field(s: &Screen, addr: usize) -> bool {
    s.field_at(addr).is_some_and(|i| s.fields[i].nondisplay())
}

fn scrub_byte(s: &Screen, b: u8) -> u8 {
    if b == byte::NULL || b == byte::BLANK {
        b
    } else {
        s.page().to_byte('X').unwrap_or(b)
    }
}

/// Rewrites the bytes of nondisplay fields in a read reply.
pub fn scrub_record(s: &Screen, rec: &mut Record) {
    if rec.flags != 0 || rec.data.len() <= 3 {
        return;
    }
    let d = &mut rec.data;
    if d[3] == order::SBA {
        // Read MDT Fields: SBA row col, data, ...
        let mut i = 3;
        let mut hidden = false;
        while i < d.len() {
            if d[i] == order::SBA && i + 2 < d.len() {
                let (r, c) = (d[i + 1] as usize, d[i + 2] as usize);
                hidden = r > 0
                    && c > 0
                    && r <= s.rows()
                    && c <= s.cols()
                    && in_nondisplay_field(s, s.addr(r - 1, c - 1));
                i += 3;
                continue;
            }
            if hidden {
                d[i] = scrub_byte(s, d[i]);
            }
            i += 1;
        }
    } else {
        // Read Input Fields: the input fields one after the other.
        let mut i = 3;
        for (_, f) in s.input_fields() {
            let end = (i + f.len).min(d.len());
            if f.nondisplay() {
                for b in &mut d[i..end] {
                    *b = scrub_byte(s, *b);
                }
            }
            i = end;
        }
    }
}

fn size_name(size: ScreenSize) -> &'static str {
    match size {
        ScreenSize::S24x80 => "24x80",
        ScreenSize::S27x132 => "27x132",
    }
}

fn hex(b: &[u8]) -> String {
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02X}");
            s
        })
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    let s: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    s.chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
        .collect()
}

/// A key's name in recordings.
pub fn key_name(k: Key) -> String {
    match k {
        Key::Char(c) => format!("Char:U+{:04X}", c as u32),
        Key::F(n) => format!("F:{n}"),
        other => format!("{other:?}"),
    }
}

/// The key a name stands for.
pub fn key_from_name(s: &str) -> Option<Key> {
    if let Some(cp) = s.strip_prefix("Char:U+") {
        return char::from_u32(u32::from_str_radix(cp, 16).ok()?).map(Key::Char);
    }
    if let Some(n) = s.strip_prefix("F:") {
        return n.parse().ok().filter(|n| (1..=24).contains(n)).map(Key::F);
    }
    Some(match s {
        "Enter" => Key::Enter,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "Clear" => Key::Clear,
        "Help" => Key::Help,
        "Print" => Key::Print,
        "RecordBackspace" => Key::RecordBackspace,
        "FieldExit" => Key::FieldExit,
        "FieldPlus" => Key::FieldPlus,
        "FieldMinus" => Key::FieldMinus,
        "Dup" => Key::Dup,
        "EraseInput" => Key::EraseInput,
        "EraseEof" => Key::EraseEof,
        "Insert" => Key::Insert,
        "Delete" => Key::Delete,
        "Backspace" => Key::Backspace,
        "Home" => Key::Home,
        "NewLine" => Key::NewLine,
        "Tab" => Key::Tab,
        "BackTab" => Key::BackTab,
        "Left" => Key::Left,
        "Right" => Key::Right,
        "Up" => Key::Up,
        "Down" => Key::Down,
        "Reset" => Key::Reset,
        "SysReq" => Key::SysReq,
        "Attention" => Key::Attention,
        _ => return None,
    })
}

/// A value from one of our JSON lines: `"name":"text"` or `"name":123`.
fn value<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("\"{name}\":");
    let rest = &line[line.find(&key)? + key.len()..];
    if let Some(s) = rest.strip_prefix('"') {
        return s.find('"').map(|e| &s[..e]);
    }
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    Some(rest[..end].trim())
}

/// Why a recording could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("line {0}: {1}")]
    Bad(usize, &'static str),
}

impl Recording {
    /// Reads a recording file.
    pub fn read(path: &Path) -> Result<Recording, ReadError> {
        Recording::parse(BufReader::new(File::open(path)?))
    }

    pub fn parse(input: impl BufRead) -> Result<Recording, ReadError> {
        let mut lines = input.lines().enumerate();
        let (_, head) = lines.next().ok_or(ReadError::Bad(1, "empty file"))?;
        let head = head?;
        if value(&head, "v") != Some("1") {
            return Err(ReadError::Bad(1, "not a version 1 recording"));
        }
        let printer = (value(&head, "kind") == Some("printer"))
            .then(|| value(&head, "device").unwrap_or("").to_owned());
        let size = match value(&head, "size") {
            Some("24x80") => ScreenSize::S24x80,
            Some("27x132") => ScreenSize::S27x132,
            None if printer.is_some() => ScreenSize::S24x80,
            _ => return Err(ReadError::Bad(1, "size must be 24x80 or 27x132")),
        };
        let ccsid = value(&head, "ccsid")
            .and_then(|c| c.parse().ok())
            .ok_or(ReadError::Bad(1, "ccsid missing"))?;
        let mut events = Vec::new();
        for (n, line) in lines {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let n = n + 1;
            let t = value(&line, "t")
                .and_then(|t| t.parse().ok())
                .ok_or(ReadError::Bad(n, "time missing"))?;
            let ev = match value(&line, "d") {
                Some("in") => Event::In(
                    value(&line, "x")
                        .and_then(unhex)
                        .ok_or(ReadError::Bad(n, "bad hex"))?,
                ),
                Some("out") => Event::Out(
                    value(&line, "x")
                        .and_then(unhex)
                        .ok_or(ReadError::Bad(n, "bad hex"))?,
                ),
                Some("key") => Event::Key(
                    value(&line, "k")
                        .and_then(key_from_name)
                        .ok_or(ReadError::Bad(n, "unknown key"))?,
                ),
                _ => return Err(ReadError::Bad(n, "direction must be in, out or key")),
            };
            events.push((t, ev));
        }
        Ok(Recording {
            size,
            ccsid,
            printer,
            events,
        })
    }
}

/// The result of replaying a printer session.
#[derive(Debug, Default)]
pub struct PrinterReplay {
    /// Each job's print data, in the order the jobs ended. A job still open
    /// at the end of the recording comes last.
    pub jobs: Vec<Vec<u8>>,
    /// Jobs that were host print transform output.
    pub transformed: usize,
    /// Records the client refused with a negative response.
    pub rejected: usize,
    /// Answers the client sent that are not what this client sends for a
    /// stored record (print complete): (event index, recorded, expected).
    pub divergences: Vec<(usize, Vec<u8>, Vec<u8>)>,
}

/// Replays a printer session's records through a [`Printer`].
pub fn replay_printer(rec: &Recording) -> PrinterReplay {
    let mut p = Printer::new();
    let mut out = PrinterReplay::default();
    let mut job: Option<(Vec<u8>, bool)> = None;
    let mut expected: std::collections::VecDeque<Vec<u8>> = Default::default();
    let start = Instant::now();
    for (i, (_, ev)) in rec.events.iter().enumerate() {
        match ev {
            Event::In(bytes) => {
                let Ok(r) = Record::parse(bytes) else {
                    continue;
                };
                match p.record(&r, start) {
                    Handled::Caller { events, opcode } => {
                        for e in events {
                            match e {
                                PrintEvent::JobStarted { transformed, .. } => {
                                    job = Some((Vec::new(), transformed));
                                }
                                PrintEvent::Data { bytes, .. } => {
                                    job.get_or_insert_with(|| (Vec::new(), false))
                                        .0
                                        .extend_from_slice(&bytes);
                                }
                                PrintEvent::JobEnded { .. } => match job.take() {
                                    Some((_, true)) => out.transformed += 1,
                                    Some((data, false)) => out.jobs.push(data),
                                    None => {}
                                },
                                PrintEvent::Cleared { .. } => job = None,
                            }
                        }
                        expected.push_back(Response::Complete.encode(opcode));
                    }
                    Handled::Rejected {
                        response, opcode, ..
                    } => {
                        out.rejected += 1;
                        expected.push_back(response.encode(opcode));
                    }
                }
            }
            Event::Out(bytes) => {
                let want = expected.pop_front().unwrap_or_default();
                if *bytes != want {
                    out.divergences.push((i, bytes.clone(), want));
                }
            }
            Event::Key(_) => {}
        }
    }
    if let Some((data, false)) = job {
        out.jobs.push(data);
    }
    out
}

/// The result of a replay.
#[derive(Debug)]
pub struct Replayed {
    pub terminal: Terminal,
    /// Outbound records that differ from the recording: (event index,
    /// recorded, produced).
    pub divergences: Vec<(usize, Vec<u8>, Vec<u8>)>,
    /// Data stream errors met on the way.
    pub errors: Vec<String>,
}

/// Plays a recording through a terminal.
pub fn replay(rec: &Recording) -> Replayed {
    let mut terminal = Terminal::new(rec.size, rec.ccsid);
    let mut produced: std::collections::VecDeque<Vec<u8>> = Default::default();
    let mut divergences = Vec::new();
    let mut errors = Vec::new();
    for (i, (_, ev)) in rec.events.iter().enumerate() {
        match ev {
            Event::In(bytes) => match Record::parse(bytes) {
                Ok(r) => {
                    let a = terminal.receive(&r);
                    errors.extend(a.errors.iter().map(|e| format!("event {i}: {e}")));
                    for mut out in a.replies {
                        scrub_record(&terminal.screen, &mut out);
                        produced.push_back(out.encode());
                    }
                }
                Err(e) => errors.push(format!("event {i}: {e}")),
            },
            Event::Key(k) => {
                for mut out in terminal.key(*k).send {
                    scrub_record(&terminal.screen, &mut out);
                    produced.push_back(out.encode());
                }
            }
            Event::Out(want) => {
                let got = produced.pop_front().unwrap_or_default();
                if &got != want {
                    divergences.push((i, want.clone(), got));
                }
            }
        }
    }
    Replayed {
        terminal,
        divergences,
        errors,
    }
}

/// The host's side of a recording, for a terminal that something other
/// than the recording drives (a macro, say).
///
/// The recorded inbound records are delivered in order. Each time the
/// terminal sends a record, the host moves on past the next recorded
/// outbound record and delivers what followed it. The recorded keystrokes
/// are not used: whoever drives the terminal supplies its own. What the
/// terminal sends is not compared byte for byte (a macro may type a
/// different password than the scrubbed one), only counted.
#[derive(Debug)]
pub struct Host {
    events: Vec<Event>,
    pos: usize,
    pub terminal: Terminal,
    /// Records the terminal sent that the recording has not answered yet.
    pending: usize,
    /// Data stream errors met on the way.
    pub errors: Vec<String>,
}

impl Host {
    /// A host for `rec`, with the records it sends before the terminal's
    /// first record already delivered.
    pub fn new(rec: &Recording) -> Host {
        let mut h = Host {
            events: rec.events.iter().map(|(_, e)| e.clone()).collect(),
            pos: 0,
            terminal: Terminal::new(rec.size, rec.ccsid),
            pending: 0,
            errors: Vec::new(),
        };
        h.run();
        h
    }

    fn sent(&mut self, n: usize) {
        self.pending += n;
        self.run();
    }

    /// Delivers recorded records until the recording waits for the
    /// terminal to send something.
    fn run(&mut self) {
        while let Some(ev) = self.events.get(self.pos) {
            match ev {
                Event::In(bytes) => {
                    self.pos += 1;
                    match Record::parse(bytes) {
                        Ok(r) => {
                            let a = self.terminal.receive(&r);
                            self.errors.extend(
                                a.errors.iter().map(|e| format!("event {}: {e}", self.pos)),
                            );
                            self.pending += a.replies.len();
                        }
                        Err(e) => self.errors.push(format!("event {}: {e}", self.pos)),
                    }
                }
                Event::Key(_) => self.pos += 1,
                Event::Out(_) if self.pending > 0 => {
                    self.pending -= 1;
                    self.pos += 1;
                }
                Event::Out(_) => break,
            }
        }
    }

    /// A key typed into the terminal; the host answers what it sends.
    pub fn key(&mut self, k: Key) -> Outcome {
        let out = self.terminal.key(k);
        let n = out.send.len();
        self.sent(n);
        out
    }

    /// Text typed into the terminal.
    pub fn paste(&mut self, text: &str) -> Outcome {
        let out = self.terminal.paste(text);
        let n = out.send.len();
        self.sent(n);
        out
    }

    /// Whether every recorded event has been played.
    pub fn finished(&self) -> bool {
        self.pos >= self.events.len()
    }
}

/// The screen as golden text: characters, attributes, fields, cursor and
/// keyboard state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Golden {
    pub rows: usize,
    pub cols: usize,
    pub chars: Vec<String>,
    pub attrs: Vec<String>,
    pub fields: Vec<String>,
    pub cursor: (usize, usize),
    pub keyboard: String,
}

/// One character per cell for the attribute grid: `#` on an attribute
/// byte, else the attribute in force as `0`-`9`, `A`-`V` (`0x20`-`0x3F`).
fn attr_char(cell_byte: u8, a: u8) -> char {
    if attr::is_attr(cell_byte) {
        return '#';
    }
    let v = (a.saturating_sub(0x20) & 0x1F) as u32;
    char::from_digit(v, 32).map_or('?', |c| c.to_ascii_uppercase())
}

impl Golden {
    pub fn of(s: &Screen) -> Golden {
        let cols = s.cols();
        let chars = s.text_rows();
        let attrs = (0..s.rows())
            .map(|r| {
                (0..cols)
                    .map(|c| {
                        let i = s.addr(r, c);
                        attr_char(s.byte(i), s.attrs()[i])
                    })
                    .collect()
            })
            .collect();
        let fields = s
            .fields
            .iter()
            .map(|f| {
                let (r, c) = s.rc(f.start);
                let fcw: Vec<String> = f.fcw.iter().map(|w| format!("{w:04X}")).collect();
                format!(
                    "{:02}/{:03} len {} ffw {:04X} attr {:02X} fcw {}",
                    r + 1,
                    c + 1,
                    f.len,
                    f.ffw.0 & !crate::screen::Ffw::MDT,
                    f.attr,
                    if fcw.is_empty() {
                        "-".into()
                    } else {
                        fcw.join(",")
                    }
                )
            })
            .collect();
        let (r, c) = s.rc(s.cursor);
        Golden {
            rows: s.rows(),
            cols,
            chars,
            attrs,
            fields,
            cursor: (r + 1, c + 1),
            keyboard: format!(
                "{:?}{}",
                s.keyboard,
                match s.pending_read {
                    Some(Read::InputFields) => " read-input",
                    Some(Read::MdtFields) => " read-mdt",
                    Some(Read::MdtAlternate) => " read-mdt-alternate",
                    None => "",
                }
            ),
        }
    }

    /// The text form stored in `.golden` files.
    pub fn to_text(&self) -> String {
        let mut t = String::new();
        let _ = writeln!(t, "size {}x{}", self.rows, self.cols);
        let _ = writeln!(t, "cursor {:02}/{:03}", self.cursor.0, self.cursor.1);
        let _ = writeln!(t, "keyboard {}", self.keyboard);
        t.push_str("chars\n");
        for r in &self.chars {
            let _ = writeln!(t, "|{r}|");
        }
        t.push_str("attrs\n");
        for r in &self.attrs {
            let _ = writeln!(t, "|{r}|");
        }
        t.push_str("fields\n");
        for f in &self.fields {
            let _ = writeln!(t, "{f}");
        }
        t
    }

    /// Reads [`Golden::to_text`].
    pub fn parse(text: &str) -> Option<Golden> {
        let text = text.replace("\r\n", "\n");
        let mut lines = text.lines();
        let size = lines.next()?.strip_prefix("size ")?;
        let (rows, cols) = size.split_once('x')?;
        let (rows, cols): (usize, usize) = (rows.parse().ok()?, cols.parse().ok()?);
        let cur = lines.next()?.strip_prefix("cursor ")?;
        let (cr, cc) = cur.split_once('/')?;
        let cursor = (cr.parse().ok()?, cc.parse().ok()?);
        let keyboard = lines.next()?.strip_prefix("keyboard ")?.to_owned();
        let mut grid = |name: &str| -> Option<Vec<String>> {
            if lines.next()? != name {
                return None;
            }
            (0..rows)
                .map(|_| {
                    let l = lines.next()?;
                    Some(l.strip_prefix('|')?.strip_suffix('|')?.to_owned())
                })
                .collect()
        };
        let chars = grid("chars")?;
        let attrs = grid("attrs")?;
        if lines.next()? != "fields" {
            return None;
        }
        let fields = lines.map(str::to_owned).collect();
        Some(Golden {
            rows,
            cols,
            chars,
            attrs,
            fields,
            cursor,
            keyboard,
        })
    }

    /// Where two goldens differ, as a report: the grids side by side,
    /// differing cells marked with `^` beneath; then the other lines.
    /// `None` when they are the same.
    pub fn diff(want: &Golden, got: &Golden) -> Option<String> {
        if want == got {
            return None;
        }
        let mut out = String::new();
        if (want.rows, want.cols) != (got.rows, got.cols) {
            let _ = writeln!(
                out,
                "size: want {}x{}, got {}x{}",
                want.rows, want.cols, got.rows, got.cols
            );
            return Some(out);
        }
        for (name, w, g) in [
            ("characters", &want.chars, &got.chars),
            ("attributes", &want.attrs, &got.attrs),
        ] {
            if w == g {
                continue;
            }
            let pad = want.cols;
            let _ = writeln!(out, "{name}: want (left), got (right)");
            for (r, (wl, gl)) in w.iter().zip(g).enumerate() {
                let _ = writeln!(out, "{:2} |{wl:<pad$}| |{gl:<pad$}|", r + 1);
                if wl != gl {
                    let marks: String = wl
                        .chars()
                        .zip(gl.chars())
                        .map(|(a, b)| if a == b { ' ' } else { '^' })
                        .collect();
                    let _ = writeln!(out, "    {marks:<pad$}    {marks}");
                }
            }
        }
        if want.cursor != got.cursor {
            let _ = writeln!(out, "cursor: want {:?}, got {:?}", want.cursor, got.cursor);
        }
        if want.keyboard != got.keyboard {
            let _ = writeln!(
                out,
                "keyboard: want {}, got {}",
                want.keyboard, got.keyboard
            );
        }
        if want.fields != got.fields {
            let _ = writeln!(out, "fields: want");
            for f in &want.fields {
                let _ = writeln!(out, "  {f}");
            }
            let _ = writeln!(out, "fields: got");
            for f in &got.fields {
                let _ = writeln!(out, "  {f}");
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A print record from the server.
    fn print_record(flags: u16, data: &[u8]) -> Vec<u8> {
        let mut r = vec![0, 0, 0x12, 0xA0, 0x01, 0x01, 10];
        r.extend_from_slice(&flags.to_be_bytes());
        r.push(crate::printer::op::PRINT);
        r.extend_from_slice(&[0; 6]);
        r.extend_from_slice(data);
        let len = r.len() as u16;
        r[..2].copy_from_slice(&len.to_be_bytes());
        r
    }

    #[test]
    fn printer_recordings_round_trip_and_replay_to_jobs() {
        use crate::printer::{Condition, flag, op};
        let path = std::env::temp_dir().join(format!("qsa-prec-{}.qsr", std::process::id()));
        let complete = Response::Complete.encode(op::PRINT);
        {
            let mut r = Recorder::create_printer(&path, "prt01", 37).unwrap();
            r.record(
                Direction::In,
                &print_record(flag::FIRST_OF_CHAIN, &[0xC1, 0xC2]),
            );
            r.record(Direction::Out, &complete);
            r.record(Direction::In, &print_record(flag::LAST_OF_CHAIN, &[0]));
            r.record(Direction::Out, &complete);
            // A second job, then a cleared one, then one left open.
            r.record(Direction::In, &print_record(0, &[0xC3]));
            r.record(
                Direction::Out,
                &Response::Intervention(Condition::NotReady).encode(op::PRINT),
            );
            r.record(Direction::In, &print_record(0, &[0]));
            r.record(Direction::Out, &complete);
            r.record(Direction::In, &print_record(0, &[0xC4]));
        }
        let rec = Recording::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(rec.printer.as_deref(), Some("prt01"));
        assert_eq!(rec.ccsid, 37);
        let out = replay_printer(&rec);
        assert_eq!(out.jobs, vec![vec![0xC1, 0xC2], vec![0xC3], vec![0xC4]]);
        // The client said not ready once: that is where it differs from
        // print complete.
        assert_eq!(out.divergences.len(), 1);
        assert_eq!(out.divergences[0].0, 5);
        // A display recording is not a printer's.
        let disp = Recording::parse(io::Cursor::new(
            "{\"v\":1,\"size\":\"24x80\",\"ccsid\":37}
",
        ))
        .unwrap();
        assert_eq!(disp.printer, None);
    }
    use crate::screen::{Ffw, Field, Keyboard};
    use crate::stream::{ESC, cmd};

    /// A sign-on-like screen: user field, then a nondisplay password field.
    fn signon_record() -> Record {
        let mut d = vec![ESC, cmd::CLEAR_UNIT, ESC, cmd::WRITE_TO_DISPLAY, 0, 0];
        d.extend_from_slice(&[
            order::SBA,
            6,
            52,
            order::SF,
            0x40,
            0x00,
            attr::GREEN_UL,
            0,
            10,
        ]);
        d.extend_from_slice(&[
            order::SBA,
            7,
            52,
            order::SF,
            0x40,
            0x00,
            attr::NONDISPLAY,
            0,
            10,
        ]);
        d.extend_from_slice(&[ESC, cmd::READ_MDT_FIELDS, 0, 0]);
        Record::new(3, d)
    }

    fn record_session(path: &Path) {
        let mut rec = Recorder::create(path, ScreenSize::S24x80, 37).unwrap();
        let mut t = Terminal::new(ScreenSize::S24x80, 37);
        let r = signon_record();
        rec.inbound(&r);
        t.receive(&r);
        for k in "QSECOFR"
            .chars()
            .map(Key::Char)
            .chain([Key::FieldExit])
            .chain("se cret".chars().map(Key::Char))
            .chain([Key::Enter])
        {
            let before = t.screen.clone();
            rec.key(&before, k);
            for out in t.key(k).send {
                rec.outbound(&t.screen, &out);
            }
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("qsa-replay-{}-{name}", std::process::id()))
    }

    #[test]
    fn passwords_never_reach_the_file() {
        let path = temp("scrub.qsr");
        record_session(&path);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();
        // No 's', 'e', 'c', 'r', 't' keys and no EBCDIC "se cret" bytes.
        for c in "secrt".chars() {
            assert!(
                !text.contains(&format!("Char:U+{:04X}", c as u32)),
                "{c} recorded"
            );
        }
        assert!(!text.contains("A28540839985A3"), "password bytes recorded");
        assert!(text.contains("Char:U+0020"), "blanks are kept");
        // The user ID is not a secret.
        assert!(text.contains("Char:U+0051"));
    }

    #[test]
    fn a_scrubbed_recording_replays_exactly() {
        let path = temp("replay.qsr");
        record_session(&path);
        let rec = Recording::read(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(rec.size, ScreenSize::S24x80);
        let r = replay(&rec);
        assert!(r.divergences.is_empty(), "{:?}", r.divergences);
        assert!(r.errors.is_empty());
        assert_eq!(r.terminal.screen.keyboard, Keyboard::System);
        let user: String = r.terminal.screen.text_rows()[5][52..62].to_owned();
        assert_eq!(user, "QSECOFR   ");
    }

    #[test]
    fn a_divergence_is_reported() {
        let mut rec = Recording {
            size: ScreenSize::S24x80,
            ccsid: 37,
            printer: None,
            events: vec![(0, Event::In(signon_record().encode()))],
        };
        rec.events.push((1, Event::Key(Key::Enter)));
        rec.events.push((2, Event::Out(vec![1, 2, 3])));
        let r = replay(&rec);
        assert_eq!(r.divergences.len(), 1);
    }

    #[test]
    fn key_names_round_trip() {
        for k in [
            Key::Char('é'),
            Key::F(13),
            Key::Enter,
            Key::FieldMinus,
            Key::SysReq,
            Key::Attention,
            Key::RecordBackspace,
        ] {
            assert_eq!(key_from_name(&key_name(k)), Some(k));
        }
        assert_eq!(key_from_name("F:25"), None);
        assert_eq!(key_from_name("Nope"), None);
    }

    #[test]
    fn golden_text_round_trips_and_diffs_mark_cells() {
        let mut t = Terminal::new(ScreenSize::S24x80, 37);
        t.receive(&signon_record());
        let g = Golden::of(&t.screen);
        assert_eq!(Golden::parse(&g.to_text()).as_ref(), Some(&g));
        assert_eq!(g.attrs[5].chars().nth(51), Some('#'));
        assert_eq!(g.attrs[5].chars().nth(52), Some('4'), "green underline");
        assert_eq!(g.keyboard, "Open read-mdt");
        assert!(Golden::diff(&g, &g).is_none());
        let mut other = g.clone();
        other.chars[0].replace_range(3..4, "Z");
        let d = Golden::diff(&g, &other).unwrap();
        assert!(d.contains("characters: want (left), got (right)"));
        let marks = d.lines().nth(2).unwrap();
        assert_eq!(marks.find('^'), Some(4 + 3));
        assert_eq!(marks.matches('^').count(), 2, "once per grid");
    }

    #[test]
    fn rif_replies_are_scrubbed_by_position() {
        let mut s = Screen::default_24x80();
        for (start, a) in [(100, attr::GREEN_UL), (200, attr::NONDISPLAY)] {
            s.define_field(Field {
                start,
                len: 3,
                ffw: Ffw(0x4800),
                fcw: vec![],
                attr: a,
            });
        }
        let mut r = Record::new(3, vec![1, 1, 0xF1, 0xC1, 0xC2, 0xC3, 0xC4, 0x40, 0xC6]);
        scrub_record(&s, &mut r);
        assert_eq!(&r.data[3..], &[0xC1, 0xC2, 0xC3, 0xE7, 0x40, 0xE7]);
    }

    #[test]
    fn malformed_recordings_are_refused() {
        let bad = |s: &str| Recording::parse(s.as_bytes()).is_err();
        assert!(bad(""));
        assert!(bad("{\"v\":2}"));
        assert!(bad("{\"v\":1,\"size\":\"10x10\",\"ccsid\":37}"));
        assert!(bad(
            "{\"v\":1,\"size\":\"24x80\",\"ccsid\":37}\n{\"t\":1,\"d\":\"in\",\"x\":\"ABC\"}"
        ));
        assert!(bad(
            "{\"v\":1,\"size\":\"24x80\",\"ccsid\":37}\n{\"t\":1,\"d\":\"up\",\"x\":\"AB\"}"
        ));
        assert!(!bad("{\"v\":1,\"size\":\"27x132\",\"ccsid\":1140}\n\n"));
    }
}
