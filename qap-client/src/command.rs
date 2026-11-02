// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Version 2's `Command` service: run, check and describe CL commands.
//!
//! `RunCommand` and `CheckCommand` answer their body whatever the command
//! did, so their outcomes carry the status rather than turning it into an
//! error:
//!
//! - `RunCommand`: `Ok`; `Error` with the escape message as the reply's
//!   message (an interactive-only command ends this way, with the system's
//!   escape); `NotFound` and `NotAuthorized` (to the command or to what it
//!   names); `Cancelled`, when [`crate::Pending::cancel`] ended it. A
//!   command cancelled once it started still answers its `CommandReply`,
//!   the message the cancel caused included; one cancelled while it waited
//!   behind another has an empty body, so no job and no messages.
//! - `CheckCommand`: `Ok` with no messages when the command is valid,
//!   otherwise `Error` or `NotFound` with the check's messages.
//!
//! Any other status (`NotSignedOn`, `InvalidRequest`, `Busy`, ...) is an
//! [`Error::Status`]. `DescribeCommand` is an ordinary call: `NotFound` and
//! `NotAuthorized` come back as errors with the system's message.

use qap_proto::ops::command::{CheckReply, CommandReply};
use qap_proto::types::JobId;
use qap_proto::{ServerMessage, Status};

use crate::{Error, Reply};

/// How a `RunCommand` ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutcome {
    /// `Ok`, `Error`, `NotFound`, `NotAuthorized` or `Cancelled`.
    pub status: Status,
    /// The escape message that ended it, when one did.
    pub escape: Option<ServerMessage>,
    /// The job it ran in (QAPCNN in QUSRWRK; its user is the job's own,
    /// not the signed-on profile). `None` when the reply had no body.
    pub job: Option<JobId>,
    /// Every message the command sent its caller, oldest first, with their
    /// second-level text.
    pub messages: Vec<ServerMessage>,
}

/// A reply whose status is not an outcome: the error it is. Its body is
/// wiped.
fn refused(mut reply: Reply) -> Error {
    reply.body.fill(0);
    Error::Status {
        status: reply.status,
        message: reply.message.take(),
    }
}

/// The statuses a `RunCommand` reply carries a command's outcome with.
fn ran(status: Status) -> bool {
    matches!(
        status,
        Status::Ok | Status::Error | Status::NotFound | Status::NotAuthorized | Status::Cancelled
    )
}

impl CommandOutcome {
    /// A `RunCommand` reply as its outcome.
    pub fn from_reply(mut reply: Reply) -> Result<CommandOutcome, Error> {
        if !ran(reply.status) {
            return Err(refused(reply));
        }
        let body: Option<CommandReply> = reply.body_as()?;
        let (job, messages) = match body {
            Some(b) => (Some(b.job), b.messages),
            None => (None, Vec::new()),
        };
        Ok(CommandOutcome {
            status: reply.status,
            escape: reply.message.take(),
            job,
            messages,
        })
    }
}

/// How a `CheckCommand` ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckOutcome {
    /// `Ok`, `Error` or `NotFound`.
    pub status: Status,
    /// The check's messages; none when the command is valid.
    pub messages: Vec<ServerMessage>,
}

impl CheckOutcome {
    /// A `CheckCommand` reply as its outcome.
    pub fn from_reply(mut reply: Reply) -> Result<CheckOutcome, Error> {
        if !matches!(reply.status, Status::Ok | Status::Error | Status::NotFound) {
            return Err(refused(reply));
        }
        let messages = reply
            .body_as::<CheckReply>()?
            .map(|b| b.messages)
            .unwrap_or_default();
        Ok(CheckOutcome {
            status: reply.status,
            messages,
        })
    }

    /// Whether the command is valid.
    pub fn valid(&self) -> bool {
        self.status == Status::Ok
    }
}
