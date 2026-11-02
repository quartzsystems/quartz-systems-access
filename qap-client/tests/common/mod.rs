// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QS/400's QAP connection engine (`qapd`, pinned with the protocol crates)
//! in this process, over loopback TLS 1.3, with QS/400's own test system:
//! `qapd::testing`'s [`System`] (profiles `BOB`, `WARN` and `ANN`, sessions,
//! Telnet tickets, the test commands, output queues and spooled files, a
//! clock) and a [`MemHost`] per connection.
//!
//! `qapd::testing::serve_loopback` serves one plain TCP connection; QS/400
//! Access speaks only TLS 1.3 and reconnects to resume, so the listener
//! here stays: it takes any number of connections over TLS, each a new
//! connection job of the same system. [`listen`] is generic over the engine,
//! so [`v1`] and [`v2`] put the version-1 and version-2 engines behind it
//! too.

#![allow(dead_code)]

pub mod v1;
pub mod v2;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use qap_client::proto::Op;
use qap_client::proto::ops::signon::{SignOn, SignOnReply};
use qap_client::{Connection, Secret, Target};
use qapd::testing::{MemHost, System};
use qapd::{Closed, Config};
use rustls::pki_types::PrivateKeyDer;
use rustls::pki_types::pem::PemObject;

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../qsa-net/tests/data/");

fn file(name: &str) -> String {
    std::fs::read_to_string(format!("{DATA}{name}")).unwrap()
}

/// The server's end of a TLS connection.
pub struct TlsConn(pub rustls::StreamOwned<rustls::ServerConnection, TcpStream>);

impl TlsConn {
    /// Sends `close_notify` and closes the socket.
    pub fn close_tls(&mut self) {
        self.0.conn.send_close_notify();
        let _ = self.0.conn.complete_io(&mut self.0.sock);
        let _ = self.0.sock.shutdown(Shutdown::Both);
    }
}

impl Read for TlsConn {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(b)
    }
}

impl Write for TlsConn {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl qapd::Transport for TlsConn {
    fn close(&mut self) {
        self.close_tls();
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
            let job = JOBS.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || {
                let mut conn = rustls::ServerConnection::new(tls).unwrap();
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                while conn.is_handshaking() {
                    if conn.complete_io(&mut s).is_err() {
                        return;
                    }
                }
                s.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
                let why = serve(TlsConn(rustls::StreamOwned::new(conn, s)), job);
                closed.lock().unwrap().push(why);
            });
        }
    });
    (port, closed)
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
    let (port, closed) = listen(versions, move |t, job| {
        let mut host = MemHost::new(Arc::clone(&sys), job);
        qapd::serve(t, &mut host, cfg)
    });
    Server { c, port, closed }
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
