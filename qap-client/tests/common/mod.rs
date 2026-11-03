// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QS/400's QAP connection engine (`qapd`, pinned with the protocol crates)
//! in this process, over loopback TLS 1.3, with QS/400's own test system:
//! `qapd::testing`'s [`System`] (profiles `BOB`, `WARN` and `ANN`, sessions,
//! Telnet tickets, the test commands, output queues and spooled files, the
//! SQL fixtures, a clock) and a `MemHost` per connection.
//!
//! QS/400 Access speaks only TLS 1.3 and opens many connections (one per
//! window, and new ones to resume), so the listener here takes any number
//! of them over TLS. For the current engine each one is relayed, in the
//! clear, to a connection of `qapd::testing::serve_loopback_as`: the engine
//! then reads with QS/400's own [`qapd::Polled`] transport (no socket read
//! timeout, which on Windows can fail with os error 997), and
//! `System::drop_connection` and `drop_all` close it mid-request as a
//! network drop would. [`listen`] is generic over the engine, so [`v1`] and
//! [`v2`] put the version-1 and version-2 engines behind it, over `Polled`
//! as well.

#![allow(dead_code)]

pub mod v1;
pub mod v2;

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use qap_client::proto::Op;
use qap_client::proto::ops::signon::{SignOn, SignOnReply};
use qap_client::{Connection, Secret, Target};
use qapd::testing::System;
use qapd::{Closed, Config, Polled};
use rustls::pki_types::PrivateKeyDer;
use rustls::pki_types::pem::PemObject;

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../qsa-net/tests/data/");

fn file(name: &str) -> String {
    std::fs::read_to_string(format!("{DATA}{name}")).unwrap()
}

/// The server's end of a TLS connection, over a non-blocking socket.
pub struct TlsConn {
    pub tls: rustls::StreamOwned<rustls::ServerConnection, Polled>,
    /// The socket, to shut down.
    raw: TcpStream,
}

impl TlsConn {
    /// Sends `close_notify` and closes the socket.
    pub fn close_tls(&mut self) {
        self.tls.conn.send_close_notify();
        let _ = self.tls.conn.complete_io(&mut self.tls.sock);
        let _ = self.raw.shutdown(Shutdown::Both);
    }
}

impl Read for TlsConn {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.tls.read(b)
    }
}

impl Write for TlsConn {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.tls.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.tls.flush()
    }
}

/// TLS 1.3 alone, as QS/400's server offers.
pub const TLS13: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];

/// A TLS listener on loopback offering `versions`; each connection runs
/// `serve` with its own job number on a thread of its own, and how it ended
/// is kept in order. The port and those ends.
pub fn listen<C: Send + 'static>(
    versions: &'static [&'static rustls::SupportedProtocolVersion],
    serve: impl Fn(TlsConn, u32) -> C + Send + Sync + 'static,
) -> (u16, Arc<Mutex<Vec<C>>>) {
    static JOBS: AtomicU32 = AtomicU32::new(100);
    listen_jobs(versions, || JOBS.fetch_add(1, Ordering::SeqCst), serve)
}

/// [`listen`], each connection's job number from `job`.
fn listen_jobs<C: Send + 'static>(
    versions: &'static [&'static rustls::SupportedProtocolVersion],
    job: impl Fn() -> u32 + Send + 'static,
    serve: impl Fn(TlsConn, u32) -> C + Send + Sync + 'static,
) -> (u16, Arc<Mutex<Vec<C>>>) {
    let certs = qsa_net::tls::certs_from_pem(&file("server.pem"));
    let key = PrivateKeyDer::from_pem_slice(file("server.key").as_bytes()).unwrap();
    let tls = Arc::new(
        rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(versions)
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap(),
    );
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let closed = Arc::new(Mutex::new(Vec::new()));
    let hclosed = Arc::clone(&closed);
    let serve = Arc::new(serve);
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let (closed, tls, serve) = (Arc::clone(&hclosed), Arc::clone(&tls), Arc::clone(&serve));
            let job = job();
            std::thread::spawn(move || {
                let mut conn = rustls::ServerConnection::new(tls).unwrap();
                // Only the handshake waits with a read timeout; after it the
                // socket is non-blocking (see `Polled`).
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                while conn.is_handshaking() {
                    if conn.complete_io(&mut s).is_err() {
                        return;
                    }
                }
                let raw = s.try_clone().unwrap();
                let t = TlsConn {
                    tls: rustls::StreamOwned::new(conn, Polled::new(s).unwrap()),
                    raw,
                };
                let why = serve(t, job);
                closed.lock().unwrap().push(why);
            });
        }
    });
    (port, closed)
}

/// Writes all of `b` to a non-blocking socket; false when it closed.
fn write_plain(plain: &mut TcpStream, mut b: &[u8]) -> bool {
    while !b.is_empty() {
        match plain.write(b) {
            Ok(0) => return false,
            Ok(n) => b = &b[n..],
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(_) => return false,
        }
    }
    true
}

/// Copies bytes both ways between the client's TLS connection and the
/// engine's plain one until either closes, then closes the other: the
/// engine's close (its own, or `System::drop_connection`) reaches the
/// client as the socket closing, with nothing more sent.
fn relay(mut tls: TlsConn, mut plain: TcpStream) {
    plain.set_nonblocking(true).unwrap();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        // The TLS read waits a moment when nothing has come (`Polled`).
        match tls.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if !write_plain(&mut plain, &buf[..n]) {
                    break;
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        match plain.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if tls.write_all(&buf[..n]).and_then(|()| tls.flush()).is_err() {
                    break;
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
    }
    let _ = plain.shutdown(Shutdown::Both);
    tls.close_tls();
}

/// A client target for a test server on `port`, trusting the test CA only.
pub fn target(port: u16) -> Target {
    let mut t = Target::new(
        "127.0.0.1",
        port,
        qsa_net::Trust::from_pem([file("test-ca.pem").as_str()]),
    );
    t.system_roots = false;
    t.options.connect_timeout = Duration::from_secs(5);
    t.options.reply_timeout = Duration::from_secs(10);
    t
}

/// QS/400's test system served over TLS on loopback.
pub struct Server {
    /// The system every connection shares.
    pub c: Arc<System>,
    pub port: u16,
    /// How each connection ended, in the order they ended.
    pub closed: Arc<Mutex<Vec<Closed>>>,
    /// Each connection's job number, in the order they were accepted.
    pub jobs: Arc<Mutex<Vec<u32>>>,
}

/// A server of a new test system with `cfg`, TLS 1.3 only.
pub fn start(cfg: Config) -> Server {
    start_with(cfg, System::new(), TLS13)
}

/// A server of the system `c` offering TLS `versions`.
pub fn start_with(
    cfg: Config,
    c: Arc<System>,
    versions: &'static [&'static rustls::SupportedProtocolVersion],
) -> Server {
    let sys = Arc::clone(&c);
    let jobs = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&jobs);
    let (port, closed) = listen_jobs(versions, qapd::testing::next_job, move |t, job| {
        seen.lock().unwrap().push(job);
        let (plain, engine) = qapd::testing::serve_loopback_as(cfg, &sys, job);
        relay(t, plain);
        engine
            .join()
            .unwrap_or_else(|_| Closed::Panic("the engine's thread".into()))
    });
    Server {
        c,
        port,
        closed,
        jobs,
    }
}

impl Server {
    /// A client target for this server, trusting the test CA only.
    pub fn target(&self) -> Target {
        target(self.port)
    }

    /// A connection signed on as `BOB`.
    pub fn signed_on(&self) -> Connection {
        let c = Connection::open(&self.target()).unwrap();
        let body = SignOn {
            user: "BOB".into(),
            password: Secret::from("SECRET1"),
        };
        let _: SignOnReply = c.call(Op::SignOn, &body).unwrap();
        c
    }

    /// Connections that have ended so far.
    pub fn ended(&self) -> Vec<Closed> {
        self.closed.lock().unwrap().clone()
    }

    /// Job numbers of the connections accepted so far, oldest first.
    pub fn jobs(&self) -> Vec<u32> {
        self.jobs.lock().unwrap().clone()
    }

    /// The job number of the connection accepted last.
    pub fn last_job(&self) -> u32 {
        *self.jobs.lock().unwrap().last().expect("a connection")
    }
}

/// Quick limits for tests.
pub fn cfg() -> Config {
    Config {
        poll: Duration::from_millis(10),
        ..Config::default()
    }
}

/// Waits up to five seconds for `f`.
pub fn until(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
