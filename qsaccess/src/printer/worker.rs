// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The network side of a printer session, on its own thread.
//!
//! It connects, negotiates, and spools print data as it arrives: each print
//! record is answered only once its data is on disk. When a job ends it is
//! rendered here, and the record that ended it is answered after that, as
//! print complete, or as intervention required (printer not ready) when
//! the output could not be made. The GUI thread only shows what happened.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use tn5250::negotiate::{Mode, StartupResponse};
use tn5250::printer::{Condition, Event, JobEnd, Options, PrintEvent, PrinterSession, Response};
use tn5250::session::{SessionError, Target};
use tn5250::telnet::Security;

use super::spool::{self, Ended as Spooled, JobRecord, JobStatus, Spooler};
use crate::session::worker::Ended;

/// How long a job may go without data before it is closed, when the server
/// never marks where jobs end.
pub const IDLE_CLOSE: Duration = Duration::from_secs(10);

/// From the GUI to the worker.
#[derive(Debug)]
pub enum ToNet {
    /// New output settings.
    Output(spool::Output),
    /// Tell the server the printer is ready again.
    Ready,
    Close,
}

/// From the worker to the GUI.
#[derive(Debug)]
pub enum FromNet {
    Connected(Security),
    Ready(Mode),
    Startup(StartupResponse),
    /// A job's first data arrived.
    Receiving,
    /// A job ended; `idle` when the idle timeout ended it.
    Job {
        record: JobRecord,
        idle: bool,
    },
    /// The spool file could not be written.
    SpoolFailed(String),
    ProtocolError(String),
    Ended(Ended),
}

/// The GUI's ends of a printer worker.
#[derive(Debug)]
pub struct PrinterWorker {
    pub tx: Sender<ToNet>,
    pub rx: Receiver<FromNet>,
    thread: Option<JoinHandle<()>>,
}

impl PrinterWorker {
    /// Starts a worker connecting to `target`; with `record`, every record
    /// either way goes to that recording file.
    pub fn spawn(
        target: Target,
        options: Options,
        output: spool::Output,
        record: Option<std::path::PathBuf>,
    ) -> PrinterWorker {
        let (to_tx, to_rx) = mpsc::channel();
        let (from_tx, from_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name(format!("5250 printer {}", options.device_name))
            .spawn(move || run(&target, &options, output, record, &to_rx, &from_tx))
            .ok();
        PrinterWorker {
            tx: to_tx,
            rx: from_rx,
            thread,
        }
    }

    pub fn close(&mut self) {
        let _ = self.tx.send(ToNet::Close);
        self.thread.take();
    }
}

fn send(tx: &Sender<FromNet>, ev: FromNet) {
    if tx.send(ev).is_ok() {
        fltk::app::awake();
    }
}

fn run(
    target: &Target,
    options: &Options,
    output: spool::Output,
    record: Option<std::path::PathBuf>,
    rx: &Receiver<ToNet>,
    tx: &Sender<FromNet>,
) {
    let mut session = match PrinterSession::connect(target, options) {
        Ok(s) => s,
        Err(e) => {
            send(tx, FromNet::Ended(Ended::Connect(e)));
            return;
        }
    };
    send(tx, FromNet::Connected(session.security()));
    if let Some(path) = record {
        match tn5250::replay::Recorder::create_printer(&path, &options.device_name, options.ccsid) {
            Ok(mut r) => {
                crate::log!("recording this printer session to {}", path.display());
                session.set_recorder(Box::new(move |d, b| r.record(d, b)));
            }
            Err(e) => crate::log!("recording not started: {}: {e}", path.display()),
        }
    }
    let mut spooler = Spooler::new(output);
    let result = serve(&mut session, &mut spooler, rx, tx);
    session.close();
    // A job cut off by the connection ending is rendered with what came,
    // and marked incomplete.
    if let Some(Spooled::Rendered(record)) = spooler.cut_off() {
        send(
            tx,
            FromNet::Job {
                record,
                idle: false,
            },
        );
    }
    let ended = match result {
        Ok(()) => Ended::ByUser,
        Err(SessionError::Closed) => Ended::Closed,
        Err(SessionError::Io(e)) => Ended::Io(e.to_string()),
    };
    send(tx, FromNet::Ended(ended));
}

/// Runs the session until the user closes it (`Ok`) or it fails.
fn serve(
    session: &mut PrinterSession,
    spooler: &mut Spooler,
    rx: &Receiver<ToNet>,
    tx: &Sender<FromNet>,
) -> Result<(), SessionError> {
    loop {
        loop {
            match rx.try_recv() {
                Ok(ToNet::Output(o)) => spooler.set_output(o),
                Ok(ToNet::Ready) => session.signal(Response::Ready)?,
                Ok(ToNet::Close) | Err(TryRecvError::Disconnected) => return Ok(()),
                Err(TryRecvError::Empty) => break,
            }
        }
        for ev in session.poll(Duration::from_millis(50))? {
            match ev {
                Event::Ready(m) => send(tx, FromNet::Ready(m)),
                Event::Startup(s) => send(tx, FromNet::Startup(s)),
                Event::ProtocolError(e) => send(tx, FromNet::ProtocolError(e)),
                Event::Rejected(why) => send(
                    tx,
                    FromNet::Job {
                        record: spooler.rejected(session.host(), &why),
                        idle: false,
                    },
                ),
                Event::Print(events) => {
                    let ok = print_events(spooler, &events, tx);
                    session.respond(if ok {
                        Response::Complete
                    } else {
                        Response::Intervention(Condition::NotReady)
                    })?;
                }
            }
        }
        if let Some(PrintEvent::JobEnded { .. }) = session.idle(IDLE_CLOSE) {
            crate::log!("{}", qsa_msg::message(qsa_msg::id::JOB_IDLE_CLOSED, &[]));
            if let Some(ended) = spooler.end() {
                job_done(ended, true, tx);
            }
        }
    }
}

/// Spools and renders one print record's events. False when the data or
/// the output could not be kept: the server is told the printer is not
/// ready.
fn print_events(spooler: &mut Spooler, events: &[PrintEvent], tx: &Sender<FromNet>) -> bool {
    let mut ok = true;
    for e in events {
        match e {
            PrintEvent::JobStarted { transformed, .. } => {
                send(tx, FromNet::Receiving);
                if let Err(err) = spooler.begin(*transformed) {
                    ok = false;
                    send(tx, FromNet::SpoolFailed(err.to_string()));
                }
            }
            PrintEvent::Data { bytes, .. } => {
                if let Err(err) = spooler.data(bytes) {
                    ok = false;
                    send(tx, FromNet::SpoolFailed(err.to_string()));
                }
            }
            PrintEvent::JobEnded { end, .. } => {
                if let Some(ended) = spooler.end() {
                    ok &= job_done(ended, *end == JobEnd::Idle, tx);
                }
            }
            PrintEvent::Cleared { .. } => spooler.clear(),
        }
    }
    ok
}

/// Passes a finished job to the GUI; false if its output failed.
fn job_done(ended: Spooled, idle: bool, tx: &Sender<FromNet>) -> bool {
    let record = match ended {
        Spooled::Rendered(r) | Spooled::Transformed(r) => r,
    };
    let ok = record.status != JobStatus::Failed;
    send(tx, FromNet::Job { record, idle });
    ok
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Instant;

    use tn5250::negotiate::{IS, SEND, USERVAR, VAR};
    use tn5250::printer::{flag, flow, op};
    use tn5250::telnet::{
        BINARY, DO, EOR_OPTION, IAC, NEW_ENVIRON, SB, SE, TERMINAL_TYPE, WILL, codec,
    };

    use super::*;

    /// A print record from the server with `flags` and SCS `data`.
    fn print_record(flags: u16, data: &[u8]) -> Vec<u8> {
        let mut r = vec![0, 0, 0x12, 0xA0];
        r.extend_from_slice(&flow::FROM_SERVER.to_be_bytes());
        r.push(10);
        r.extend_from_slice(&flags.to_be_bytes());
        r.push(op::PRINT);
        r.extend_from_slice(&[0; 6]);
        r.extend_from_slice(data);
        let len = r.len() as u16;
        r[..2].copy_from_slice(&len.to_be_bytes());
        r
    }

    /// The record QS/400 sent before its writer was fixed (data flow 0000,
    /// a four-byte variable header, operation 00), with made-up SCS.
    fn unfixed_record(data: &[u8]) -> Vec<u8> {
        let mut r = vec![0, 0, 0x12, 0xA0, 0x00, 0x00, 4, 0x00, 0x00, 0x00];
        r.extend_from_slice(data);
        let len = r.len() as u16;
        r[..2].copy_from_slice(&len.to_be_bytes());
        r
    }

    /// A scripted server: negotiation, a start-up response, a record the
    /// client must refuse, then one job in two records and the null
    /// record; it reads one response for each.
    #[test]
    fn a_job_from_the_server_becomes_a_pdf() {
        let dir = std::env::temp_dir().join(format!("qsa-pworker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let mut b = scs::sample::Builder::new();
        b.text("HELLO FROM THE SYSTEM").nl().text("SECOND LINE");
        let job = b.bytes();
        let (first, second) = job.split_at(job.len() / 2);
        let (first, second) = (first.to_vec(), second.to_vec());
        let server = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 4096];
            let mut got = Vec::new();
            let mut wait_for = |s: &mut std::net::TcpStream, got: &mut Vec<u8>, want: &[u8]| {
                let end = Instant::now() + Duration::from_secs(10);
                while !got.windows(want.len()).any(|w| w == want) {
                    assert!(Instant::now() < end, "timed out waiting for {want:02X?}");
                    let n = s.read(&mut buf).unwrap();
                    assert!(n > 0);
                    got.extend_from_slice(&buf[..n]);
                }
                got.clear();
            };
            s.write_all(&[IAC, DO, NEW_ENVIRON, IAC, DO, TERMINAL_TYPE])
                .unwrap();
            s.write_all(&[IAC, SB, NEW_ENVIRON, SEND, VAR, USERVAR, IAC, SE])
                .unwrap();
            wait_for(&mut s, &mut got, b"QSAPRT01");
            s.write_all(&[IAC, SB, TERMINAL_TYPE, SEND, IAC, SE])
                .unwrap();
            wait_for(&mut s, &mut got, &[IAC, SB, TERMINAL_TYPE, IS]);
            s.write_all(&[
                IAC, DO, EOR_OPTION, IAC, WILL, EOR_OPTION, IAC, DO, BINARY, IAC, WILL, BINARY,
            ])
            .unwrap();
            let page = tn5250::codepage::default_page();
            let mut startup = vec![
                0, 0, 0x12, 0xA0, 0x90, 0x00, 5, 0x60, 0x06, 0x00, 0x20, 0xC0, 0x00, 0x3D, 0x00,
                0x00,
            ];
            startup.extend(page.encode("I902QS400   QSAPRT01  "));
            let len = startup.len() as u16;
            startup[..2].copy_from_slice(&len.to_be_bytes());
            s.write_all(&codec::encode_record(&startup)).unwrap();
            // Refused with RFC 4777's invalid print command, operation 01.
            s.write_all(&codec::encode_record(&unfixed_record(&first)))
                .unwrap();
            wait_for(
                &mut s,
                &mut got,
                &codec::encode_record(&[
                    0x00, 0x0E, 0x12, 0xA0, 0x01, 0x02, 0x08, 0x80, 0x00, 0x01, 0x08, 0x11, 0x02,
                    0x28,
                ]),
            );
            let complete = codec::encode_record(&Response::Complete.encode(op::PRINT));
            for rec in [
                print_record(flag::FIRST_OF_CHAIN, &first),
                print_record(0, &second),
                print_record(flag::LAST_OF_CHAIN, &[0]),
            ] {
                s.write_all(&codec::encode_record(&rec)).unwrap();
                wait_for(&mut s, &mut got, &complete);
            }
            std::thread::sleep(Duration::from_millis(300));
        });

        let output = spool::Output {
            device: "QSAPRT01".into(),
            target: qsa_config::qsp::Target::Pdf,
            pdf_folder: Some(dir.join("out")),
            file_name: qsa_config::qsp::DEFAULT_FILE_NAME.into(),
            also_save_text: true,
            system_printer: String::new(),
            paper: scs::Paper::Letter,
            margin: None,
            ccsid: 37,
            font: 11,
            spool_dir: dir.join("spool"),
        };
        let mut w = PrinterWorker::spawn(
            Target {
                host: "127.0.0.1".into(),
                port,
                tls: None,
            },
            Options::new("qsaprt01", 37),
            output,
            Some(dir.join("session.qsr")),
        );
        let end = Instant::now() + Duration::from_secs(20);
        let mut events = Vec::new();
        loop {
            assert!(Instant::now() < end, "worker events so far: {events:?}");
            match w.rx.recv_timeout(Duration::from_millis(200)) {
                Ok(FromNet::Ended(e)) => {
                    events.push(format!("ended {e:?}"));
                    break;
                }
                Ok(ev) => events.push(format!("{ev:?}")),
                Err(_) => {}
            }
        }
        server.join().unwrap();
        w.close();
        assert!(events.iter().any(|e| e.contains("I902")), "{events:?}");
        assert!(events.iter().any(|e| e == "Receiving"), "{events:?}");
        let rejected = events
            .iter()
            .find(|e| e.starts_with("Job") && e.contains("Rejected"))
            .expect("an error row for the refused record");
        assert!(
            rejected.contains("status: Failed") && rejected.contains("operation 00"),
            "{rejected}"
        );
        let job = events
            .iter()
            .find(|e| e.starts_with("Job") && !e.contains("Rejected"))
            .expect("a job");
        assert!(
            job.contains("status: Done") && job.contains("pages: 1"),
            "{job}"
        );
        let pdfs: Vec<_> = std::fs::read_dir(dir.join("out"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        let pdf = pdfs
            .iter()
            .find(|p| p.extension().is_some_and(|e| e == "pdf"))
            .unwrap();
        assert!(std::fs::read(pdf).unwrap().starts_with(b"%PDF-"));
        // The recording replays to the same job.
        let recording = tn5250::replay::Recording::read(&dir.join("session.qsr")).unwrap();
        assert_eq!(recording.printer.as_deref(), Some("QSAPRT01"));
        let replayed = tn5250::replay::replay_printer(&recording);
        assert!(
            replayed.divergences.is_empty(),
            "{:?}",
            replayed.divergences
        );
        assert_eq!(replayed.jobs.len(), 1);
        assert_eq!(replayed.rejected, 1);
        let txt = pdf.with_extension("txt");
        assert_eq!(
            std::fs::read_to_string(txt).unwrap(),
            "HELLO FROM THE SYSTEM\nSECOND LINE\n"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
