// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QAP sessions in the application: one session per system per process.
//!
//! - [`Registry`] holds one [`Client`] per system, shared by every tool as a
//!   [`Handle`]. A tool asks [`session`] for a signed-on one; it prompts
//!   Sign On only when there is none, and otherwise every tool uses the same
//!   session and connection, with requests multiplexed on it and the
//!   session resumed quietly after a drop.
//! - Network calls run off the GUI thread ([`busy`]). A call that waits for
//!   long (a command that runs) holds the client only to send
//!   ([`Client::request`]) and waits on its [`qap_client::Pending`] without
//!   it, so other tools go on meanwhile. The GUI thread never blocks on a
//!   client another thread holds: it looks with `try_lock` and otherwise
//!   uses what it last saw.
//! - The client's errors become `QAD` messages, with the system's own
//!   message kept as it was sent; [`pump`] shows `SessionEnded` and
//!   `ServerEnding` and offers to sign on or reconnect.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use fltk::app;
use fltk::prelude::*;
use qap_client::proto::ops::command::CommandDefinition;
use qap_client::proto::ops::signon::PasswordStatus;
use qap_client::proto::types::JobId;
use qap_client::proto::{Op, ServerMessage};
use qap_client::{Attached, Client, Closed, Ended, Error, Notice, Status, Target};
use qsa_config::{AddressLookup, System};
use qsa_msg::{Message, id};
use qsa_net::{ConnectError, TlsError};
use qsa_ui::msgdetails::{self, SystemMessage};

use crate::app::Shared;
use crate::mainwin::Ui;

/// Where a system's QAP server is: its name (or fixed address) and its
/// `qap` port, trusting what the system's configuration trusts.
pub fn target_for(system: &System) -> Target {
    let host = if system.address_lookup == AddressLookup::Never && !system.fixed_ip.is_empty() {
        system.fixed_ip.clone()
    } else {
        system.name.clone()
    };
    let mut t = Target::new(host, system.ports.qap, crate::trust::for_system(system));
    t.wake = Some(Arc::new(app::awake));
    t
}

/// The key a system's client is kept under.
fn key(system: &str) -> String {
    system.to_ascii_lowercase()
}

/// A system's client, shared by every tool in the process.
pub type Handle = Arc<Mutex<Client>>;

/// Locks a client, poisoned or not: a tool that panicked mid-call leaves a
/// client that is still usable, or that says it is not.
pub fn lock(h: &Handle) -> MutexGuard<'_, Client> {
    h.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One system's entry.
struct Entry {
    client: Handle,
    /// The signed-on profile as the GUI thread last saw it, for when
    /// another thread holds the client.
    profile: Option<String>,
    /// Command definitions Prompt asked for in this session, by
    /// `LIBRARY/NAME` as asked, with the profile they were asked as.
    definitions: HashMap<String, (String, CommandDefinition)>,
    /// The password expiry warning was shown in this session.
    warned: bool,
    /// The client's resume count when last looked at.
    resumes_seen: u64,
}

/// Every system's client in this process.
#[derive(Default)]
pub struct Registry {
    map: HashMap<String, Entry>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut m = f.debug_map();
        for (k, e) in &self.map {
            m.entry(k, &e.profile);
        }
        m.finish()
    }
}

impl Registry {
    /// The client for `system`: the one there is, unless it is not signed on
    /// and free, in which case a new one for the system as it is configured
    /// now (its address, port and trust may have changed).
    pub fn client(&mut self, system: &System) -> Handle {
        let k = key(&system.name);
        if let Some(e) = self.map.get(&k) {
            let stale = e.client.try_lock().is_ok_and(|c| !c.is_signed_on());
            if !stale {
                return Arc::clone(&e.client);
            }
        }
        let client = Arc::new(Mutex::new(Client::new(target_for(system))));
        self.map.insert(
            k,
            Entry {
                client: Arc::clone(&client),
                profile: None,
                definitions: HashMap::new(),
                warned: false,
                resumes_seen: 0,
            },
        );
        client
    }

    /// Whether `system`'s session resumed since this was last asked (the
    /// client free to look at).
    pub fn resumed(&mut self, system: &str) -> bool {
        let Some(e) = self.map.get_mut(&key(system)) else {
            return false;
        };
        let Ok(c) = e.client.try_lock() else {
            return false;
        };
        let n = c.resumes();
        drop(c);
        let new = n != e.resumes_seen;
        e.resumes_seen = n;
        new
    }

    /// Takes `system`'s one password expiry warning of the session: true
    /// the first time.
    pub fn take_warning(&mut self, system: &str) -> bool {
        match self.map.get_mut(&key(system)) {
            Some(e) if !e.warned => {
                e.warned = true;
                true
            }
            _ => false,
        }
    }

    /// A command definition kept for `system`'s session, if this session
    /// (the same profile, signed on since) asked for it.
    pub fn definition(&mut self, system: &str, key_: &str) -> Option<CommandDefinition> {
        let profile = self.profile(system)?;
        let e = self.map.get(&key(system))?;
        let (asked_as, def) = e.definitions.get(&key_.to_ascii_uppercase())?;
        (*asked_as == profile).then(|| def.clone())
    }

    /// Keeps a command definition for `system`'s session.
    pub fn keep_definition(&mut self, system: &str, key_: &str, def: CommandDefinition) {
        let Some(profile) = self.profile(system) else {
            return;
        };
        if let Some(e) = self.map.get_mut(&key(system)) {
            e.definitions
                .insert(key_.to_ascii_uppercase(), (profile, def));
        }
    }

    /// The client for `system` if it is signed on. A client busy on
    /// another thread counts as signed on when it last was.
    pub fn signed_on_client(&mut self, system: &str) -> Option<Handle> {
        self.profile(system)?;
        self.get(&key(system))
    }

    /// The profile signed on to `system`, if any; what was last seen when
    /// another thread holds the client.
    pub fn profile(&mut self, system: &str) -> Option<String> {
        let e = self.map.get_mut(&key(system))?;
        if let Ok(c) = e.client.try_lock() {
            e.profile = c.profile().map(|p| p.profile.clone());
        }
        e.profile.clone()
    }

    /// Takes `system`'s client out (Sign Off).
    pub fn remove(&mut self, system: &str) -> Option<Handle> {
        self.map.remove(&key(system)).map(|e| e.client)
    }

    /// Forgets `system`'s client once a window's own connection saw the
    /// session end: it is over for every tool, so the next one signs on,
    /// whether or not the shared connection has heard yet.
    pub fn forget_ended(&mut self, system: &str) {
        if self.map.remove(&key(system)).is_some() {
            crate::log!("QAP session on {system} ended; the next tool signs on again");
        }
    }

    /// The systems with a client, by key.
    pub fn keys(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }

    /// The client kept under `k`.
    pub fn get(&self, k: &str) -> Option<Handle> {
        self.map.get(&key(k)).map(|e| Arc::clone(&e.client))
    }
}

/// Runs `f` on another thread while the GUI goes on drawing, and returns
/// what it returned. Windows stay responsive; modal dialogs keep other
/// windows from starting anything.
pub fn busy<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        let h = s.spawn(|| {
            let r = f();
            app::awake();
            r
        });
        while !h.is_finished() {
            let _ = app::wait_for(0.05);
        }
        h.join().expect("QAP call panicked")
    })
}

/// Sends `body` as `op` holding the client only to send, then waits
/// without it: other tools use the session meanwhile.
pub fn call_unlocked<B: qap_client::Wire, R: qap_client::Wire>(
    h: &Handle,
    op: Op,
    body: &B,
) -> Result<R, Error> {
    let (pending, timeout) = {
        let mut c = lock(h);
        let t = c.target().options.reply_timeout;
        (c.request(op, body)?, t)
    };
    pending.wait(timeout).and_then(qap_client::Reply::decode)
}

/// `system`'s client, for a sign-on, a password change or a tool.
pub fn client(state: &Shared, system: &System) -> Handle {
    state.borrow_mut().qap.client(system)
}

/// The profile signed on to `system`, if any.
pub fn signed_on(state: &Shared, system: &str) -> Option<String> {
    state.borrow_mut().qap.profile(system)
}

/// The QAP version `system` speaks, for settings that depend on it: what
/// this process's last `Hello` with it said, or, when `ask`, a new `Hello`
/// (no sign-on). `None` when it is not known or cannot be reached.
pub fn version_of(state: &Shared, system: &str, ask: bool) -> Option<u16> {
    let sys = state.borrow().systems.get(system).cloned()?;
    let h = client(state, &sys);
    if let Ok(c) = h.try_lock()
        && let Some(v) = c.version()
    {
        return Some(v);
    }
    if !ask {
        return None;
    }
    busy(|| lock(&h).connect().ok().map(|hello| hello.version))
}

/// Why a 5250 session cannot sign on to `system` automatically, if it
/// cannot: `QAD5030` without Telnet over TLS, `QAD5001` when the system's
/// QAP version has no `TelnetTicket`. A version not known does not stop
/// it: the connection finds out.
pub fn auto_sign_on_blocked(system: &str, tls: bool, version: Option<u16>) -> Option<Message> {
    if !tls {
        return Some(qsa_msg::message(id::AUTO_SIGNON_NEEDS_TLS, &[]));
    }
    match version {
        Some(v) if v < Op::TelnetTicket.since() => Some(version_needed(
            function_name(Op::TelnetTicket),
            system,
            Op::TelnetTicket,
            v,
        )),
        _ => None,
    }
}

/// Why Printer Output is not available for `system`, if it is not:
/// `QAD5001` when the system's QAP version has no `Spool` service. A
/// version not known does not stop it: opening it finds out.
pub fn printer_output_blocked(system: &str, version: Option<u16>) -> Option<Message> {
    let op = Op::ListSpooledFiles;
    match version {
        Some(v) if v < op.since() => Some(version_needed(function_name(op), system, op, v)),
        _ => None,
    }
}

/// The days until a password expires, when that is within the system's
/// warning period: `status` from `PasswordStatus`, `now_ms` the system's
/// time. Part of a day counts as a day; an expired password has 0.
pub fn expires_within(status: &PasswordStatus, now_ms: i64) -> Option<u32> {
    const DAY: i64 = 86_400_000;
    let left = status.expires? - now_ms;
    if left > i64::from(status.warning_days) * DAY {
        return None;
    }
    let days = (left.max(0) + DAY - 1) / DAY;
    u32::try_from(days).ok()
}

/// Asks the system when the signed-on profile's password expires: the
/// profile and the days left when within the warning period. A system
/// whose QAP version has no `PasswordStatus` answers `None`, silently.
pub fn expiry_due(h: &Handle) -> Result<Option<(String, u32)>, Error> {
    let mut c = lock(h);
    if c.supports(Op::PasswordStatus).is_err() {
        return Ok(None);
    }
    let status = c.password_status()?;
    let now = c.server_now_ms().unwrap_or(0);
    let profile = c.profile().map(|p| p.profile.clone()).unwrap_or_default();
    Ok(expires_within(&status, now).map(|d| (profile, d)))
}

/// `QAD4031`: `user`'s password on `system` expires in `days` days.
pub fn expiry_message(user: &str, system: &str, days: u32) -> Message {
    qsa_msg::message(
        id::PASSWORD_EXPIRES_SOON,
        &[user, system, &days.to_string()],
    )
}

/// After a sign-on or a resume on `system`: asks when the password
/// expires and, within the system's warning period, shows `QAD4031` with
/// Change Password..., once per session.
pub fn warn_expiry(parent: Option<&dyn WindowExt>, state: &Shared, system: &str) {
    let Some(h) = state.borrow_mut().qap.signed_on_client(system) else {
        return;
    };
    let due = match busy(|| expiry_due(&h)) {
        Ok(Some(due)) => due,
        Ok(None) => return,
        Err(e) => {
            crate::log!("password status of {system}: {e}");
            return;
        }
    };
    if !state.borrow_mut().qap.take_warning(system) {
        return;
    }
    let m = expiry_message(&due.0, system, due.1);
    crate::log!("{m}");
    if msgdetails::ask_message(parent, &m, &["Change Password...", "Close"]) == 0 {
        crate::chgpwd::run(parent, state, system, None, None);
    }
}

/// A signed-on session on `system` for a tool: the one this process has,
/// or, when there is none, one the user signs on to now (Sign On is shown
/// once; `None` when they cancel it).
pub fn session(parent: Option<&dyn WindowExt>, state: &Shared, system: &str) -> Option<Handle> {
    let existing = state.borrow_mut().qap.signed_on_client(system);
    if existing.is_some() {
        return existing;
    }
    if !crate::signon::run(parent, state, system) {
        return None;
    }
    state.borrow_mut().qap.signed_on_client(system)
}

/// A job as the system names it: `number/user/name`.
pub fn job_name(job: &JobId) -> String {
    format!("{}/{}/{}", job.number, job.user, job.name)
}

/// A window's title: `function - system (job)`.
pub fn window_title(function: &str, system: &str, job: Option<&JobId>) -> String {
    match job {
        Some(j) => format!("{function} - {system} ({})", job_name(j)),
        None => format!("{function} - {system}"),
    }
}

/// `QAD7001`: `function` has its own connection to `system`, in `job`.
pub fn attached_message(system: &str, function: &str, job: &JobId) -> Message {
    qsa_msg::message(id::ATTACHED, &[system, function, &job_name(job)])
}

/// Attaches a connection of its own to `h`'s session for `function` on
/// `system` (version 4), off the GUI thread. A failure is shown and logged:
/// the session's 16 connections in use (`Busy`, `TCP1A59`) as `QAD7003`,
/// anything else as [`explain`] says. `QAD7001` is logged on success.
pub fn attach(
    parent: Option<&dyn WindowExt>,
    h: &Handle,
    system: &System,
    function: &str,
) -> Option<Attached> {
    match busy(|| lock(h).attach()) {
        Ok(a) => {
            crate::log!("{}", attached_message(&system.name, function, &a.job().job));
            Some(a)
        }
        Err(e) => {
            show(parent, &explain(system, &e));
            None
        }
    }
}

/// Why the session under a window's own connection is over, as messages:
/// the system's own (`TCP1A58` another connection signed off, `TCP1A54` an
/// operator ended a job of the session) as it was sent, with this program's
/// for the case.
pub fn ended(system: &System, why: &Ended) -> Explained {
    let context = match why {
        Ended::Server(_) => id::QAP_SERVER_ENDING,
        Ended::Session(Some(m)) if m.id == "TCP1A54" => id::SESSION_ENDED_BY_OPERATOR,
        Ended::Session(Some(m)) if m.id == "TCP1A58" => id::SIGNED_OFF,
        _ => id::QAP_SESSION_EXPIRED,
    };
    Explained {
        message: qsa_msg::message(context, &[&system.name]),
        server: why.message().cloned(),
    }
}

/// Shows `x` with Sign On... and Close; true when the user chose to sign
/// on.
pub fn offer_sign_on(parent: Option<&dyn WindowExt>, x: &Explained) -> bool {
    crate::log!("{}", x.message);
    let buttons = ["Sign On...", "Close"];
    let choice = match &x.server {
        Some(sm) => msgdetails::ask_system_message(parent, Some(&x.message), &shown(sm), &buttons),
        None => msgdetails::ask_message(parent, &x.message, &buttons),
    };
    choice == 0
}

/// What a version-2 or version-3 function is called where the user sees
/// it, for `QAD5001`.
pub fn function_name(op: Op) -> &'static str {
    match op {
        Op::RunCommand => "Run Command",
        Op::CheckCommand => "Check",
        Op::DescribeCommand => "Prompt",
        Op::PasswordStatus => "The password expiry warning",
        Op::TelnetTicket => "Automatic sign-on",
        Op::ListSpooledFiles
        | Op::ReadSpooledFile
        | Op::HoldSpooledFile
        | Op::ReleaseSpooledFile
        | Op::DeleteSpooledFile
        | Op::MoveSpooledFile
        | Op::ListOutputQueues => "Printer Output",
        Op::SetOptions
        | Op::Prepare
        | Op::Execute
        | Op::Fetch
        | Op::CloseCursor
        | Op::Release
        | Op::Commit
        | Op::Rollback => "Run SQL",
        Op::ListSchemas | Op::ListTables | Op::ListColumns => "Schemas",
        Op::ListDirectory
        | Op::GetInfo
        | Op::ReadFile
        | Op::WriteFile
        | Op::CreateDirectory
        | Op::Delete
        | Op::Rename
        | Op::Copy => crate::ifs::FUNCTION,
        Op::ListMessages
        | Op::ReplyMessage
        | Op::RemoveMessages
        | Op::SendMessage
        | Op::WatchQueue
        | Op::UnwatchQueue => crate::messages::FUNCTION,
        _ => "This function",
    }
}

/// `QAD5001`: `function` needs the QAP version that defines `op`, and
/// `system` speaks `has`.
pub fn version_needed(function: &str, system: &str, op: Op, has: u16) -> Message {
    qsa_msg::message(
        id::QAP_VERSION_NEEDED,
        &[function, &op.since().to_string(), system, &has.to_string()],
    )
}

/// A system's message for showing.
pub fn shown(m: &ServerMessage) -> SystemMessage<'_> {
    SystemMessage {
        id: &m.id,
        severity: m.severity,
        text: &m.text,
        help: &m.help,
    }
}

/// What to tell the user about a failed call: this program's message, and
/// the system's own message when it sent one.
#[derive(Debug)]
pub struct Explained {
    pub message: Message,
    pub server: Option<ServerMessage>,
}

/// One line for the status bar and the log: the system's message ID and
/// text when there is one, otherwise this program's message.
impl Explained {
    pub fn line(&self) -> String {
        match &self.server {
            Some(m) => format!("{}: {}", m.id, m.text),
            None => self.message.to_string(),
        }
    }
}

/// `e`, from a call to `system`'s QAP server, as messages.
pub fn explain(system: &System, e: &Error) -> Explained {
    let sys = system.name.as_str();
    let port = system.ports.qap.to_string();
    let m = |id: &str, args: &[&str]| qsa_msg::message(id, args);
    let message = match e {
        Error::Connect(ConnectError::NotFound(h)) => m(id::HOST_NOT_FOUND, &[h]),
        Error::Connect(ConnectError::Refused(_)) => {
            m(id::QAP_NOT_REACHED, &[sys, &port, "connection refused"])
        }
        Error::Connect(ConnectError::TimedOut(_)) => {
            m(id::QAP_NOT_REACHED, &[sys, &port, "no answer in time"])
        }
        Error::Connect(ConnectError::Io { source, .. }) => {
            m(id::QAP_NOT_REACHED, &[sys, &port, &source.to_string()])
        }
        Error::Connect(ConnectError::Tls(TlsError::Untrusted { reason, .. })) => {
            m(id::CERT_UNTRUSTED, &[sys, reason])
        }
        Error::Connect(ConnectError::Tls(TlsError::Handshake(r))) => {
            m(id::QAP_TLS_FAILED, &[sys, r])
        }
        Error::Status {
            status: Status::VersionUnsupported,
            ..
        } => {
            let o = qap_client::Options::default();
            m(
                id::QAP_VERSION_UNSUPPORTED,
                &[sys, &o.min_version.to_string(), &o.max_version.to_string()],
            )
        }
        Error::Status {
            status: Status::SessionExpired,
            ..
        } => m(id::QAP_SESSION_EXPIRED, &[sys]),
        e if no_more_connections(e) => m(id::NO_MORE_CONNECTIONS, &[sys]),
        Error::Status {
            status,
            message: Some(sm),
        } => {
            // The template ends the sentence itself.
            let why = format!("{}: {}", sm.id, sm.text.trim_end_matches('.'));
            match status {
                Status::SignOnFailed | Status::PasswordExpired | Status::NotAuthorized => {
                    m(id::SIGNON_FAILED, &[sys, &why])
                }
                _ => m(id::UNEXPECTED_REPLY, &["QAP", sys, &why]),
            }
        }
        Error::Status { status, .. } => {
            m(id::UNEXPECTED_REPLY, &["QAP", sys, &format!("{status:?}")])
        }
        Error::Unsupported { op, has, .. } => version_needed(function_name(*op), sys, *op, *has),
        Error::Closed(Closed::ServerEnding) => m(id::QAP_SERVER_ENDING, &[sys]),
        Error::Closed(Closed::Quietly) => m(id::SESSION_ENDED, &[sys]),
        Error::Closed(Closed::ByClient) => m(id::DISCONNECTED, &[sys]),
        Error::TimedOut => m(id::CONNECT_TIMED_OUT, &[sys]),
        Error::ReplyTooLarge | Error::Closed(Closed::ReplyTooLarge(_)) => {
            m(id::REPLY_TOO_LARGE, &[sys])
        }
        e => m(id::UNEXPECTED_REPLY, &["QAP", sys, &e.to_string()]),
    };
    // QAD7003 is shown for TCP1A59; the system's line goes to the log.
    let server = if no_more_connections(e) {
        if let Some(sm) = e.server_message() {
            crate::log!("{}: {}", sm.id, sm.text);
        }
        None
    } else {
        e.server_message().cloned()
    };
    Explained { message, server }
}

/// `Busy` with `TCP1A59`: the session has its 16 connections.
pub fn no_more_connections(e: &Error) -> bool {
    e.status() == Some(Status::Busy) && e.server_message().is_some_and(|m| m.id == "TCP1A59")
}

/// Shows a failure: the system's message alone and as it was sent when there
/// is one (this program's message goes to the status line and the log), or
/// this program's message.
pub fn show(parent: Option<&dyn WindowExt>, x: &Explained) {
    crate::log!("{}", x.line());
    match &x.server {
        Some(sm) => msgdetails::show_system_message(parent, None, &shown(sm)),
        None => msgdetails::show_message(parent, &x.message),
    }
}

/// Signs off from the selected system, if signed on. Every tool's session
/// on it ends with it.
pub fn sign_off(ui: &Ui, system: &str) {
    let Some(h) = ui.state.borrow_mut().qap.remove(system) else {
        return;
    };
    let r = busy(|| lock(&h).sign_off());
    if let Err(e) = r {
        crate::log!("sign-off from {system}: {e}");
    }
    report(ui, &qsa_msg::message(id::SIGNED_OFF, &[system]));
    ui.refresh_systems();
}

fn report(ui: &Ui, m: &Message) {
    crate::log!("{m}");
    ui.report(m);
}

/// Looks at every signed-on client no tool is using right now: takes in
/// the server's events, lets a quietly closed connection resume, and tells
/// the user what ended. A client in use is looked at next time.
pub fn pump(ui: &Ui) {
    let keys = ui.state.borrow().qap.keys();
    for k in keys {
        let Some(h) = ui.state.borrow().qap.get(&k) else {
            continue;
        };
        let (quiet, host) = {
            let Ok(c) = h.try_lock() else { continue };
            if !c.is_signed_on() {
                continue;
            }
            let quiet = c.connection().is_none_or(|n| n.closed().is_some());
            (quiet, c.target().host.clone())
        };
        // Reconnecting reaches the network; do it off the GUI thread.
        let notices = if quiet {
            busy(|| lock(&h).poll())
        } else {
            lock(&h).poll()
        };
        let system = ui
            .state
            .borrow()
            .systems
            .get(&k)
            .cloned()
            .unwrap_or_else(|| System::new(&host));
        let mut offer = None;
        for n in notices {
            offer = offer.or(tell(ui, &system, n));
        }
        // What the watches reported (version 6), for the windows that show
        // it. A client busy elsewhere is looked at next time.
        let events = h
            .try_lock()
            .map(|mut c| c.watch_events())
            .unwrap_or_default();
        if !events.is_empty() {
            crate::messages::watch_events(&system.name, &events);
        }
        if ui.state.borrow_mut().qap.resumed(&k) {
            warn_expiry(Some(&ui.window), &ui.state, &system.name);
        }
        ui.refresh_systems();
        match offer {
            Some(Offer::SignOn) => crate::signon::run(Some(&ui.window), &ui.state, &system.name),
            Some(Offer::Reconnect) => reconnect(ui, &system),
            None => false,
        };
    }
}

/// What to offer after a notice.
enum Offer {
    SignOn,
    Reconnect,
}

/// Tells the user about one notice; what they chose to do next.
fn tell(ui: &Ui, system: &System, n: Notice) -> Option<Offer> {
    let sys = system.name.as_str();
    let parent = Some(&ui.window as &dyn WindowExt);
    let (context, server, buttons, offer): (Message, Option<ServerMessage>, [&str; 2], Offer) =
        match n {
            Notice::Resumed => {
                crate::log!("QAP session on {sys} resumed on a new connection");
                return None;
            }
            Notice::ReconnectFailed(e) => {
                crate::log!("QAP session on {sys}: reconnect failed: {e}");
                return None;
            }
            Notice::SignOnNeeded(m) => (
                qsa_msg::message(id::QAP_SESSION_EXPIRED, &[sys]),
                m,
                ["Sign On...", "Close"],
                Offer::SignOn,
            ),
            Notice::SessionEnded {
                by_operator: true,
                message,
            } => (
                qsa_msg::message(id::SESSION_ENDED_BY_OPERATOR, &[sys]),
                message,
                ["Sign On...", "Close"],
                Offer::SignOn,
            ),
            Notice::SessionEnded {
                by_operator: false,
                message,
            } => (
                qsa_msg::message(id::QAP_SESSION_EXPIRED, &[sys]),
                message,
                ["Sign On...", "Close"],
                Offer::SignOn,
            ),
            Notice::ServerEnding(m) => (
                qsa_msg::message(id::QAP_SERVER_ENDING, &[sys]),
                m,
                ["Reconnect", "Close"],
                Offer::Reconnect,
            ),
        };
    report(ui, &context);
    let choice = match &server {
        Some(sm) => {
            crate::log!("{}: {}", sm.id, sm.text);
            msgdetails::ask_system_message(parent, Some(&context), &shown(sm), &buttons)
        }
        None => msgdetails::ask_message(parent, &context, &buttons),
    };
    (choice == 0).then_some(offer)
}

/// After `ServerEnding`: connect again, then sign on (the server's end
/// ended the session).
fn reconnect(ui: &Ui, system: &System) -> bool {
    let h = client(&ui.state, system);
    let r = busy(|| lock(&h).reconnect().map(|_| ()));
    match r {
        Ok(()) => crate::signon::run(Some(&ui.window), &ui.state, &system.name),
        Err(e) => {
            let x = explain(system, &e);
            report(ui, &x.message);
            show(Some(&ui.window), &x);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system() -> System {
        let mut s = System::new("qs400.example.net");
        s.ports.qap = 2399;
        s
    }

    fn refused(status: Status, id: &str, text: &str) -> Error {
        Error::Status {
            status,
            message: Some(ServerMessage {
                id: id.into(),
                severity: 40,
                data: Vec::new(),
                text: text.into(),
                help: "Cause . . . : as the system says.".into(),
            }),
        }
    }

    #[test]
    fn the_target_is_the_qap_port_and_the_fixed_address() {
        let t = target_for(&system());
        assert_eq!((t.host.as_str(), t.port), ("qs400.example.net", 2399));
        let mut s = system();
        s.address_lookup = AddressLookup::Never;
        s.fixed_ip = "10.42.70.150".into();
        assert_eq!(target_for(&s).host, "10.42.70.150");
        assert_eq!(System::new("x").ports.qap, 2320);
    }

    #[test]
    fn a_refused_sign_on_keeps_the_system_message_as_sent() {
        let x = explain(
            &system(),
            &refused(
                Status::SignOnFailed,
                "CPF1107",
                "Password not correct for user profile.",
            ),
        );
        assert_eq!(x.message.id, id::SIGNON_FAILED);
        assert_eq!(
            x.message.text,
            "Sign-on to qs400.example.net failed: CPF1107: Password not correct for user profile."
        );
        let sm = x.server.as_ref().unwrap();
        assert_eq!(sm.help, "Cause . . . : as the system says.");
        assert_eq!(x.line(), "CPF1107: Password not correct for user profile.");
    }

    #[test]
    fn a_validation_program_refusal_is_shown_as_is() {
        let e = refused(
            Status::Error,
            "CPF22C6",
            "Password validation program failed.",
        );
        let x = explain(&system(), &e);
        assert_eq!(x.line(), "CPF22C6: Password validation program failed.");
        assert_eq!(x.server.unwrap().id, "CPF22C6");
    }

    #[test]
    fn expiry_within_the_warning_period() {
        const DAY: i64 = 86_400_000;
        let now = 1_793_000_000_000;
        let s = |expires: Option<i64>, warning_days| PasswordStatus {
            expires,
            warning_days,
        };
        assert_eq!(expires_within(&s(None, 7), now), None);
        assert_eq!(expires_within(&s(Some(now + 3 * DAY), 7), now), Some(3));
        assert_eq!(expires_within(&s(Some(now + 3 * DAY - 1), 7), now), Some(3));
        assert_eq!(expires_within(&s(Some(now + 7 * DAY), 7), now), Some(7));
        assert_eq!(expires_within(&s(Some(now + 7 * DAY + 1), 7), now), None);
        assert_eq!(expires_within(&s(Some(now - DAY), 7), now), Some(0));
        assert_eq!(expires_within(&s(Some(now + DAY), 0), now), None);
        assert_eq!(
            expiry_message("WARN", "sys1", 3).text,
            "Password for WARN on sys1 expires in 3 days."
        );
    }

    #[test]
    fn a_version_2_function_on_a_version_1_system_is_qad5001() {
        let x = explain(
            &system(),
            &Error::Unsupported {
                op: Op::RunCommand,
                needs: 2,
                has: 1,
            },
        );
        assert_eq!(x.message.id, id::QAP_VERSION_NEEDED);
        assert_eq!(
            x.message.text,
            "Run Command needs QAP version 2; system qs400.example.net has version 1."
        );
        assert!(x.server.is_none());
    }

    #[test]
    fn client_side_failures_are_qad_messages() {
        let s = system();
        let ids = |e: Error| explain(&s, &e).message.id;
        assert_eq!(
            ids(Error::Connect(ConnectError::Refused("h".into()))),
            id::QAP_NOT_REACHED
        );
        assert_eq!(
            explain(&s, &Error::Connect(ConnectError::Refused("h".into())))
                .message
                .text,
            "QAP server on qs400.example.net port 2399 cannot be reached: connection refused."
        );
        assert_eq!(
            ids(Error::Connect(ConnectError::NotFound("h".into()))),
            id::HOST_NOT_FOUND
        );
        assert_eq!(
            ids(Error::Connect(ConnectError::Tls(TlsError::Handshake(
                "peer is incompatible".into()
            )))),
            id::QAP_TLS_FAILED
        );
        assert_eq!(
            ids(Error::Status {
                status: Status::VersionUnsupported,
                message: None
            }),
            id::QAP_VERSION_UNSUPPORTED
        );
        assert_eq!(
            ids(Error::Closed(Closed::ServerEnding)),
            id::QAP_SERVER_ENDING
        );
        assert_eq!(ids(Error::ReplyTooLarge), id::REPLY_TOO_LARGE);
        assert_eq!(
            explain(&s, &Error::ReplyTooLarge).message.text,
            "Reply from qs400.example.net was larger than the program allows."
        );
        assert_eq!(function_name(Op::ReadSpooledFile), "Printer Output");
        assert_eq!(
            ids(Error::Status {
                status: Status::SessionExpired,
                message: None
            }),
            id::QAP_SESSION_EXPIRED
        );
    }
}
