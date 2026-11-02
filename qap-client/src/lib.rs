// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The QAP client (`docs/qap-protocol-spec.md`): how QS/400 Access for
//! Desktop reaches QS/400 for everything but 5250 display and printer
//! sessions. The envelope, the operations' bodies and the Wire codec come
//! from QS/400's `qap-proto` and `ipc` crates, pinned by commit in the
//! workspace manifest.
//!
//! - [`conn`]: one connection. TLS 1.3 over [`qsa_net`], `Hello` first with
//!   request ID 1, then many requests at once, each with its own ID and
//!   its reply matched by it, whatever order replies come in; split replies
//!   joined, `Ping` after a quiet minute, `Cancel`, and the server's events.
//!   Short operations time out; a command or a spooled file read runs until
//!   it ends or is cancelled ([`runs_long`]).
//! - [`session`]: a [`Client`] signs on, changes passwords, resumes its
//!   session on a new connection when the server closed the old one
//!   quietly, and signs off; from version 2 it also asks when the password
//!   expires and for Telnet tickets.
//! - [`command`]: version 2's `Command` service: run, check and describe CL
//!   commands, with every status they answer.
//!
//! `Hello` offers versions [`proto::MIN_VERSION`] to [`proto::VERSION`] (1
//! to 3) and the server picks the highest both speak. An operation is sent
//! only when the connection's version defines it ([`proto::Op::since`]) and
//! `Hello`'s `services` lists its service; otherwise it is refused here with
//! [`Error::Unsupported`] and nothing is sent ([`Connection::supports`]).
//!
//! Passwords travel only as [`Secret`] and session tokens only as
//! [`Token`]: neither prints, both are wiped when dropped, and this crate
//! logs neither, nor any request or reply body.

pub mod command;
pub mod conn;
pub mod session;

mod error;
mod frames;

use std::sync::Arc;
use std::time::Duration;

pub use command::{CheckOutcome, CommandOutcome};
pub use conn::{Closed, Connection, Event, Pending, Reply, runs_long};
pub use error::Error;
pub use qap_proto as proto;
pub use qap_proto::{Secret, ServerMessage, Status, Token};
pub use session::{Client, Notice};

/// The name `Hello` gives for this client.
pub const CLIENT_NAME: &str = "QS/400 Access for Desktop";

/// Called from the connection's thread after an event arrives or the
/// connection closes, so a user interface can wake and look.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// A system's QAP server and how to reach it.
#[derive(Clone)]
pub struct Target {
    /// Name or address; also the name the server's certificate must carry.
    pub host: String,
    /// The systems file's `qap` port; [`proto::DEFAULT_PORT`] unless changed.
    pub port: u16,
    /// What to trust beyond the system store ([`qsa_net::Trust`]).
    pub trust: qsa_net::Trust,
    /// Use the platform's certificate store as well. Off only in tests.
    pub system_roots: bool,
    pub options: Options,
    pub wake: Option<Wake>,
}

impl Target {
    /// `host:port` with the default options, the system store and `trust`.
    pub fn new(host: impl Into<String>, port: u16, trust: qsa_net::Trust) -> Target {
        Target {
            host: host.into(),
            port,
            trust,
            system_roots: true,
            options: Options::default(),
            wake: None,
        }
    }

    /// The TLS options for this target: TLS 1.3 only.
    pub fn tls(&self) -> qsa_net::TlsOptions {
        qsa_net::TlsOptions {
            trust: self.trust.clone(),
            system_roots: self.system_roots,
            timeout: self.options.connect_timeout,
            tls13_only: true,
        }
    }
}

impl std::fmt::Debug for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Target")
            .field("host", &self.host)
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

/// Timings and the versions offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// TCP connect and TLS handshake together.
    pub connect_timeout: Duration,
    /// How long a short request may wait for its reply before it is
    /// cancelled. [`conn::runs_long`] operations have no such limit.
    pub reply_timeout: Duration,
    /// `Ping` after this long without traffic either way.
    pub ping_after: Duration,
    /// The protocol versions `Hello` offers.
    pub min_version: u16,
    pub max_version: u16,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            connect_timeout: qsa_net::CONNECT_TIMEOUT,
            reply_timeout: Duration::from_secs(60),
            ping_after: proto::PING_AFTER,
            min_version: proto::MIN_VERSION,
            max_version: proto::VERSION,
        }
    }
}
