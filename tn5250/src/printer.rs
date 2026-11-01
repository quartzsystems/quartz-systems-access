// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! TN5250E printer sessions (RFC 4777 sections 8 to 12).
//!
//! - [`Options`]: the printer's device name, message queue, font, forms and
//!   paper, sent as `NEW-ENVIRON` variables with a printer terminal type.
//!   Host print transform is always off.
//! - [`Printer`]: print records in, job events out, without I/O. Every
//!   print record needs one [`Response`].
//! - [`PrinterSession`]: a [`Session`] driven through a [`Printer`], for one
//!   worker thread. It shares the Telnet layer and the negotiator with the
//!   display session.
//!
//! A print record (section 11) has the 5250 record header with data flow
//! `0101` (from the server) in the reserved field, a variable header of ten
//! bytes (flags, the printer operation, zero padding) and SCS data. The
//! server ends each job with a null print record (section 11.3). Responses
//! use data flow `0102`; an error response carries a diagnostic in a longer
//! variable header.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::debug;
use crate::negotiate::{self, Mode, Request, StartupResponse};
use crate::record::{RECORD_TYPE, Record};
use crate::session::{Recorder, Session, SessionError, Target};
use crate::telnet::{ConnectError, Security, Transport};
use crate::wire;

/// The data flow field (record bytes 4 and 5).
pub mod flow {
    /// A record from the server.
    pub const FROM_SERVER: u16 = 0x0101;
    /// A response from the client.
    pub const TO_SERVER: u16 = 0x0102;
}

/// Flags (record bytes 7 and 8).
pub mod flag {
    /// Error indicator: a negative response follows.
    pub const ERROR: u16 = 0x8000;
    /// Intervention required: a printer signal follows.
    pub const INTERVENTION: u16 = 0x4000;
    /// The printer is ready again.
    pub const READY: u16 = 0x2000;
    /// First record of a chain.
    pub const FIRST_OF_CHAIN: u16 = 0x1000;
    /// Last record of a chain.
    pub const LAST_OF_CHAIN: u16 = 0x0800;
}

/// Printer operation codes (record byte 9).
pub mod op {
    /// Print, or print complete in a response.
    pub const PRINT: u8 = 0x01;
    /// Clear print buffers.
    pub const CLEAR: u8 = 0x02;
}

/// SCS ASCII transparency. Host print transform wraps its ASCII output in
/// it; a job that starts with it was transformed.
const SCS_ASCII_TRANSPARENT: u8 = 0x03;

/// The form feed variable's values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormFeed {
    #[default]
    Continuous,
    Cut,
    Autocut,
}

impl FormFeed {
    fn value(self) -> &'static [u8] {
        match self {
            FormFeed::Continuous => b"C",
            FormFeed::Cut => b"U",
            FormFeed::Autocut => b"A",
        }
    }
}

/// Paper in a paper source (section 8's index values).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Paper {
    /// No paper source.
    #[default]
    None,
    Letter,
    Legal,
    Executive,
    A4,
    A5,
    B5,
    A3,
    B4,
    Ledger,
}

impl Paper {
    /// The one-byte value sent for the paper.
    pub fn code(self) -> u8 {
        match self {
            Paper::None => 0xFF,
            Paper::Letter => 0x01,
            Paper::Legal => 0x02,
            Paper::Executive => 0x03,
            Paper::A4 => 0x04,
            Paper::A5 => 0x05,
            Paper::B5 => 0x06,
            Paper::A3 => 0x0E,
            Paper::B4 => 0x0F,
            Paper::Ledger => 0x10,
        }
    }
}

/// The message queue that gets printer messages when none is named.
pub const DEFAULT_MESSAGE_QUEUE: &str = "QSYSOPR";
/// Its library.
pub const DEFAULT_MESSAGE_QUEUE_LIBRARY: &str = "QSYS";
/// The font asked for when none is named: a 10-pitch monospace face.
pub const DEFAULT_FONT: u16 = 11;

/// What a printer session asks the server for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The printer device, 1 to 10 characters; required.
    pub device_name: String,
    pub message_queue: String,
    pub message_queue_library: String,
    /// The font ID the device is created with.
    pub font: u16,
    pub form_feed: FormFeed,
    pub paper_source_1: Paper,
    pub paper_source_2: Paper,
    pub ccsid: u16,
}

impl Options {
    /// A printer named `device_name` with the defaults: messages to
    /// `QSYSOPR` in `QSYS`, font 11, continuous forms, letter paper.
    pub fn new(device_name: &str, ccsid: u16) -> Options {
        Options {
            device_name: device_name.trim().to_ascii_uppercase(),
            message_queue: DEFAULT_MESSAGE_QUEUE.to_owned(),
            message_queue_library: DEFAULT_MESSAGE_QUEUE_LIBRARY.to_owned(),
            font: DEFAULT_FONT,
            form_feed: FormFeed::Continuous,
            paper_source_1: Paper::Letter,
            paper_source_2: Paper::None,
            ccsid,
        }
    }

    /// The negotiation request: the single-byte printer terminal type, the
    /// device name, and the printer variables of section 8. Host print
    /// transform is never asked for.
    pub fn request(&self) -> Request {
        let upper = |s: &str, default: &str| {
            let s = s.trim();
            if s.is_empty() { default } else { s }
                .to_ascii_uppercase()
                .into_bytes()
        };
        let extra = vec![
            (
                wire::MESSAGE_QUEUE_NAME.to_owned(),
                upper(&self.message_queue, DEFAULT_MESSAGE_QUEUE),
            ),
            (
                wire::MESSAGE_QUEUE_LIBRARY.to_owned(),
                upper(&self.message_queue_library, DEFAULT_MESSAGE_QUEUE_LIBRARY),
            ),
            (wire::FONT.to_owned(), self.font.to_string().into_bytes()),
            (wire::FORM_FEED.to_owned(), self.form_feed.value().to_vec()),
            (wire::TRANSFORM.to_owned(), b"0".to_vec()),
            (
                wire::PAPER_SOURCE_1.to_owned(),
                vec![self.paper_source_1.code()],
            ),
            (
                wire::PAPER_SOURCE_2.to_owned(),
                vec![self.paper_source_2.code()],
            ),
            (wire::ENVELOPE.to_owned(), vec![Paper::None.code()]),
            (wire::ASCII_899.to_owned(), b"0".to_vec()),
        ];
        Request {
            terminal_type: wire::TT_PRINTER_SBCS.to_owned(),
            device_name: Some(self.device_name.trim().to_ascii_uppercase())
                .filter(|d| !d.is_empty()),
            ccsid: self.ccsid,
            extra,
        }
    }
}

/// A printer condition reported with intervention required (section 11,
/// note 2: the printer's signal codes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    /// The printer is not ready: the output target cannot take data now.
    NotReady,
    EndOfForms,
    PaperJam,
    PrintCheck,
    MachineCheck,
    /// Data stream error, class 1 (loss of text) to 4.
    DataStream(u8),
}

impl Condition {
    fn signal(self) -> u8 {
        match self {
            Condition::NotReady => 0x51,
            Condition::EndOfForms => 0x50,
            Condition::PaperJam => 0x47,
            Condition::PrintCheck => 0x31,
            Condition::MachineCheck => 0x11,
            Condition::DataStream(class) => 0x65 + class.clamp(1, 4),
        }
    }
}

/// A negative response (section 11, note 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Negative {
    Cancel,
    InvalidParameter,
    InvalidCommand,
}

/// The client's answer to a print record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    /// Print complete (section 11.2).
    Complete,
    /// The printer is ready again after an intervention.
    Ready,
    /// Intervention required, with the condition.
    Intervention(Condition),
    /// A negative response.
    Negative(Negative),
}

impl Response {
    /// The flags and diagnostic for this response.
    fn parts(self) -> (u16, Vec<u8>) {
        match self {
            Response::Complete => (0, Vec::new()),
            Response::Ready => (flag::READY, vec![0xC9, 0x00, 0x00, 0x00, 0x02]),
            Response::Intervention(c) => {
                (flag::INTERVENTION, vec![0xC9, 0x00, 0x03, 0x02, c.signal()])
            }
            Response::Negative(n) => (
                flag::ERROR,
                vec![
                    0x08,
                    0x11,
                    0x02,
                    match n {
                        Negative::Cancel => 0x00,
                        Negative::InvalidParameter => 0x29,
                        Negative::InvalidCommand => 0x28,
                    },
                ],
            ),
        }
    }

    /// The response record answering a record with printer operation
    /// `opcode`, before Telnet framing. The diagnostic sits in the
    /// variable header, whose length counts it.
    pub fn encode(self, opcode: u8) -> Vec<u8> {
        let (flags, diag) = self.parts();
        let var = 4 + diag.len();
        let len = 6 + var;
        let mut out = Vec::with_capacity(len);
        out.extend_from_slice(&(len as u16).to_be_bytes());
        out.extend_from_slice(&RECORD_TYPE.to_be_bytes());
        out.extend_from_slice(&flow::TO_SERVER.to_be_bytes());
        out.push(var as u8);
        out.extend_from_slice(&flags.to_be_bytes());
        out.push(opcode);
        out.extend_from_slice(&diag);
        out
    }
}

/// Why a job ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobEnd {
    /// The server's null print record.
    NullRecord,
    /// No job boundaries from this server, and no data for a while.
    Idle,
}

/// What print records mean for the output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrintEvent {
    /// A job's first data arrived. `transformed`: the data is host print
    /// transform output, which this client never asks for and does not
    /// print.
    JobStarted { job: u64, transformed: bool },
    /// SCS data for the job.
    Data { job: u64, bytes: Vec<u8> },
    /// The job is complete.
    JobEnded { job: u64, end: JobEnd },
    /// The server cleared the print buffers: the job so far is dropped.
    Cleared { job: Option<u64> },
}

/// How a record is to be answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Handled {
    /// The caller deals with the events, then answers with
    /// [`PrinterSession::respond`] (or [`Response::encode`]) using `opcode`.
    Caller { events: Vec<PrintEvent>, opcode: u8 },
    /// The protocol answers by itself.
    Now { response: Response, opcode: u8 },
}

#[derive(Clone, Debug)]
struct Job {
    id: u64,
    last_data: Instant,
}

/// The printer's side of the steady state, without I/O.
#[derive(Debug, Default)]
pub struct Printer {
    job: Option<Job>,
    jobs: u64,
    /// The server has ended a job with a null print record, so it gives job
    /// boundaries and idle time never ends a job.
    boundaries: bool,
}

/// Whether print data is a null print record's: none, or one null byte.
fn is_null(data: &[u8]) -> bool {
    matches!(data, [] | [0x00])
}

impl Printer {
    pub fn new() -> Printer {
        Printer::default()
    }

    /// The job open now, if any.
    pub fn current_job(&self) -> Option<u64> {
        self.job.as_ref().map(|j| j.id)
    }

    /// A record from the server, at `now`.
    pub fn record(&mut self, rec: &Record, now: Instant) -> Handled {
        if rec.reserved != flow::FROM_SERVER {
            debug!(
                "printer: record with data flow {:04X} (not {:04X}), flags {:04X}, operation {:02X}",
                rec.reserved,
                flow::FROM_SERVER,
                rec.flags,
                rec.opcode
            );
        }
        match rec.opcode {
            op::PRINT => {
                let mut events = Vec::new();
                if is_null(&rec.data) {
                    self.boundaries = true;
                    match self.job.take() {
                        Some(j) => {
                            debug!("printer: null print record; job {} complete", j.id);
                            events.push(PrintEvent::JobEnded {
                                job: j.id,
                                end: JobEnd::NullRecord,
                            });
                        }
                        None => debug!("printer: null print record with no job open"),
                    }
                } else {
                    let job = match &mut self.job {
                        Some(j) => {
                            j.last_data = now;
                            j.id
                        }
                        None => {
                            self.jobs += 1;
                            let id = self.jobs;
                            let transformed = rec.data[0] == SCS_ASCII_TRANSPARENT;
                            debug!(
                                "printer: job {id} started{}",
                                if transformed {
                                    " (host print transform output)"
                                } else {
                                    ""
                                }
                            );
                            self.job = Some(Job { id, last_data: now });
                            events.push(PrintEvent::JobStarted {
                                job: id,
                                transformed,
                            });
                            id
                        }
                    };
                    events.push(PrintEvent::Data {
                        job,
                        bytes: rec.data.clone(),
                    });
                }
                Handled::Caller {
                    events,
                    opcode: op::PRINT,
                }
            }
            op::CLEAR => {
                let job = self.job.take().map(|j| j.id);
                debug!(
                    "printer: clear print buffers{}",
                    job.map(|j| format!("; job {j} dropped"))
                        .unwrap_or_default()
                );
                Handled::Caller {
                    events: vec![PrintEvent::Cleared { job }],
                    opcode: op::CLEAR,
                }
            }
            other => {
                debug!("printer: operation {other:02X} is not a printer operation; rejected");
                Handled::Now {
                    response: Response::Negative(Negative::InvalidCommand),
                    opcode: other,
                }
            }
        }
    }

    /// Ends the open job if it has had no data for `timeout` and the server
    /// has never sent a job boundary.
    pub fn idle(&mut self, now: Instant, timeout: Duration) -> Option<PrintEvent> {
        if self.boundaries {
            return None;
        }
        let j = self.job.as_ref()?;
        if now.saturating_duration_since(j.last_data) < timeout {
            return None;
        }
        let id = j.id;
        self.job = None;
        debug!(
            "printer: job {id} closed after {} s without data (the server sends no job boundaries)",
            timeout.as_secs()
        );
        Some(PrintEvent::JobEnded {
            job: id,
            end: JobEnd::Idle,
        })
    }
}

/// What a printer session passes up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Records can flow; the mode is decided.
    Ready(Mode),
    /// The start-up response.
    Startup(StartupResponse),
    /// A print record's events. Answer each `Print` with
    /// [`PrinterSession::respond`], in order, once its events are dealt
    /// with (the data safely stored, say).
    Print(Vec<PrintEvent>),
    /// Something in the stream was wrong; the session goes on.
    ProtocolError(String),
}

/// A connected printer session.
#[derive(Debug)]
pub struct PrinterSession {
    session: Session,
    printer: Printer,
    /// Operations of records waiting for the caller's response, oldest
    /// first.
    unanswered: VecDeque<u8>,
}

impl PrinterSession {
    /// Connects (and runs the TLS handshake).
    pub fn connect(target: &Target, options: &Options) -> Result<PrinterSession, ConnectError> {
        Ok(PrinterSession::from_session(Session::connect(
            target,
            options.request(),
        )?))
    }

    /// A printer session over a transport that is already connected.
    pub fn over(transport: Box<dyn Transport>, host: &str, options: &Options) -> PrinterSession {
        PrinterSession::from_session(Session::over(transport, host, options.request()))
    }

    fn from_session(session: Session) -> PrinterSession {
        PrinterSession {
            session,
            printer: Printer::new(),
            unanswered: VecDeque::new(),
        }
    }

    /// Records every record from now on.
    pub fn set_recorder(&mut self, recorder: Recorder) {
        self.session.set_recorder(recorder);
    }

    pub fn host(&self) -> &str {
        self.session.host()
    }

    pub fn security(&self) -> Security {
        self.session.security()
    }

    pub fn mode(&self) -> Option<Mode> {
        self.session.mode()
    }

    /// The job open now, if any.
    pub fn current_job(&self) -> Option<u64> {
        self.printer.current_job()
    }

    /// Waits up to `wait` for bytes; returns what they completed.
    pub fn poll(&mut self, wait: Duration) -> Result<Vec<Event>, SessionError> {
        let mut out = Vec::new();
        for ev in self.session.poll(wait)? {
            match ev {
                negotiate::Event::Ready(m) => out.push(Event::Ready(m)),
                negotiate::Event::Startup(s) => out.push(Event::Startup(s)),
                negotiate::Event::ProtocolError(e) => out.push(Event::ProtocolError(e)),
                negotiate::Event::Record(rec) => match self.printer.record(&rec, Instant::now()) {
                    Handled::Caller { events, opcode } => {
                        self.unanswered.push_back(opcode);
                        out.push(Event::Print(events));
                    }
                    Handled::Now { response, opcode } => {
                        self.session.send_bytes(&response.encode(opcode))?;
                    }
                },
            }
        }
        Ok(out)
    }

    /// Answers the oldest print record not yet answered.
    pub fn respond(&mut self, response: Response) -> Result<(), SessionError> {
        let Some(opcode) = self.unanswered.pop_front() else {
            debug!("printer: response {response:?} with no record to answer; not sent");
            return Ok(());
        };
        debug!("printer: send {response:?} for operation {opcode:02X}");
        self.session.send_bytes(&response.encode(opcode))
    }

    /// Sends a response that answers no record: [`Response::Ready`] after
    /// the output target recovers.
    pub fn signal(&mut self, response: Response) -> Result<(), SessionError> {
        debug!("printer: send {response:?}");
        self.session.send_bytes(&response.encode(op::PRINT))
    }

    /// Ends the open job if it has idled for `timeout` (see
    /// [`Printer::idle`]).
    pub fn idle(&mut self, timeout: Duration) -> Option<PrintEvent> {
        self.printer.idle(Instant::now(), timeout)
    }

    /// Closes the connection.
    pub fn close(&mut self) {
        self.session.close();
    }
}

#[cfg(test)]
mod tests;
