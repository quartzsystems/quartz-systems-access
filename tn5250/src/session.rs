// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A connected 5250 session: a transport driven through the
//! [`Negotiator`]. Blocking, for one worker thread: [`Session::poll`] waits
//! a short while for bytes, so the same thread can send between polls.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use crate::negotiate::{Event, Mode, Negotiator, Request};
use crate::record::Record;
use crate::telnet::tls::{Tls, TlsOptions};
use crate::telnet::transport::{Tcp, is_timeout};
use crate::telnet::{CONNECT_TIMEOUT, ConnectError, KEEPALIVE, Security, Transport};

/// Where to connect.
#[derive(Clone, Debug)]
pub struct Target {
    pub host: String,
    pub port: u16,
    /// TLS, with what to trust; `None` for plain TCP.
    pub tls: Option<TlsOptions>,
}

/// Which way bytes went, for recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// From the server.
    In,
    /// To the server.
    Out,
}

/// Called with every record that goes either way, before Telnet framing.
pub type Recorder = Box<dyn FnMut(Direction, &[u8]) + Send>;

/// How a session stopped.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The server closed the connection.
    #[error("session ended by the system")]
    Closed,
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// One session.
pub struct Session {
    transport: Box<dyn Transport>,
    neg: Negotiator,
    last_write: Instant,
    recorder: Option<Recorder>,
    host: String,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("host", &self.host).finish()
    }
}

impl Session {
    /// Connects (and runs the TLS handshake) within [`CONNECT_TIMEOUT`].
    pub fn connect(target: &Target, request: Request) -> Result<Session, ConnectError> {
        crate::debug!(
            "session: connecting to {} port {}{}",
            target.host,
            target.port,
            if target.tls.is_some() {
                " with TLS"
            } else {
                ""
            }
        );
        let tcp = Tcp::connect(&target.host, target.port, CONNECT_TIMEOUT)?;
        let transport: Box<dyn Transport> = match &target.tls {
            Some(opts) => Box::new(Tls::handshake(tcp, &target.host, opts)?),
            None => Box::new(tcp),
        };
        Ok(Session::over(transport, &target.host, request))
    }

    /// A session over a transport that is already connected.
    pub fn over(transport: Box<dyn Transport>, host: &str, request: Request) -> Session {
        Session {
            transport,
            neg: Negotiator::new(request),
            last_write: Instant::now(),
            recorder: None,
            host: host.to_owned(),
        }
    }

    /// Records every record from now on.
    pub fn set_recorder(&mut self, recorder: Recorder) {
        self.recorder = Some(recorder);
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn security(&self) -> Security {
        self.transport.security()
    }

    pub fn mode(&self) -> Option<Mode> {
        self.neg.mode()
    }

    /// Waits up to `wait` for bytes; returns what they completed. Sends a
    /// keepalive when the session has been quiet for [`KEEPALIVE`].
    pub fn poll(&mut self, wait: Duration) -> Result<Vec<Event>, SessionError> {
        if self.last_write.elapsed() >= KEEPALIVE {
            self.neg.keepalive();
            self.flush()?;
        }
        self.transport
            .set_read_timeout(Some(wait.max(Duration::from_millis(1))))?;
        let mut buf = [0u8; 16 * 1024];
        let n = match self.transport.read(&mut buf) {
            Ok(0) => return Err(SessionError::Closed),
            Ok(n) => n,
            Err(e) if is_timeout(&e) => return Ok(Vec::new()),
            Err(e) if is_closed(&e) => return Err(SessionError::Closed),
            Err(e) => return Err(e.into()),
        };
        let events = self.neg.receive(&buf[..n]);
        if let Some(rec) = self.recorder.as_mut() {
            for ev in &events {
                if let Event::Record(r) = ev {
                    rec(Direction::In, &r.encode());
                }
            }
        }
        // Negotiation answers go straight back.
        self.flush()?;
        Ok(events)
    }

    /// Sends a record.
    pub fn send(&mut self, record: &Record) -> Result<(), SessionError> {
        let bytes = record.encode();
        if let Some(rec) = self.recorder.as_mut() {
            rec(Direction::Out, &bytes);
        }
        self.neg.send(record);
        self.flush()
    }

    /// Closes the connection.
    pub fn close(&mut self) {
        crate::debug!("session: closing connection to {}", self.host);
        self.transport.shutdown();
    }

    fn flush(&mut self) -> Result<(), SessionError> {
        let out = self.neg.take_output();
        if out.is_empty() {
            return Ok(());
        }
        self.transport.write_all(&out).map_err(|e| {
            if is_closed(&e) {
                SessionError::Closed
            } else {
                SessionError::Io(e)
            }
        })?;
        self.transport.flush()?;
        self.last_write = Instant::now();
        Ok(())
    }
}

fn is_closed(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::negotiate::ScreenSize;
    use crate::telnet::{BINARY, DO, EOR_OPTION, IAC, NEW_ENVIRON, TERMINAL_TYPE, WILL};
    use std::net::TcpListener;

    /// A scripted server: sends the opening, reads the answers, sends one
    /// record, then closes.
    #[test]
    fn negotiates_and_receives_a_record_over_tcp() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            s.write_all(&[IAC, DO, NEW_ENVIRON, IAC, DO, TERMINAL_TYPE])
                .unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 256];
            while got.len() < 6 {
                let n = s.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
            }
            assert_eq!(got, [IAC, WILL, NEW_ENVIRON, IAC, WILL, TERMINAL_TYPE]);
            s.write_all(&[
                IAC, DO, EOR_OPTION, IAC, WILL, EOR_OPTION, IAC, DO, BINARY, IAC, WILL, BINARY,
            ])
            .unwrap();
            let rec = Record::new(3, vec![0x04, 0x40]).encode();
            s.write_all(&crate::telnet::codec::encode_record(&rec))
                .unwrap();
            std::thread::sleep(Duration::from_millis(100));
        });
        let target = Target {
            host: "127.0.0.1".into(),
            port,
            tls: None,
        };
        let mut sess =
            Session::connect(&target, Request::display(ScreenSize::S24x80, None, 37)).unwrap();
        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let r2 = recorded.clone();
        sess.set_recorder(Box::new(move |d, b| {
            r2.lock().unwrap().push((d, b.to_vec()))
        }));
        let mut events = Vec::new();
        let end = loop {
            match sess.poll(Duration::from_millis(50)) {
                Ok(ev) => events.extend(ev),
                Err(e) => break e,
            }
        };
        assert!(matches!(end, SessionError::Closed));
        // Without an environment SEND the mode is basic.
        assert_eq!(events[0], Event::Ready(Mode::Basic));
        assert!(matches!(&events[1], Event::Record(r) if r.data == [0x04, 0x40]));
        assert_eq!(recorded.lock().unwrap()[0].0, Direction::In);
        server.join().unwrap();
    }
}
