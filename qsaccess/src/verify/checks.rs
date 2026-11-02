// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The checks of Verify Connection, in order, without a user interface:
//! the name, the Telnet port, Telnet's TLS when the system uses it, the QAP
//! port, QAP's TLS 1.3 handshake and certificate, `Hello`, and a test
//! sign-on when a user ID is given (signed off at once). A check that
//! cannot run because an earlier one failed is skipped. Failures on this
//! side are `QAD` messages; the system's own messages are kept as sent.

use std::net::ToSocketAddrs;
use std::time::Duration;

use qap_client::proto::Op;
use qap_client::proto::ops::signon::{SignOn, SignOnReply};
use qap_client::{Connection, Error, Secret, ServerMessage, Status, Target};
use qsa_config::System;
use qsa_msg::{Message, id};
use qsa_net::x509::CertInfo;
use qsa_net::{ConnectError, Security, Tcp, Tls, TlsError, TlsOptions};

/// One check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    Dns,
    Telnet,
    TelnetTls,
    Qap,
    QapTls,
    Hello,
    SignOn,
}

impl Check {
    /// Every check, in the order run.
    pub const ALL: [Check; 7] = [
        Check::Dns,
        Check::Telnet,
        Check::TelnetTls,
        Check::Qap,
        Check::QapTls,
        Check::Hello,
        Check::SignOn,
    ];

    /// The name shown for `system`.
    pub fn label(self, system: &System) -> String {
        let p = &system.ports;
        match self {
            Check::Dns => "Name lookup".into(),
            Check::Telnet => format!("Telnet port {}", p.telnet),
            Check::TelnetTls => format!("Telnet TLS port {}", p.telnet_tls),
            Check::Qap => format!("QAP port {}", p.qap),
            Check::QapTls => "QAP TLS 1.3 and certificate".into(),
            Check::Hello => "QAP Hello".into(),
            Check::SignOn => "Test sign-on".into(),
        }
    }

    /// Whether the connection cannot be called verified when it fails.
    pub fn required(self) -> bool {
        self != Check::SignOn
    }
}

/// A certificate the system presented that is not trusted, for the
/// untrusted-certificate flow.
#[derive(Clone, Debug)]
pub struct Untrusted {
    pub reason: String,
    pub certificate: Vec<u8>,
    pub info: Box<CertInfo>,
}

/// How a check came out.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// Passed; what was found.
    Ok(String),
    /// Failed: this program's message, the system's when it sent one, and
    /// the certificate when trust was the problem.
    Failed {
        message: Message,
        server: Option<Box<ServerMessage>>,
        untrusted: Option<Box<Untrusted>>,
    },
    /// Not run; why.
    Skipped(String),
}

impl Outcome {
    fn failed(message: Message) -> Outcome {
        Outcome::Failed {
            message,
            server: None,
            untrusted: None,
        }
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Outcome::Failed { .. })
    }

    /// The Detail column's text.
    pub fn detail(&self) -> String {
        match self {
            Outcome::Ok(s) | Outcome::Skipped(s) => s.clone(),
            Outcome::Failed {
                message,
                server: Some(sm),
                ..
            } if message.id == id::SIGNON_FAILED => format!("{}: {}", sm.id, sm.text),
            Outcome::Failed { message, .. } => message.to_string(),
        }
    }
}

/// What to verify.
#[derive(Debug)]
pub struct Plan {
    pub system: System,
    /// The QAP target (host, port and trust).
    pub target: Target,
    /// Trust for Telnet's TLS: the system's, like the 5250 sessions'.
    pub telnet_tls: TlsOptions,
    /// A user ID and password for the test sign-on.
    pub sign_on: Option<(String, Secret)>,
    pub timeout: Duration,
}

/// The overall answer: `QAD4050` when every check passed, `QAD4051` when
/// only the optional test sign-on failed, `QAD4052` when a required check
/// failed.
pub fn summary(system: &str, results: &[(Check, Outcome)]) -> Message {
    let failed = |required: bool| {
        results
            .iter()
            .any(|(c, o)| o.is_failed() && c.required() == required)
    };
    let id = if failed(true) {
        id::VERIFY_FAILED
    } else if failed(false) {
        id::VERIFIED_WITH_WARNINGS
    } else {
        id::VERIFIED
    };
    qsa_msg::message(id, &[system])
}

fn tcp_failure(host: &str, e: &ConnectError) -> Message {
    match e {
        ConnectError::NotFound(h) => qsa_msg::message(id::HOST_NOT_FOUND, &[h]),
        ConnectError::Refused(_) => qsa_msg::message(id::CONNECTION_REFUSED, &[host]),
        _ => qsa_msg::message(id::CONNECT_TIMED_OUT, &[host]),
    }
}

/// Why the QAP port could not be reached, for `QAD4053`.
fn qap_reason(e: &ConnectError) -> String {
    match e {
        ConnectError::Refused(_) => "connection refused".into(),
        ConnectError::TimedOut(_) => "no answer in time".into(),
        ConnectError::NotFound(_) => "the name was not found".into(),
        ConnectError::Io { source, .. } => source.to_string(),
        ConnectError::Tls(t) => t.to_string(),
    }
}

fn untrusted(system: &str, e: TlsError, handshake: impl FnOnce(&str) -> Message) -> Outcome {
    match e {
        TlsError::Untrusted {
            reason,
            certificate,
            info,
        } => Outcome::Failed {
            message: qsa_msg::message(id::CERT_UNTRUSTED, &[system, &reason]),
            server: None,
            untrusted: Some(Box::new(Untrusted {
                reason,
                certificate,
                info,
            })),
        },
        TlsError::Handshake(r) => Outcome::failed(handshake(&r)),
    }
}

fn security(s: &Security) -> String {
    match s {
        Security::Tls { protocol, cipher } => format!("{protocol}, {cipher}; certificate trusted"),
        Security::Plain => "no TLS".into(),
    }
}

/// Runs every check, handing each result to `report` as it is known.
pub fn run(plan: &Plan, mut report: impl FnMut(Check, Outcome)) {
    let sys = plan.system.name.as_str();
    let host = plan.target.host.as_str();
    let skip = |why: &str| Outcome::Skipped(why.to_owned());

    // The name.
    let resolved = match (host, 0).to_socket_addrs() {
        Ok(a) => {
            let mut addrs: Vec<String> = a.map(|a| a.ip().to_string()).collect();
            addrs.dedup();
            if addrs.is_empty() {
                None
            } else {
                Some(format!("{host} is {}", addrs.join(", ")))
            }
        }
        Err(_) => None,
    };
    let Some(found) = resolved else {
        report(
            Check::Dns,
            Outcome::failed(qsa_msg::message(id::HOST_NOT_FOUND, &[host])),
        );
        for c in &Check::ALL[1..] {
            report(*c, skip("Not tested: the name was not found."));
        }
        return;
    };
    report(Check::Dns, Outcome::Ok(found));

    // Telnet, and its TLS when the system uses it.
    let p = &plan.system.ports;
    report(
        Check::Telnet,
        match Tcp::connect(host, p.telnet, plan.timeout) {
            Ok(_) => Outcome::Ok(format!("Port {} accepts connections.", p.telnet)),
            Err(e) => Outcome::failed(tcp_failure(host, &e)),
        },
    );
    report(
        Check::TelnetTls,
        if !plan.system.use_tls {
            skip("Not tested: this system's 5250 sessions do not use TLS.")
        } else {
            match Tcp::connect(host, p.telnet_tls, plan.timeout) {
                Err(e) => Outcome::failed(tcp_failure(host, &e)),
                Ok(tcp) => match Tls::handshake(tcp, host, &plan.telnet_tls) {
                    Ok(t) => Outcome::Ok(security(&qsa_net::Transport::security(&t))),
                    Err(e) => untrusted(sys, e, |r| qsa_msg::message(id::TLS_FAILED, &[host, r])),
                },
            }
        },
    );

    // QAP: the port, TLS 1.3, Hello.
    let port = p.qap.to_string();
    let tcp = match Tcp::connect(host, p.qap, plan.timeout) {
        Ok(t) => {
            report(
                Check::Qap,
                Outcome::Ok(format!("Port {} accepts connections.", p.qap)),
            );
            t
        }
        Err(e) => {
            report(
                Check::Qap,
                Outcome::failed(qsa_msg::message(
                    id::QAP_NOT_REACHED,
                    &[sys, &port, &qap_reason(&e)],
                )),
            );
            for c in [Check::QapTls, Check::Hello, Check::SignOn] {
                report(c, skip("Not tested: the QAP port was not reached."));
            }
            return;
        }
    };
    let tls = match Tls::handshake(tcp, host, &plan.target.tls()) {
        Ok(t) => {
            report(
                Check::QapTls,
                Outcome::Ok(security(&qsa_net::Transport::security(&t))),
            );
            t
        }
        Err(e) => {
            report(
                Check::QapTls,
                untrusted(sys, e, |r| qsa_msg::message(id::QAP_TLS_FAILED, &[sys, r])),
            );
            for c in [Check::Hello, Check::SignOn] {
                report(c, skip("Not tested: TLS was not set up."));
            }
            return;
        }
    };
    let conn = match Connection::over(Box::new(tls), &plan.target) {
        Ok(c) => {
            let h = c.hello();
            report(
                Check::Hello,
                Outcome::Ok(format!(
                    "Version {}, system {}, release {}.",
                    h.version, h.system_name, h.release
                )),
            );
            c
        }
        Err(e) => {
            let o = plan.target.options;
            let message = match e.status() {
                Some(Status::VersionUnsupported) => qsa_msg::message(
                    id::QAP_VERSION_UNSUPPORTED,
                    &[sys, &o.min_version.to_string(), &o.max_version.to_string()],
                ),
                _ => qsa_msg::message(id::UNEXPECTED_REPLY, &["QAP", sys, &e.to_string()]),
            };
            report(
                Check::Hello,
                Outcome::Failed {
                    message,
                    server: e.server_message().cloned().map(Box::new),
                    untrusted: None,
                },
            );
            report(Check::SignOn, skip("Not tested: Hello failed."));
            return;
        }
    };

    // The test sign-on, then sign-off.
    let Some((user, password)) = &plan.sign_on else {
        report(Check::SignOn, skip("Not tested: no user ID entered."));
        return;
    };
    let body = SignOn {
        user: user.clone(),
        password: password.clone(),
    };
    report(
        Check::SignOn,
        match conn.call::<_, SignOnReply>(Op::SignOn, &body) {
            Ok(reply) => {
                let off = conn.call::<(), ()>(Op::SignOff, &());
                let m = qsa_msg::message(id::SIGNON_OK, &[sys, &reply.profile]);
                Outcome::Ok(match off {
                    Ok(()) => format!("{} Signed off.", m.text),
                    Err(e) => format!("{} Sign-off: {e}.", m.text),
                })
            }
            Err(e) => sign_on_failure(sys, user, e),
        },
    );
    conn.close();
}

fn sign_on_failure(sys: &str, user: &str, e: Error) -> Outcome {
    let server = e.server_message().cloned().map(Box::new);
    let message = match (&e, &server) {
        (e, _) if e.status() == Some(Status::PasswordExpired) => {
            qsa_msg::message(id::PASSWORD_EXPIRED, &[user, sys])
        }
        (_, Some(sm)) => qsa_msg::message(
            id::SIGNON_FAILED,
            &[
                sys,
                &format!("{}: {}", sm.id, sm.text.trim_end_matches('.')),
            ],
        ),
        (e, None) => qsa_msg::message(id::SIGNON_FAILED, &[sys, &e.to_string()]),
    };
    Outcome::Failed {
        message,
        server,
        untrusted: None,
    }
}

impl Plan {
    /// A plan for `system` as configured.
    pub fn for_system(system: &System, sign_on: Option<(String, Secret)>) -> Plan {
        let target = crate::qap::target_for(system);
        Plan {
            system: system.clone(),
            telnet_tls: TlsOptions {
                trust: crate::trust::for_system(system),
                system_roots: true,
                timeout: target.options.connect_timeout,
                tls13_only: false,
            },
            timeout: target.options.connect_timeout,
            target,
            sign_on,
        }
    }
}
