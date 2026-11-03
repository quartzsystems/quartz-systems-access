// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! `qsaccess run <file.qdx>`: a saved transfer run without its window.
//!
//! - The session: the running instance's on the request's system, when it
//!   has one (asked for over the per-user instance endpoint, its token
//!   answered on a reply endpoint of this process's), with a connection of
//!   this process's own attached to it. Otherwise the Sign On dialog, and
//!   that session is signed off at the end. Passwords are never saved, so a
//!   run with no running instance always asks for one: fully unattended runs
//!   are not offered.
//! - Progress goes to stderr, and the progress dialog shows it with Cancel;
//!   the summary goes to stdout.
//! - Exit codes ([`Exit`]): 0 done, 1 bad arguments or request file, 2
//!   sign-on cancelled or failed, 3 transfer failed, 4 cancelled.

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use qap_client::{Attached, SqlConnection, Token};
use qsa_config::ConfigDir;
use qsa_transfer::request::{self, Request};

use crate::app::{AppState, Shared};
use crate::qap;

/// How `qsaccess run` ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Done = 0,
    BadInput = 1,
    SignOn = 2,
    Failed = 3,
    Cancelled = 4,
}

/// Hex of `b`.
fn hex(b: &[u8]) -> String {
    use std::fmt::Write as _;
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}

/// A token from its hex; `None` for anything else.
fn token_from_hex(s: &str) -> Option<Token> {
    let b = s.as_bytes();
    if b.len() != 64 {
        return None;
    }
    let mut t = Token::default();
    for (i, pair) in b.chunks(2).enumerate() {
        let v = std::str::from_utf8(pair).ok()?;
        t.0[i] = u8::from_str_radix(v, 16).ok()?;
    }
    Some(t)
}

/// The running instance answers `qsaccess run`'s question: its session's
/// token on `system`, to the reply endpoint `reply` (only one of this
/// user's), or nothing when it has none. The token is never logged.
pub fn answer(state: &Shared, system: &str, reply: &str) {
    let Some(main) = crate::ipc::remembered() else {
        return;
    };
    let Some(ep) = crate::ipc::reply_endpoint(main, reply) else {
        crate::log!(
            "a session request for {system} named a reply endpoint that is not this user's: ignored"
        );
        return;
    };
    let h = state.borrow_mut().qap.signed_on_client(system);
    let token = h.and_then(|h| qap::lock(&h).session_token());
    crate::log!(
        "qsaccess run asked for the session on {system}: {}",
        if token.is_some() {
            "handed over"
        } else {
            "none"
        }
    );
    let mut arg = token.map(|t| hex(&t.0)).unwrap_or_default();
    let _ = ep.send(std::slice::from_ref(&arg));
    // The hex goes as it came.
    // SAFETY: zeroes are valid UTF-8.
    unsafe { arg.as_bytes_mut().fill(b'0') };
}

/// The running instance's session token on `system`, if there is an
/// instance and it has one.
fn borrowed_token(dir: Option<&ConfigDir>, system: &str) -> Option<Token> {
    let main = crate::ipc::Endpoint::for_user(dir.map(ConfigDir::path)).ok()?;
    borrow_session(&main, system)
}

/// The session token on `system` of the instance listening at `main`, if
/// it has one: asked for with [`crate::ipc::SESSION_TOKEN`], answered on a
/// reply endpoint of this process's within 15 seconds.
pub fn borrow_session(main: &crate::ipc::Endpoint, system: &str) -> Option<Token> {
    if !main.is_running() {
        return None;
    }
    // A name of its own for every question: the process ID, then a count.
    static ASKED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = ASKED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let id = u64::from(std::process::id()) * 1000 + u64::from(n % 1000);
    let reply = crate::ipc::Endpoint::from_id(&crate::ipc::reply_name(main, id));
    let server = crate::ipc::Server::bind(&reply).ok()?;
    let (tx, rx) = mpsc::channel::<Vec<String>>();
    let _server = server.spawn(move |args| {
        let _ = tx.send(args);
    });
    let ask = [
        crate::ipc::SESSION_TOKEN.to_owned(),
        system.to_owned(),
        reply.id(),
    ];
    if !main.send(&ask).unwrap_or(false) {
        return None;
    }
    let mut got = rx.recv_timeout(Duration::from_secs(15)).ok()?;
    let t = got.first().and_then(|s| token_from_hex(s));
    for s in &mut got {
        // SAFETY: zeroes are valid UTF-8.
        unsafe { s.as_bytes_mut().fill(b'0') };
    }
    t
}

/// The console of whatever started this program, for stdout and stderr
/// (Windows starts a windowed program without one).
fn attach_console() {
    #[cfg(windows)]
    // SAFETY: a plain Win32 call; it fails harmlessly with no parent console.
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
}

fn say(line: &str) {
    let mut o = std::io::stdout();
    let _ = writeln!(o, "{line}");
    let _ = o.flush();
}

fn tell(line: &str) {
    let mut e = std::io::stderr();
    let _ = writeln!(e, "{line}");
    let _ = e.flush();
}

/// `qsaccess run <file.qdx>`; `args` are those after `run`.
pub fn run(args: &[String], dir: Option<ConfigDir>) -> Exit {
    attach_console();
    let [file] = args else {
        tell("usage: qsaccess run <file.qdx>");
        return Exit::BadInput;
    };
    let path = std::path::absolute(PathBuf::from(file)).unwrap_or_else(|_| PathBuf::from(file));
    let r = match request::load(&path) {
        Ok(r) => r,
        Err(e) => {
            tell(&format!("{}: {e}", path.display()));
            return Exit::BadInput;
        }
    };
    let state = AppState::load(dir.clone()).shared();
    let Some(sys) = state.borrow().systems.get(r.system()).cloned() else {
        tell(&qsa_msg::message(qsa_msg::id::SYSTEM_NOT_CONFIGURED, &[r.system()]).to_string());
        return Exit::BadInput;
    };
    let _app = fltk::app::App::default();
    if let Err(e) = qsa_ui::theme::init() {
        crate::log!("fonts not loaded: {e}");
    }
    // The running instance's session, else a sign-on of this process's.
    let target = qap::target_for(&sys);
    let mut own: Option<qap::Handle> = None;
    let attached = match borrowed_token(dir.as_ref(), &sys.name)
        .and_then(|t| Attached::open(&target, &t).ok())
    {
        Some(a) => {
            tell(&format!(
                "Using the running program's session on {}.",
                sys.name
            ));
            a
        }
        None => {
            let Some(h) = qap::session(None, &state, &sys.name) else {
                tell(&format!("Not signed on to {}.", sys.name));
                return Exit::SignOn;
            };
            let refused = qap::lock(&h).supports(qap_client::proto::Op::Prepare).err();
            if let Some(e) = refused {
                tell(&qap::explain(&sys, &e).line());
                let _ = qap::lock(&h).sign_off();
                return Exit::SignOn;
            }
            let a = match qap::busy(|| qap::lock(&h).attach()) {
                Ok(a) => a,
                Err(e) => {
                    tell(&qap::explain(&sys, &e).line());
                    let _ = qap::lock(&h).sign_off();
                    return Exit::SignOn;
                }
            };
            own = Some(h);
            a
        }
    };
    let p = attached.profile().clone();
    let dates = qsa_sql::DateStyle::job(&p.date_format, &p.date_separator, &p.time_separator);
    let ccsid = match p.ccsid {
        0 | 65535 => attached.hello().system_ccsid,
        c => c,
    };
    let q = SqlConnection::new(attached);
    let name = r
        .file()
        .file_name()
        .map_or_else(|| "transfer".into(), |n| n.to_string_lossy().into_owned());
    tell(&format!("Transfer {name} on {}...", sys.name));
    let exit = go(q, &r, &name, dates, ccsid);
    if let Some(h) = own {
        let _ = qap::busy(|| qap::lock(&h).sign_off());
    }
    exit
}

/// Runs the transfer with the progress dialog; what to exit with.
fn go(q: SqlConnection, r: &Request, name: &str, dates: qsa_sql::DateStyle, ccsid: u16) -> Exit {
    let req = r.clone();
    let result = super::with_progress(None, name, move |progress, cancel| {
        let mut q = q;
        let report = |p: qsa_transfer::Progress| {
            tell(&super::progress_text(p));
            progress(p);
        };
        let out = match &req {
            Request::Download { download: d, .. } => {
                let mut d = d.clone();
                // Unattended: an existing file is replaced, as asked or not.
                if d.existing == qsa_transfer::Existing::Ask {
                    d.existing = qsa_transfer::Existing::Replace;
                }
                qsa_transfer::download::run(&mut q, &d, dates, ccsid, report, cancel).map_err(|f| {
                    match f {
                        qsa_transfer::Failure::Cancelled { .. } => (true, None, String::new()),
                        f => (false, None, failure_text(&f)),
                    }
                })
            }
            Request::Upload { upload: u, .. } => {
                qsa_transfer::upload::run(&mut q, u, dates, ccsid, report, cancel).map_err(|f| {
                    let cancelled = f.stop == qsa_transfer::upload::Stop::Cancelled;
                    (
                        cancelled,
                        Some((f.row, f.line, f.kept, f.resume)),
                        f.reason.clone(),
                    )
                })
            }
        };
        q.close();
        out
    });
    match result {
        Ok(s) => {
            say(&super::complete(name, s.rows, s.elapsed).to_string());
            Exit::Done
        }
        Err((true, at, _)) => {
            say(&super::cancelled(name).to_string());
            if let Some((_, _, kept, Some(x))) = at {
                say(&resumable(name, kept, x));
            }
            Exit::Cancelled
        }
        Err((false, at, why)) => {
            let (row, line) = at.map_or((0, 0), |(r, l, _, _)| {
                (r.unwrap_or(0), l.map_or(0, |l| l as u64))
            });
            say(&super::failed(name, row, line, &r.file().display().to_string(), &why).to_string());
            if let Some((_, _, kept, Some(x))) = at {
                say(&resumable(name, kept, x));
            }
            Exit::Failed
        }
    }
}

fn resumable(name: &str, kept: u64, x: u64) -> String {
    qsa_msg::message(
        qsa_msg::id::TRANSFER_RESUMABLE,
        &[name, &kept.to_string(), &x.to_string()],
    )
    .to_string()
}

fn failure_text(f: &qsa_transfer::Failure) -> String {
    match f.server_message() {
        Some(m) => format!("{}: {}", m.id, m.text),
        None => f.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_as_hex() {
        let t = Token([0xAB; 32]);
        let h = hex(&t.0);
        assert_eq!(h.len(), 64);
        assert_eq!(token_from_hex(&h).unwrap().0, t.0);
        assert!(token_from_hex("").is_none());
        assert!(token_from_hex(&"zz".repeat(32)).is_none());
        assert_eq!(Exit::Cancelled as i32, 4);
    }
}
