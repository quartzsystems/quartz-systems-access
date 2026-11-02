// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QS/400's QAP connection engine (`qapd`, pinned with the protocol crates)
//! in this process, over loopback TLS 1.3, with the system in memory. The
//! host model follows `net/qapd/tests/common/mod.rs` at the pinned commit:
//! the 5250 display's sign-on rules (`QMAXSIGN` 3 with the profile
//! disabled, CPF1107, CPF1392 before the last try, CPF1393 at it, CPF1394
//! disabled, CPF1118 `*NONE`, CPD9BAE expired) and audit entries as `PW`
//! types. A listener takes any number of connections, each a new
//! connection job, so a client can reconnect and resume.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use qap_client::proto::ops::signon::SignOnReply;
use qap_client::proto::ops::system::{Product, SystemInfo};
use qap_client::{Secret, ServerMessage, Status, Target, Token};
use qapd::sessions::Sessions;
use qapd::{Closed, Config, Ending, Host, Refusal, SystemId};
use rustls::pki_types::PrivateKeyDer;
use rustls::pki_types::pem::PemObject;

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../qsa-net/tests/data/");

fn file(name: &str) -> String {
    std::fs::read_to_string(format!("{DATA}{name}")).unwrap()
}

/// Holds operations in progress while closed.
#[derive(Default)]
pub struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
}

impl Gate {
    pub fn set(&self, open: bool) {
        *self.open.lock().unwrap() = open;
        self.cv.notify_all();
    }

    fn pass(&self) {
        let g = self.open.lock().unwrap();
        drop(self.cv.wait_while(g, |o| !*o).unwrap());
    }
}

/// A user profile as the model keeps it.
#[derive(Clone, Debug, Default)]
pub struct User {
    pub password: String,
    pub disabled: bool,
    pub expired: bool,
    pub none: bool,
    pub invalid: u32,
}

/// What a test controls of the host.
#[derive(Default)]
pub struct Controls {
    pub gate: Gate,
    pub ending: Mutex<Option<Ending>>,
    pub users: Mutex<HashMap<String, User>>,
    pub sessions: Mutex<Sessions>,
    /// Unix seconds.
    pub clock: Mutex<u64>,
    pub idle_secs: u32,
    pub audit: Mutex<Vec<String>>,
    /// Products `SystemInfo` lists.
    pub products: Mutex<usize>,
    /// Who each connection job runs as, by job.
    pub running_as: Mutex<HashMap<u32, String>>,
    /// A password validation program that refuses every new password.
    pub validation_program_refuses: Mutex<bool>,
    tokens: AtomicU32,
}

impl Controls {
    pub fn new() -> Arc<Self> {
        let c = Self {
            idle_secs: 600,
            products: Mutex::new(3),
            ..Self::default()
        };
        c.gate.set(true);
        c.user("BOB", "SECRET1");
        Arc::new(c)
    }

    pub fn user(&self, name: &str, password: &str) {
        self.users.lock().unwrap().insert(
            name.into(),
            User {
                password: password.into(),
                ..User::default()
            },
        );
    }

    pub fn edit(&self, name: &str, f: impl FnOnce(&mut User)) {
        f(self.users.lock().unwrap().get_mut(name).unwrap());
    }

    pub fn get(&self, name: &str) -> User {
        self.users.lock().unwrap()[name].clone()
    }

    pub fn advance(&self, secs: u64) {
        *self.clock.lock().unwrap() += secs;
    }

    pub fn end(&self, why: Ending) {
        *self.ending.lock().unwrap() = Some(why);
    }
}

/// The host in memory: one connection job.
pub struct MemHost {
    pub c: Arc<Controls>,
    pub job: u32,
}

impl MemHost {
    fn refuse(&mut self, status: Status, id: &str, data: &[&str]) -> Refusal {
        Refusal {
            status,
            message: self.message(id, data),
        }
    }

    fn reply(&self, token: Token, user: &str) -> SignOnReply {
        SignOnReply {
            token,
            idle_timeout_secs: self.c.idle_secs,
            profile: user.into(),
            text: format!("{user} profile"),
            ccsid: 37,
            date_format: "*MDY".into(),
            date_separator: "/".into(),
            time_separator: ":".into(),
            home_directory: format!("/home/{user}"),
            special_authorities: Vec::new(),
        }
    }

    /// The display's checks; the profile and whether it expired.
    fn check(&mut self, user: &str, password: &str) -> Result<(String, bool), Refusal> {
        let user = user.trim().to_ascii_uppercase();
        let entry = |c: &Controls, e: &str| c.audit.lock().unwrap().push(format!("PW {e} {user}"));
        let mut users = self.c.users.lock().unwrap();
        let Some(u) = users.get_mut(&user) else {
            drop(users);
            entry(&self.c, "U");
            return Err(self.refuse(Status::SignOnFailed, "CPF1107", &[]));
        };
        if u.disabled {
            drop(users);
            entry(&self.c, "Q");
            return Err(self.refuse(Status::SignOnFailed, "CPF1394", &[&user]));
        }
        if u.none {
            drop(users);
            return Err(self.refuse(Status::SignOnFailed, "CPF1118", &[&user]));
        }
        if u.password != password {
            u.invalid += 1;
            let n = u.invalid;
            if n >= 3 {
                u.disabled = true;
            }
            drop(users);
            entry(&self.c, "P");
            return Err(match n {
                3.. => self.refuse(Status::SignOnFailed, "CPF1393", &[&user]),
                2 => self.refuse(Status::SignOnFailed, "CPF1392", &[]),
                _ => self.refuse(Status::SignOnFailed, "CPF1107", &[]),
            });
        }
        u.invalid = 0;
        Ok((user, u.expired))
    }

    fn open(&mut self, user: &str) -> SignOnReply {
        let n = self.c.tokens.fetch_add(1, Ordering::SeqCst) + 1;
        let token = Token([u8::try_from(n % 251).unwrap(); 32]);
        self.c
            .sessions
            .lock()
            .unwrap()
            .create(&token, user, self.c.idle_secs, self.job);
        self.c
            .running_as
            .lock()
            .unwrap()
            .insert(self.job, user.into());
        self.reply(token, user)
    }
}

impl Host for MemHost {
    fn system_id(&mut self) -> SystemId {
        SystemId {
            system_name: "QSHQ400".into(),
            release: "V1R0M0".into(),
            time_zone: "QN0000UTC".into(),
            ccsid: 37,
        }
    }

    fn now_ms(&mut self) -> i64 {
        self.c.gate.pass();
        1_793_000_000_000
    }

    /// Its text says whose language it was looked up in.
    fn message(&mut self, id: &str, data: &[&str]) -> ServerMessage {
        let lang = self
            .c
            .running_as
            .lock()
            .unwrap()
            .get(&self.job)
            .map_or_else(|| "system".to_string(), |u| format!("lang of {u}"));
        ServerMessage {
            id: id.into(),
            severity: 40,
            data: data.join(" ").into_bytes(),
            text: format!("{id} {} ({lang})", data.join(" ")),
            help: format!("Cause . . . : {id} help. Recovery  . . . : Try again."),
        }
    }

    fn system_info(&mut self) -> SystemInfo {
        self.c.gate.pass();
        let n = *self.c.products.lock().unwrap();
        SystemInfo {
            system_name: "QSHQ400".into(),
            release: "V1R0M0".into(),
            serial: "1234567".into(),
            model: "Q01".into(),
            ipl_time: 1_792_000_000_000,
            products: (0..n)
                .map(|i| Product {
                    id: format!("QS{i:05}"),
                    option: "0000".into(),
                    release: "V1R0M0".into(),
                })
                .collect(),
        }
    }

    fn ending(&mut self) -> Option<Ending> {
        self.c.ending.lock().unwrap().take()
    }

    fn sign_on(&mut self, user: &str, password: &Secret) -> Result<SignOnReply, Refusal> {
        let (user, expired) = self.check(user, password.expose())?;
        if expired {
            return Err(self.refuse(Status::PasswordExpired, "CPD9BAE", &[]));
        }
        Ok(self.open(&user))
    }

    fn change_password(
        &mut self,
        user: &str,
        old: &Secret,
        new: &Secret,
    ) -> Result<SignOnReply, Refusal> {
        let me = self.c.running_as.lock().unwrap().get(&self.job).cloned();
        let user = user.trim().to_ascii_uppercase();
        match &me {
            Some(m) if *m != user => {
                return Err(self.refuse(Status::NotAuthorized, "CPF22E9", &[&user]));
            }
            Some(_) => {
                if self.c.get(&user).password != old.expose() {
                    return Err(self.refuse(Status::Error, "CPF22E2", &[&user]));
                }
            }
            None => {
                self.check(&user, old.expose())?;
            }
        }
        if new.expose().len() < 6 || new.expose() == old.expose() {
            return Err(self.refuse(Status::Error, "CPF2362", &[]));
        }
        if *self.c.validation_program_refuses.lock().unwrap() {
            return Err(self.refuse(Status::Error, "CPF22C6", &[]));
        }
        self.c.edit(&user, |u| {
            u.password = new.expose().into();
            u.expired = false;
        });
        Ok(match me {
            Some(_) => self.reply(Token([0; 32]), &user),
            None => self.open(&user),
        })
    }

    fn resume(&mut self, token: &Token) -> Result<SignOnReply, Refusal> {
        let now = *self.c.clock.lock().unwrap();
        let r = self
            .c
            .sessions
            .lock()
            .unwrap()
            .resume(token, self.job, now, &|_| true);
        match r {
            Ok(s) => {
                self.c
                    .running_as
                    .lock()
                    .unwrap()
                    .insert(self.job, s.user.clone());
                Ok(self.reply(token.clone(), &s.user))
            }
            Err(_) => Err(self.refuse(Status::SessionExpired, "TCP1A53", &[])),
        }
    }

    fn sign_off(&mut self, token: &Token) {
        self.c.sessions.lock().unwrap().end(token);
        self.c.running_as.lock().unwrap().remove(&self.job);
    }

    fn detach(&mut self, token: &Token) {
        let now = *self.c.clock.lock().unwrap();
        self.c.sessions.lock().unwrap().detach(token, self.job, now);
        self.c.running_as.lock().unwrap().remove(&self.job);
    }
}

/// The server's end of a TLS connection.
pub struct TlsConn(rustls::StreamOwned<rustls::ServerConnection, TcpStream>);

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
        self.0.conn.send_close_notify();
        let _ = self.0.conn.complete_io(&mut self.0.sock);
        let _ = self.0.sock.shutdown(Shutdown::Both);
    }
}

/// A QAP server on loopback.
pub struct Server {
    pub c: Arc<Controls>,
    pub port: u16,
    /// How each connection ended, in the order they ended.
    pub closed: Arc<Mutex<Vec<Closed>>>,
}

/// TLS 1.3 alone, as QS/400's server offers.
pub const TLS13: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];

/// A server with `cfg`, TLS 1.3 only.
pub fn start(cfg: Config) -> Server {
    start_with(cfg, Controls::new(), TLS13)
}

/// A server of the system `c` offering TLS `versions`.
pub fn start_with(
    cfg: Config,
    c: Arc<Controls>,
    versions: &'static [&'static rustls::SupportedProtocolVersion],
) -> Server {
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
    let (hc, hclosed) = (Arc::clone(&c), Arc::clone(&closed));
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let (c, closed, tls) = (Arc::clone(&hc), Arc::clone(&hclosed), Arc::clone(&tls));
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
                let mut host = MemHost { c, job };
                let why = qapd::serve(TlsConn(rustls::StreamOwned::new(conn, s)), &mut host, cfg);
                closed.lock().unwrap().push(why);
            });
        }
    });
    Server { c, port, closed }
}

impl Server {
    /// A client target for this server, trusting the test CA only.
    pub fn target(&self) -> Target {
        let mut t = Target::new(
            "127.0.0.1",
            self.port,
            qsa_net::Trust::from_pem([file("test-ca.pem").as_str()]),
        );
        t.system_roots = false;
        t.options.connect_timeout = Duration::from_secs(5);
        t.options.reply_timeout = Duration::from_secs(10);
        t
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
