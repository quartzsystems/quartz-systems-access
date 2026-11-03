// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! What can go wrong.

use qap_proto::ops::sql::SqlFailure;
use qap_proto::{Op, ServerMessage, Status};

use crate::conn::Closed;

/// Why a request or a connection failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The connection could not be opened: the name, TCP, or TLS
    /// (including [`qsa_net::TlsError::Untrusted`] for the trust flow).
    #[error(transparent)]
    Connect(#[from] qsa_net::ConnectError),
    /// The server answered with a status other than `Ok`, and the message
    /// it sent with it, if any.
    #[error("{}", status_text(*status, message.as_ref()))]
    Status {
        status: Status,
        message: Option<ServerMessage>,
    },
    /// `SqlError` (version 4): the statement failed in the database: its
    /// SQLSTATE, SQLCODE and, for an `Execute` over several rows, the
    /// 0-based failing row of that request; and the SQL message as sent.
    #[error("{}", status_text(Status::SqlError, message.as_ref()))]
    Sql {
        failure: Box<SqlFailure>,
        message: Option<ServerMessage>,
    },
    /// The connection closed before the reply came.
    #[error("connection closed ({0})")]
    Closed(Closed),
    /// No reply in time; the request was cancelled.
    #[error("no reply in time")]
    TimedOut,
    /// The reply grew past [`crate::Options::max_reply`]: it was not
    /// joined, and the connection was closed. The session can be resumed.
    #[error("reply larger than this program allows")]
    ReplyTooLarge,
    /// What a read was written to failed.
    #[error("write failed: {0}")]
    Write(std::io::Error),
    /// What a write reads from failed.
    #[error("read failed: {0}")]
    Read(std::io::Error),
    /// A request that does not fit in one frame.
    #[error("request of {0} bytes exceeds the 1 MiB frame limit")]
    TooLarge(usize),
    /// A reply this client cannot read.
    #[error("{0}")]
    Protocol(String),
    /// A sign-on or resume asked for while signed on.
    #[error("already signed on")]
    AlreadySignedOn,
    /// The connection's protocol version does not define the operation, or
    /// `Hello` did not list its service: nothing was sent. `needs` is the
    /// version that defines it, `has` the connection's.
    #[error("{op:?} needs QAP version {needs}; the system has version {has}")]
    Unsupported { op: Op, needs: u16, has: u16 },
    /// A password change for another profile while signed on: a session
    /// changes only its own profile's password.
    #[error("a signed-on session changes only its own password, not {0}'s")]
    OtherProfile(String),
}

impl Error {
    /// The status, when the server refused.
    pub fn status(&self) -> Option<Status> {
        match self {
            Error::Status { status, .. } => Some(*status),
            Error::Sql { .. } => Some(Status::SqlError),
            _ => None,
        }
    }

    /// The server's message, when it sent one.
    pub fn server_message(&self) -> Option<&ServerMessage> {
        match self {
            Error::Status { message, .. } | Error::Sql { message, .. } => message.as_ref(),
            _ => None,
        }
    }

    /// `Busy`: too many requests outstanding; retry later.
    pub fn is_busy(&self) -> bool {
        self.status() == Some(Status::Busy)
    }
}

fn status_text(status: Status, message: Option<&ServerMessage>) -> String {
    match message {
        Some(m) if !m.id.is_empty() => format!("{}: {}", m.id, m.text),
        Some(m) if !m.text.is_empty() => m.text.clone(),
        _ => format!("{status:?}"),
    }
}
