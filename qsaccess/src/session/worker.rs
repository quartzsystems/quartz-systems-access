// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The network side of a session, on its own thread. It connects, runs the
//! negotiation and moves records; the screen lives on the GUI thread. Each
//! event it sends is followed by `fltk::app::awake`, so the GUI thread wakes
//! and picks it up.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use tn5250::negotiate::{Event, Mode, Request, StartupResponse};
use tn5250::record::Record;
use tn5250::session::{Session, SessionError, Target};
use tn5250::telnet::{ConnectError, Security};

/// From the GUI to the worker.
#[derive(Debug)]
pub enum ToNet {
    Send(Record),
    Close,
}

/// Why a session ended.
#[derive(Debug)]
pub enum Ended {
    /// The connection could not be opened.
    Connect(ConnectError),
    /// The system closed it.
    Closed,
    /// It failed after opening.
    Io(String),
    /// The user disconnected.
    ByUser,
}

/// From the worker to the GUI.
#[derive(Debug)]
pub enum FromNet {
    Connected(Security),
    Ready(Mode),
    Startup(StartupResponse),
    Record(Record),
    ProtocolError(String),
    Ended(Ended),
}

/// The GUI's ends of a worker.
#[derive(Debug)]
pub struct Worker {
    pub tx: Sender<ToNet>,
    pub rx: Receiver<FromNet>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    /// Starts a worker connecting to `target`.
    pub fn spawn(target: Target, request: Request) -> Worker {
        let (to_tx, to_rx) = mpsc::channel();
        let (from_tx, from_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name(format!("5250 {}", target.host))
            .spawn(move || run(&target, request, &to_rx, &from_tx))
            .ok();
        Worker {
            tx: to_tx,
            rx: from_rx,
            thread,
        }
    }

    /// Asks the worker to close and lets it go.
    pub fn close(&mut self) {
        let _ = self.tx.send(ToNet::Close);
        // The thread ends on its own within one poll; it is not joined, so
        // the GUI never waits on the network.
        self.thread.take();
    }
}

fn send(tx: &Sender<FromNet>, ev: FromNet) {
    if tx.send(ev).is_ok() {
        fltk::app::awake();
    }
}

fn run(target: &Target, request: Request, rx: &Receiver<ToNet>, tx: &Sender<FromNet>) {
    let mut session = match Session::connect(target, request) {
        Ok(s) => s,
        Err(e) => {
            send(tx, FromNet::Ended(Ended::Connect(e)));
            return;
        }
    };
    send(tx, FromNet::Connected(session.security()));
    loop {
        loop {
            match rx.try_recv() {
                Ok(ToNet::Send(rec)) => {
                    if let Err(e) = session.send(&rec) {
                        finish(&mut session, tx, e);
                        return;
                    }
                }
                Ok(ToNet::Close) | Err(TryRecvError::Disconnected) => {
                    session.close();
                    send(tx, FromNet::Ended(Ended::ByUser));
                    return;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        match session.poll(Duration::from_millis(20)) {
            Ok(events) => {
                for ev in events {
                    send(
                        tx,
                        match ev {
                            Event::Ready(m) => FromNet::Ready(m),
                            Event::Startup(s) => FromNet::Startup(s),
                            Event::Record(r) => FromNet::Record(r),
                            Event::ProtocolError(e) => FromNet::ProtocolError(e),
                        },
                    );
                }
            }
            Err(e) => {
                finish(&mut session, tx, e);
                return;
            }
        }
    }
}

fn finish(session: &mut Session, tx: &Sender<FromNet>, e: SessionError) {
    session.close();
    let ended = match e {
        SessionError::Closed => Ended::Closed,
        SessionError::Io(e) => Ended::Io(e.to_string()),
    };
    send(tx, FromNet::Ended(ended));
}
