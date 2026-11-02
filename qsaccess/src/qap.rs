// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! QAP sessions in the application: one [`Client`] per system, kept in the
//! application state while signed on; the network calls run off the GUI
//! thread ([`busy`]); the client's errors as `QAD` messages, with the
//! system's own message kept as it was sent; and [`pump`], which shows
//! `SessionEnded` and `ServerEnding` and offers to sign on or reconnect.

use std::sync::Arc;

use fltk::app;
use fltk::prelude::*;
use qap_client::proto::{Op, ServerMessage};
use qap_client::{Client, Closed, Error, Notice, Status, Target};
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

/// The system's client, taken out of the state for a call: the signed-on
/// one, or a new one for the system as it is configured now.
pub fn take(state: &Shared, system: &System) -> Client {
    match state.borrow_mut().qap.remove(&key(&system.name)) {
        Some(c) if c.is_signed_on() => c,
        _ => Client::new(target_for(system)),
    }
}

/// Puts a client back. One that is not signed on is dropped: it holds
/// nothing worth keeping.
pub fn keep(state: &Shared, system: &str, client: Client) {
    if client.is_signed_on() {
        state.borrow_mut().qap.insert(key(system), client);
    }
}

/// The profile signed on to `system`, if any.
pub fn signed_on(state: &Shared, system: &str) -> Option<String> {
    let st = state.borrow();
    let c = st.qap.get(&key(system))?;
    c.profile().map(|p| p.profile.clone())
}

/// What a version-2 function is called where the user sees it, for
/// `QAD5001`.
pub fn function_name(op: Op) -> &'static str {
    match op {
        Op::RunCommand => "Run Command",
        Op::CheckCommand => "Check",
        Op::DescribeCommand => "Prompt",
        Op::PasswordStatus => "The password expiry warning",
        Op::TelnetTicket => "Automatic sign-on",
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
        e => m(id::UNEXPECTED_REPLY, &["QAP", sys, &e.to_string()]),
    };
    Explained {
        message,
        server: e.server_message().cloned(),
    }
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

/// Signs off from the selected system, if signed on.
pub fn sign_off(ui: &Ui, system: &str) {
    let Some(mut c) = ui.state.borrow_mut().qap.remove(&key(system)) else {
        return;
    };
    let r = busy(|| c.sign_off());
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

/// Looks at every signed-on client: takes in the server's events, lets a
/// quietly closed connection resume, and tells the user what ended.
pub fn pump(ui: &Ui) {
    let names: Vec<String> = ui.state.borrow().qap.keys().cloned().collect();
    for k in names {
        let Some(mut c) = ui.state.borrow_mut().qap.remove(&k) else {
            continue;
        };
        // Reconnecting reaches the network; do it off the GUI thread.
        let quiet = c.connection().is_none_or(|n| n.closed().is_some());
        let notices = if quiet { busy(|| c.poll()) } else { c.poll() };
        let system = ui
            .state
            .borrow()
            .systems
            .get(&k)
            .cloned()
            .unwrap_or_else(|| System::new(&c.target().host));
        let mut offer = None;
        for n in notices {
            offer = offer.or(tell(ui, &system, n));
        }
        if c.is_signed_on() {
            ui.state.borrow_mut().qap.insert(k, c);
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
    let mut c = take(&ui.state, system);
    let r = busy(|| c.reconnect().map(|_| ()));
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
        assert_eq!(
            ids(Error::Status {
                status: Status::SessionExpired,
                message: None
            }),
            id::QAP_SESSION_EXPIRED
        );
    }
}
