// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A connection of its own to a signed-on session (version 4), for a
//! window that keeps job state: `Hello`, then `Attach` with the session's
//! token as the first request after it, then `JobInfo`.
//!
//! - No sign-on: the connection joins the session the [`crate::Client`]
//!   signed on, and its job runs as the session's profile with its own
//!   library list, `QTEMP`, commitment definition, statements and cursors.
//! - `Attach` must be the first request after `Hello`, so the connection's
//!   thread sends no `Ping` of its own until it has gone; after it, `Ping`
//!   keeps the connection open on the usual schedule.
//! - A failed `Attach` cannot be tried again on the same connection: it is
//!   closed, and a later try opens a new one. `Busy` (`TCP1A59`) means the
//!   session has its 16 connections; it is not tried again by itself.
//! - An attached connection never signs off: `SignOff` on any connection
//!   ends the whole session. Closing it (or dropping it) closes the
//!   connection only; uncommitted changes are rolled back by the server.
//! - `SessionEnded` (another connection signed off, or an operator ended a
//!   job of the session) and `ServerEnding` end it for good: the token is
//!   forgotten and nothing attaches again ([`Attached::ended`]).
//! - When the connection closes quietly or drops, its job is gone, and
//!   with it the job's statements, cursors and uncommitted changes. The next
//!   request attaches a new connection ([`Attached::attaches`] counts them).

use std::fmt;

use ipc::Wire;
use qap_proto::ops::connection::{HelloReply, JobInfoReply};
use qap_proto::ops::signon::{Attach, SignOnReply};
use qap_proto::{Op, ServerMessage, Status, Token};

use crate::conn::{Closed, Connection, Event, Pending};
use crate::{Error, Target};

/// Why an attached connection's session is over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    /// `SessionEnded`: another connection signed off (`TCP1A58`), an
    /// operator ended one of the session's jobs (`TCP1A54`), or it
    /// expired. The server's message.
    Session(Option<ServerMessage>),
    /// `ServerEnding`, and the server's message.
    Server(Option<ServerMessage>),
    /// Attaching a new connection found the session gone
    /// (`SessionExpired`).
    Expired(Option<ServerMessage>),
}

impl Ended {
    /// The server's message for it.
    pub fn message(&self) -> Option<&ServerMessage> {
        match self {
            Ended::Session(m) | Ended::Server(m) | Ended::Expired(m) => m.as_ref(),
        }
    }
}

/// A connection attached to a session, re-attached on a new connection
/// after a drop.
pub struct Attached {
    target: Target,
    /// The session's token; gone once the session ended.
    token: Option<Token>,
    conn: Option<Connection>,
    hello: HelloReply,
    job: JobInfoReply,
    /// What `Attach` answered, its token zeroed.
    profile: SignOnReply,
    attaches: u64,
    ended: Option<Ended>,
}

impl fmt::Debug for Attached {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attached")
            .field("system", &self.hello.system_name)
            .field("job", &self.job.job)
            .field("attaches", &self.attaches)
            .finish_non_exhaustive()
    }
}

/// One attached connection: `Hello`, `Attach`, `JobInfo`.
fn attach_one(
    target: &Target,
    token: &Token,
) -> Result<(Connection, SignOnReply, JobInfoReply), Error> {
    let c = Connection::open_held(target)?;
    let attached = c.supports(Op::Attach).and_then(|()| {
        let body = Attach {
            token: token.clone(),
        };
        // Not `call`: `Busy` is the session's connection limit, not a
        // request to try again.
        c.request(Op::Attach, &body)?
            .wait(target.options.reply_timeout)?
            .decode::<SignOnReply>()
    });
    let mut profile = match attached {
        Ok(p) => p,
        Err(e) => {
            // A failed Attach cannot be retried on this connection.
            c.close();
            return Err(e);
        }
    };
    profile.token = Token::default();
    c.release_pings();
    let job = match c.call::<_, JobInfoReply>(Op::JobInfo, &()) {
        Ok(j) => j,
        Err(e) => {
            c.close();
            return Err(e);
        }
    };
    qsa_net::debug!(
        "qap: {}: attached job {}/{}/{}",
        target.host,
        job.job.number,
        job.job.user,
        job.job.name
    );
    Ok((c, profile, job))
}

impl Attached {
    /// Attaches a new connection to the session whose token is `token`.
    /// [`Error::Unsupported`] before version 4; `Busy` with `TCP1A59` when
    /// the session has its 16 connections; `SessionExpired` when it is
    /// gone.
    pub fn open(target: &Target, token: &Token) -> Result<Attached, Error> {
        let (c, profile, job) = attach_one(target, token)?;
        Ok(Attached {
            target: target.clone(),
            token: Some(token.clone()),
            hello: c.hello().clone(),
            conn: Some(c),
            job,
            profile,
            attaches: 1,
            ended: None,
        })
    }

    /// What `Hello` answered.
    pub fn hello(&self) -> &HelloReply {
        &self.hello
    }

    /// The connection's job and the profile it runs as, from `JobInfo`.
    pub fn job(&self) -> &JobInfoReply {
        &self.job
    }

    /// What `Attach` answered, its token zeroed.
    pub fn profile(&self) -> &SignOnReply {
        &self.profile
    }

    pub fn target(&self) -> &Target {
        &self.target
    }

    /// Connections attached so far, the first included: a caller that
    /// keeps job state (statements, cursors) notes this and compares, since
    /// a new connection has a new job.
    pub fn attaches(&self) -> u64 {
        self.attaches
    }

    /// Why the session is over, once it is.
    pub fn ended(&self) -> Option<&Ended> {
        self.ended.as_ref()
    }

    /// The open connection, if there is one.
    pub fn connection(&self) -> Option<&Connection> {
        self.conn.as_ref()
    }

    /// Whether `op` may be sent on this connection.
    pub fn supports(&self, op: Op) -> Result<(), Error> {
        crate::conn::supported(&self.hello, op)
    }

    /// Takes in the server's events; whether the session ended with them.
    pub fn poll(&mut self) -> Option<&Ended> {
        self.absorb_events();
        self.ended.as_ref()
    }

    /// The open connection, or a new one attached after a drop.
    pub fn live(&mut self) -> Result<&Connection, Error> {
        self.absorb_events();
        if let Some(why) = self.conn.as_ref().and_then(Connection::closed) {
            qsa_net::debug!(
                "qap: {}: job {} closed ({why})",
                self.target.host,
                self.job.job.number
            );
            self.conn = None;
        }
        if let Some(e) = &self.ended {
            return Err(ended_error(e));
        }
        if self.conn.is_none() {
            let token = self
                .token
                .as_ref()
                .expect("a session not ended has its token");
            match attach_one(&self.target, token) {
                Ok((c, profile, job)) => {
                    self.attaches += 1;
                    self.hello = c.hello().clone();
                    self.profile = profile;
                    self.job = job;
                    self.conn = Some(c);
                }
                Err(e) => {
                    if e.status() == Some(Status::SessionExpired) {
                        self.end(Ended::Expired(e.server_message().cloned()));
                    }
                    return Err(e);
                }
            }
        }
        Ok(self.conn.as_ref().expect("attached above"))
    }

    /// `JobInfo`: the connection's job now.
    pub fn job_info(&mut self) -> Result<JobInfoReply, Error> {
        self.call(Op::JobInfo, &())
    }

    /// Any request, decoded.
    pub fn call<B: Wire, R: Wire>(&mut self, op: Op, body: &B) -> Result<R, Error> {
        let r = self.live()?.call(op, body);
        self.absorb_events();
        r
    }

    /// Sends a request and returns at once; never tried again.
    pub fn request<B: Wire>(&mut self, op: Op, body: &B) -> Result<Pending, Error> {
        let r = self.live()?.request(op, body);
        self.absorb_events();
        r
    }

    /// Closes the connection, never signing off; the session goes on.
    pub fn close(&mut self) {
        if let Some(c) = self.conn.take() {
            c.close();
        }
    }

    fn end(&mut self, why: Ended) {
        // Dropping the token wipes it.
        self.token = None;
        self.ended.get_or_insert(why);
    }

    fn absorb_events(&mut self) {
        let Some(c) = &self.conn else { return };
        let mut events = Vec::new();
        while let Some(ev) = c.next_event() {
            events.push(ev);
        }
        for ev in events {
            match ev {
                Event::ServerEnding(m) => self.end(Ended::Server(m)),
                Event::SessionEnded { message, .. } => self.end(Ended::Session(message)),
            }
        }
    }
}

/// The error a request gets once the session is over.
fn ended_error(e: &Ended) -> Error {
    match e {
        Ended::Server(_) => Error::Closed(Closed::ServerEnding),
        Ended::Session(m) | Ended::Expired(m) => Error::Status {
            status: Status::SessionExpired,
            message: m.clone(),
        },
    }
}
