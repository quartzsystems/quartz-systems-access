// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! A system's QAP client: one connection at a time and, once signed on, one
//! session.
//!
//! - [`Client::sign_on`]: `PasswordExpired` comes back as an error with the
//!   server's message, the connection still open, for
//!   [`Client::change_password`] to follow.
//! - [`Client::change_password`] also works while signed on, for the
//!   session's own profile only.
//! - When the server closes the connection without an event first (its
//!   idle close), the next [`Client::poll`] or request opens a new one and
//!   resumes the session quietly. `SessionExpired` on the resume means a new
//!   sign-on ([`Notice::SignOnNeeded`]).
//! - After `ServerEnding` nothing reconnects by itself: the user is offered
//!   [`Client::reconnect`]. The server's end ends every session.
//! - [`Client::sign_off`] ends the session. Dropping the client only closes
//!   the connection; the session can be resumed until its idle timeout.
//!
//! The session token is held as a [`Token`] only, and the profile kept for
//! display has its token zeroed.

use ipc::Wire;
use qap_proto::ops::connection::HelloReply;
use qap_proto::ops::signon::{ChangePassword, Resume, SignOn, SignOnReply};
use qap_proto::ops::system::SystemInfo;
use qap_proto::{Op, Secret, ServerMessage, Status, Token};

use crate::conn::{Closed, Connection, Event};
use crate::{Error, Target};

/// What happened to the connection or the session since the last look.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// The server had closed the connection quietly; a new one resumed the
    /// session.
    Resumed,
    /// The session could not be resumed: sign on again. The server's
    /// message for it (`SessionExpired`).
    SignOnNeeded(Option<ServerMessage>),
    /// The server is ending and closed the connection; the message says
    /// why. The session is gone. Offer [`Client::reconnect`].
    ServerEnding(Option<ServerMessage>),
    /// The session ended: by an operator when `by_operator`, otherwise it
    /// expired.
    SessionEnded {
        by_operator: bool,
        message: Option<ServerMessage>,
    },
    /// A quiet reconnect failed; the next request tries again.
    ReconnectFailed(String),
}

/// A system's QAP client.
#[derive(Debug)]
pub struct Client {
    target: Target,
    conn: Option<Connection>,
    token: Option<Token>,
    /// The signed-on profile, its token zeroed.
    profile: Option<SignOnReply>,
    /// Set by `ServerEnding`: nothing reconnects until asked to.
    ending: bool,
    notices: Vec<Notice>,
}

/// Whether two profile names are the same profile.
fn same_profile(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

impl Client {
    /// A client for `target`; nothing is opened yet.
    pub fn new(target: Target) -> Client {
        Client {
            target,
            conn: None,
            token: None,
            profile: None,
            ending: false,
            notices: Vec::new(),
        }
    }

    pub fn target(&self) -> &Target {
        &self.target
    }

    /// The open connection, if there is one.
    pub fn connection(&self) -> Option<&Connection> {
        self.conn.as_ref()
    }

    /// What the last `Hello` answered.
    pub fn hello(&self) -> Option<&HelloReply> {
        self.conn.as_ref().map(Connection::hello)
    }

    /// The signed-on profile.
    pub fn profile(&self) -> Option<&SignOnReply> {
        self.profile.as_ref()
    }

    pub fn is_signed_on(&self) -> bool {
        self.token.is_some()
    }

    /// Whether the server said it is ending and nothing has reconnected
    /// since.
    pub fn server_ending(&self) -> bool {
        self.ending
    }

    /// Opens a connection unless one is open.
    pub fn connect(&mut self) -> Result<&HelloReply, Error> {
        self.ending = false;
        Ok(self.live()?.hello())
    }

    /// A new connection, resuming the session if there is one (after
    /// `ServerEnding`, when the user accepts the offer to reconnect).
    pub fn reconnect(&mut self) -> Result<&HelloReply, Error> {
        self.ending = false;
        self.conn = None;
        Ok(self.live()?.hello())
    }

    /// Signs on as `user`.
    pub fn sign_on(&mut self, user: &str, password: &Secret) -> Result<&SignOnReply, Error> {
        if self.token.is_some() {
            return Err(Error::AlreadySignedOn);
        }
        self.ending = false;
        let body = SignOn {
            user: user.to_owned(),
            password: password.clone(),
        };
        let reply = self.live()?.call::<_, SignOnReply>(Op::SignOn, &body);
        self.absorb_events();
        Ok(self.signed_on(reply?))
    }

    /// Changes `user`'s password. Not signed on (after `PasswordExpired`)
    /// it signs on as well; signed on, `user` must be the session's own
    /// profile and the session goes on.
    pub fn change_password(
        &mut self,
        user: &str,
        old: &Secret,
        new: &Secret,
    ) -> Result<&SignOnReply, Error> {
        if let Some(p) = &self.profile
            && !same_profile(user, &p.profile)
        {
            return Err(Error::OtherProfile(user.trim().to_owned()));
        }
        if self.token.is_none() {
            self.ending = false;
        }
        let body = ChangePassword {
            user: user.to_owned(),
            old_password: old.clone(),
            new_password: new.clone(),
        };
        let reply = self
            .live()?
            .call::<_, SignOnReply>(Op::ChangePassword, &body);
        self.absorb_events();
        let mut reply = reply?;
        if self.token.is_some() {
            // The session keeps its token; the reply's is all zeros.
            reply.token = Token::default();
            Ok(self.profile.insert(reply))
        } else {
            Ok(self.signed_on(reply))
        }
    }

    /// Ends the session. The token is forgotten whatever the server said.
    pub fn sign_off(&mut self) -> Result<(), Error> {
        let r = self.live().and_then(|c| c.call::<(), ()>(Op::SignOff, &()));
        self.absorb_events();
        self.end_session();
        r
    }

    /// `SystemInfo`.
    pub fn system_info(&mut self) -> Result<SystemInfo, Error> {
        self.call(Op::SystemInfo, &())
    }

    /// Any request, decoded, on a live connection.
    pub fn call<B: Wire, R: Wire>(&mut self, op: Op, body: &B) -> Result<R, Error> {
        let r = self.live()?.call(op, body);
        self.absorb_events();
        r
    }

    /// Takes in the server's events and, when the server closed the
    /// connection quietly while signed on, reconnects and resumes. What
    /// happened, oldest first.
    pub fn poll(&mut self) -> Vec<Notice> {
        self.absorb_events();
        if let Some(why) = self.conn.as_ref().and_then(Connection::closed) {
            self.conn = None;
            if why == Closed::Quietly && !self.ending && self.token.is_some() {
                match self.live() {
                    Ok(_) => self.notices.push(Notice::Resumed),
                    // `live` has said SignOnNeeded already.
                    Err(e) if e.status() == Some(Status::SessionExpired) => {}
                    Err(e) => self.notices.push(Notice::ReconnectFailed(e.to_string())),
                }
            }
        }
        std::mem::take(&mut self.notices)
    }

    /// The open connection, or a new one that has resumed the session.
    fn live(&mut self) -> Result<&Connection, Error> {
        self.absorb_events();
        if self.conn.as_ref().is_some_and(|c| c.closed().is_some()) {
            self.conn = None;
        }
        if self.conn.is_none() {
            if self.ending {
                return Err(Error::Closed(Closed::ServerEnding));
            }
            let c = Connection::open(&self.target)?;
            if let Some(token) = self.token.clone() {
                match c.call::<_, SignOnReply>(Op::Resume, &Resume { token }) {
                    Ok(reply) => {
                        qsa_net::debug!("qap: {}: session resumed", self.target.host);
                        self.signed_on(reply);
                    }
                    Err(e) => {
                        if e.status() == Some(Status::SessionExpired) {
                            qsa_net::debug!("qap: {}: session expired", self.target.host);
                            self.end_session();
                            self.notices
                                .push(Notice::SignOnNeeded(e.server_message().cloned()));
                            self.conn = Some(c);
                        }
                        return Err(e);
                    }
                }
            }
            self.conn = Some(c);
        }
        Ok(self.conn.as_ref().expect("opened above"))
    }

    fn signed_on(&mut self, mut reply: SignOnReply) -> &SignOnReply {
        self.token = Some(std::mem::take(&mut reply.token));
        qsa_net::debug!("qap: {}: signed on as {}", self.target.host, reply.profile);
        self.profile.insert(reply)
    }

    fn end_session(&mut self) {
        // Dropping the token wipes it.
        self.token = None;
        self.profile = None;
    }

    fn absorb_events(&mut self) {
        let Some(c) = &self.conn else { return };
        let mut events = Vec::new();
        while let Some(ev) = c.next_event() {
            events.push(ev);
        }
        for ev in events {
            match ev {
                Event::ServerEnding(m) => {
                    // The server's end ends every session.
                    self.ending = true;
                    self.end_session();
                    self.notices.push(Notice::ServerEnding(m));
                }
                ev @ Event::SessionEnded { .. } => {
                    let by_operator = ev.by_operator();
                    let Event::SessionEnded { message, .. } = ev else {
                        unreachable!()
                    };
                    self.end_session();
                    self.notices.push(Notice::SessionEnded {
                        by_operator,
                        message,
                    });
                }
            }
        }
    }
}
