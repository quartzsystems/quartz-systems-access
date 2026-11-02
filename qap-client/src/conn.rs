// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! One QAP connection. A thread owns the TLS stream: it writes what the
//! caller queues, reads frames, joins split replies and hands each reply to
//! the request that waits for it, keeps the server's events, and sends
//! `Ping` after [`Options::ping_after`] without traffic either way.
//!
//! [`Connection::open`] sends `Hello` first, as request 1. After it every
//! request takes the next free non-zero ID; an ID is free again once its
//! last reply frame is in. The server allows 64 requests outstanding and
//! answers the 65th `Busy`; [`Connection::call`] waits and tries again,
//! [`Connection::request`] hands `Busy` back. A frame is at most 1 MiB:
//! a longer request is refused here ([`Error::TooLarge`]) and a longer
//! frame from the server ends the connection.
//!
//! Request and reply bytes are wiped once used, since they may hold a
//! password or a session token.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use ipc::Wire;
use qap_proto::ops::connection::{Hello, HelloReply};
use qap_proto::ops::event;
use qap_proto::{Envelope, Kind, Op, ProtoError, Reassembly, ServerMessage, Service, Status};
use qsa_net::{Security, Transport, is_timeout};

use crate::frames::{FrameReader, frame};
use crate::{CLIENT_NAME, Error, Options, Target, Wake};

/// How long one read waits, so the thread also writes and pings.
const POLL: Duration = Duration::from_millis(10);

/// Something the server sent unasked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// `Connection` 100: the server is ending and closes the connection.
    /// The message says why.
    ServerEnding(Option<ServerMessage>),
    /// `SignOn` 100: the session ended. Status `Ok` means an operator ended
    /// it; `SessionExpired` that it timed out.
    SessionEnded {
        status: Status,
        message: Option<ServerMessage>,
    },
}

impl Event {
    /// Whether an operator ended the session.
    pub fn by_operator(&self) -> bool {
        matches!(
            self,
            Event::SessionEnded {
                status: Status::Ok,
                ..
            }
        )
    }
}

/// How a connection ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Closed {
    /// The server closed it without an event first (its idle close), or the
    /// link broke. The session, if any, can be resumed.
    Quietly,
    /// The server closed it after `ServerEnding`.
    ServerEnding,
    /// This side closed it.
    ByClient,
    /// The server sent a frame over 1 MiB or one that does not decode.
    BadFrame(String),
}

impl fmt::Display for Closed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Closed::Quietly => f.write_str("by the system"),
            Closed::ServerEnding => f.write_str("the server is ending"),
            Closed::ByClient => f.write_str("by this program"),
            Closed::BadFrame(e) => write!(f, "unreadable frame: {e}"),
        }
    }
}

/// A finished reply: every frame's body joined in order.
#[derive(Debug)]
pub struct Reply {
    pub status: Status,
    pub message: Option<ServerMessage>,
    pub body: Vec<u8>,
}

impl Reply {
    /// The body as `R` when the status is `Ok`; otherwise
    /// [`Error::Status`]. The body's bytes are wiped either way.
    pub fn decode<R: Wire>(mut self) -> Result<R, Error> {
        let r = if self.status == Status::Ok {
            ipc::decode(&self.body).map_err(|e| Error::Protocol(e.to_string()))
        } else {
            Err(Error::Status {
                status: self.status,
                message: self.message.take(),
            })
        };
        wipe(&mut self.body);
        r
    }
}

/// From callers to the connection's thread.
enum Out {
    Frame(Vec<u8>),
    Close,
}

/// What callers and the connection's thread share.
struct Shared {
    /// Requests waiting for their reply; `None` for the thread's own pings.
    waiters: HashMap<u32, Option<Sender<Envelope>>>,
    next_id: u32,
    closed: Option<Closed>,
    events: VecDeque<Event>,
    last_traffic: Instant,
    frames_in: u64,
    pings: u64,
}

impl Shared {
    /// The next non-zero ID no outstanding request has.
    fn alloc(&mut self) -> u32 {
        loop {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if id != 0 && !self.waiters.contains_key(&id) {
                return id;
            }
        }
    }
}

type Link = Arc<Mutex<Shared>>;

fn lock(s: &Link) -> MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wipe(v: &mut [u8]) {
    v.fill(0);
    std::hint::black_box(&v);
}

/// An open connection, `Hello` done.
pub struct Connection {
    shared: Link,
    out: Sender<Out>,
    hello: HelloReply,
    security: Security,
    options: Options,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("system", &self.hello.system_name)
            .field("version", &self.hello.version)
            .finish_non_exhaustive()
    }
}

/// A request sent; its reply is still to come.
pub struct Pending {
    id: u32,
    op: Op,
    rx: Receiver<Envelope>,
    out: Sender<Out>,
    shared: Link,
}

impl Pending {
    /// The request's ID.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Asks the server to stop the request. It still gets a final reply:
    /// `Cancelled`, or its own if it finished first.
    pub fn cancel(&self) {
        if let Ok(b) = frame(&Envelope::cancel(self.id)) {
            let _ = self.out.send(Out::Frame(b));
        }
    }

    /// The reply, waiting at most `timeout`; after that the request is
    /// cancelled and [`Error::TimedOut`] returned.
    pub fn wait(self, timeout: Duration) -> Result<Reply, Error> {
        match self.rx.recv_timeout(timeout) {
            Ok(env) => Ok(self.finish(env)),
            Err(RecvTimeoutError::Timeout) => {
                self.cancel();
                qsa_net::debug!(
                    "qap: {:?} {}: no reply in time; cancelled",
                    self.op,
                    self.id
                );
                Err(Error::TimedOut)
            }
            Err(RecvTimeoutError::Disconnected) => Err(self.gone()),
        }
    }

    /// The reply if it is in, without waiting.
    pub fn try_wait(&self) -> Option<Result<Reply, Error>> {
        match self.rx.try_recv() {
            Ok(env) => Some(Ok(self.finish(env))),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(self.gone())),
        }
    }

    fn finish(&self, env: Envelope) -> Reply {
        qsa_net::debug!("qap: {:?} {}: {:?}", self.op, self.id, env.status);
        Reply {
            status: env.status,
            message: env.message,
            body: env.body,
        }
    }

    fn gone(&self) -> Error {
        Error::Closed(lock(&self.shared).closed.clone().unwrap_or(Closed::Quietly))
    }
}

/// Queues a request on a connection's thread.
fn send<B: Wire>(shared: &Link, out: &Sender<Out>, op: Op, body: &B) -> Result<Pending, Error> {
    let mut s = lock(shared);
    if let Some(c) = &s.closed {
        return Err(Error::Closed(c.clone()));
    }
    let id = s.alloc();
    let mut env = Envelope::request(id, op, body);
    let framed = frame(&env);
    wipe(&mut env.body);
    let b = match framed {
        Ok(b) => b,
        Err(ProtoError::TooLong { len }) => return Err(Error::TooLarge(len)),
        Err(e) => return Err(Error::Protocol(e.to_string())),
    };
    let (tx, rx) = mpsc::channel();
    s.waiters.insert(id, Some(tx));
    drop(s);
    if out.send(Out::Frame(b)).is_err() {
        lock(shared).waiters.remove(&id);
        return Err(Error::Closed(Closed::ByClient));
    }
    qsa_net::debug!("qap: {op:?} {id}: sent");
    Ok(Pending {
        id,
        op,
        rx,
        out: out.clone(),
        shared: Arc::clone(shared),
    })
}

impl Connection {
    /// Connects to `target` over TLS 1.3 and runs `Hello`.
    pub fn open(target: &Target) -> Result<Connection, Error> {
        qsa_net::debug!("qap: connecting to {} port {}", target.host, target.port);
        let t = qsa_net::connect(
            &target.host,
            target.port,
            Some(&target.tls()),
            target.options.connect_timeout,
        )?;
        Connection::over(t, target)
    }

    /// Runs `Hello` over a stream already open (TLS done) to `target`.
    pub fn over(mut t: Box<dyn Transport>, target: &Target) -> Result<Connection, Error> {
        t.set_read_timeout(Some(POLL))
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let security = t.security();
        let shared = Arc::new(Mutex::new(Shared {
            waiters: HashMap::new(),
            next_id: qap_proto::HELLO_ID,
            closed: None,
            events: VecDeque::new(),
            last_traffic: Instant::now(),
            frames_in: 0,
            pings: 0,
        }));
        let (out, rx) = mpsc::channel();
        let pump = Pump {
            t,
            shared: Arc::clone(&shared),
            rx,
            ping_after: target.options.ping_after,
            wake: target.wake.clone(),
            frames: FrameReader::default(),
            asm: Reassembly::new(),
            server_ending: false,
        };
        std::thread::Builder::new()
            .name(format!("qap {}", target.host))
            .spawn(move || pump.run())
            .map_err(|e| Error::Protocol(e.to_string()))?;

        let o = target.options;
        let hello = Hello {
            min_version: o.min_version,
            max_version: o.max_version,
            client_name: CLIENT_NAME.into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
            capabilities: Vec::new(),
        };
        let close = || {
            let _ = out.send(Out::Close);
        };
        let pending = send(&shared, &out, Op::Hello, &hello)?;
        debug_assert_eq!(pending.id(), qap_proto::HELLO_ID);
        let hello: HelloReply = match pending.wait(o.connect_timeout).and_then(Reply::decode) {
            Ok(h) => h,
            Err(e) => {
                close();
                return Err(e);
            }
        };
        if !(o.min_version..=o.max_version).contains(&hello.version) {
            close();
            return Err(Error::Protocol(format!(
                "the server chose version {}, not one offered",
                hello.version
            )));
        }
        qsa_net::debug!(
            "qap: {} port {}: version {}, system {} {}",
            target.host,
            target.port,
            hello.version,
            hello.system_name,
            hello.release
        );
        Ok(Connection {
            shared,
            out,
            hello,
            security,
            options: o,
        })
    }

    /// What `Hello` answered.
    pub fn hello(&self) -> &HelloReply {
        &self.hello
    }

    /// The TLS version and cipher suite.
    pub fn security(&self) -> &Security {
        &self.security
    }

    /// Sends a request and returns at once.
    pub fn request<B: Wire>(&self, op: Op, body: &B) -> Result<Pending, Error> {
        send(&self.shared, &self.out, op, body)
    }

    /// Sends a request and waits for its reply, decoded. `Busy` is tried
    /// again after a pause, for as long as [`Options::reply_timeout`].
    pub fn call<B: Wire, R: Wire>(&self, op: Op, body: &B) -> Result<R, Error> {
        let deadline = Instant::now() + self.options.reply_timeout;
        let mut pause = Duration::from_millis(20);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.request(op, body)?.wait(left).and_then(Reply::decode) {
                Err(e) if e.is_busy() && Instant::now() + pause < deadline => {
                    std::thread::sleep(pause);
                    pause = (pause * 2).min(Duration::from_secs(1));
                }
                r => return r,
            }
        }
    }

    /// The next event the server sent, oldest first.
    pub fn next_event(&self) -> Option<Event> {
        lock(&self.shared).events.pop_front()
    }

    /// How the connection ended; `None` while it is open.
    pub fn closed(&self) -> Option<Closed> {
        lock(&self.shared).closed.clone()
    }

    /// Frames received so far, for diagnostics.
    pub fn frames_received(&self) -> u64 {
        lock(&self.shared).frames_in
    }

    /// Pings sent so far, for diagnostics.
    pub fn pings_sent(&self) -> u64 {
        lock(&self.shared).pings
    }

    /// Closes the connection. A signed-on session stays on the server and
    /// can be resumed until its idle timeout.
    pub fn close(&self) {
        let _ = self.out.send(Out::Close);
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // The thread is not joined: it ends on its own within one poll, so
        // a user interface never waits on the network.
        self.close();
    }
}

/// The connection's thread.
struct Pump {
    t: Box<dyn Transport>,
    shared: Link,
    rx: Receiver<Out>,
    ping_after: Duration,
    wake: Option<Wake>,
    frames: FrameReader,
    asm: Reassembly,
    server_ending: bool,
}

impl Pump {
    fn run(mut self) {
        let mut buf = vec![0u8; 64 * 1024];
        let why = 'run: loop {
            if let Some(why) = self.write_queued() {
                break why;
            }
            let quiet = lock(&self.shared).last_traffic.elapsed();
            if quiet >= self.ping_after && !self.ping() {
                break self.peer_closed();
            }
            match self.t.read(&mut buf) {
                Ok(0) => break self.peer_closed(),
                Ok(n) => {
                    lock(&self.shared).last_traffic = Instant::now();
                    self.frames.push(&buf[..n]);
                    wipe(&mut buf[..n]);
                    loop {
                        match self.frames.next_frame() {
                            Ok(Some(env)) => self.take(env),
                            Ok(None) => break,
                            Err(e) => {
                                qsa_net::warn!("qap: unreadable frame: {e}");
                                break 'run Closed::BadFrame(e.to_string());
                            }
                        }
                    }
                }
                Err(e) if is_timeout(&e) => {}
                Err(e) => {
                    qsa_net::debug!("qap: read failed: {e}");
                    break self.peer_closed();
                }
            }
        };
        self.t.shutdown();
        qsa_net::debug!("qap: connection closed ({why})");
        self.frames.clear();
        {
            let mut s = lock(&self.shared);
            s.closed.get_or_insert(why);
            // Dropping the senders ends every wait with `Closed`.
            s.waiters.clear();
        }
        self.wake();
    }

    fn wake(&self) {
        if let Some(w) = &self.wake {
            w();
        }
    }

    fn peer_closed(&self) -> Closed {
        if self.server_ending {
            Closed::ServerEnding
        } else {
            Closed::Quietly
        }
    }

    /// Writes what callers queued; `Some` when the connection is to end.
    fn write_queued(&mut self) -> Option<Closed> {
        loop {
            match self.rx.try_recv() {
                Ok(Out::Frame(mut b)) => {
                    let ok = self.t.write_all(&b).and_then(|()| self.t.flush());
                    wipe(&mut b);
                    if let Err(e) = ok {
                        qsa_net::debug!("qap: write failed: {e}");
                        return Some(self.peer_closed());
                    }
                    lock(&self.shared).last_traffic = Instant::now();
                }
                Ok(Out::Close) | Err(TryRecvError::Disconnected) => return Some(Closed::ByClient),
                Err(TryRecvError::Empty) => return None,
            }
        }
    }

    /// Sends a `Ping` of its own; false when the write failed.
    fn ping(&mut self) -> bool {
        let id = {
            let mut s = lock(&self.shared);
            let id = s.alloc();
            s.waiters.insert(id, None);
            s.pings += 1;
            s.last_traffic = Instant::now();
            id
        };
        let Ok(b) = frame(&Envelope::request(id, Op::Ping, &())) else {
            return true;
        };
        qsa_net::debug!("qap: Ping {id}: sent after {:?} quiet", self.ping_after);
        self.t.write_all(&b).and_then(|()| self.t.flush()).is_ok()
    }

    fn take(&mut self, env: Envelope) {
        lock(&self.shared).frames_in += 1;
        match env.kind {
            Kind::Reply => {
                let Some(whole) = self.asm.push(env) else {
                    return;
                };
                let waiter = lock(&self.shared).waiters.remove(&whole.request_id);
                match waiter {
                    Some(Some(tx)) => {
                        let _ = tx.send(whole);
                        self.wake();
                    }
                    Some(None) => {}
                    None => qsa_net::debug!(
                        "qap: reply to {}, which is not outstanding",
                        whole.request_id
                    ),
                }
            }
            Kind::Event => {
                let ev = match (env.service, env.op) {
                    (Service::Connection, event::SERVER_ENDING) => {
                        self.server_ending = true;
                        Event::ServerEnding(env.message)
                    }
                    (Service::SignOn, event::SESSION_ENDED) => Event::SessionEnded {
                        status: env.status,
                        message: env.message,
                    },
                    (service, op) => {
                        qsa_net::debug!("qap: event {service:?} {op} not known; ignored");
                        return;
                    }
                };
                qsa_net::debug!("qap: event {ev:?}");
                lock(&self.shared).events.push_back(ev);
                self.wake();
            }
            // Requests and cancels are the client's to send.
            Kind::Request | Kind::Cancel => {}
        }
    }
}
