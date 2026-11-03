// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! `qsaccess run <file.qdx>`: a saved transfer run without its window.
//!
//! - With a running instance (the per-user instance endpoint, its user
//!   checked as [`crate::ipc`] does): only `--run` and the request file's
//!   path go to it. The instance runs the transfer on its own session (Sign
//!   On there when it has none) and a connection of its own attached to it,
//!   shows its progress dialog, and sends back one frame per line:
//!   `progress` (to stderr here), `summary` (to stdout) and, last, `exit`
//!   with the code. The session token never leaves the instance.
//! - Without one: the Sign On dialog here, and that session is signed off
//!   at the end. Passwords are never saved, so such a run always asks for
//!   one: fully unattended runs are not offered.
//! - Progress goes to stderr, and the progress dialog shows it with Cancel;
//!   the summary goes to stdout. When this process goes away, a delegated
//!   transfer is cancelled at its next progress line.
//! - Exit codes ([`Exit`]): 0 done, 1 bad arguments or request file, 2
//!   sign-on cancelled or failed, 3 transfer failed, 4 cancelled.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use qap_client::SqlConnection;
use qsa_config::ConfigDir;
use qsa_transfer::request::{self, Request};

use crate::app::{AppState, Shared};
use crate::ipc::{Endpoint, Link};
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

impl Exit {
    /// The exit for `code` as an `exit` frame carries it; anything not
    /// known is a failure.
    pub fn from_code(code: &str) -> Exit {
        match code {
            "0" => Exit::Done,
            "1" => Exit::BadInput,
            "2" => Exit::SignOn,
            "4" => Exit::Cancelled,
            _ => Exit::Failed,
        }
    }
}

/// Where a run's lines go: this process's console, or the link back to the
/// `qsaccess run` that asked for it.
#[derive(Clone)]
pub enum Out {
    Console,
    Link(Arc<Mutex<Option<Link>>>),
}

impl Out {
    fn link(link: Link) -> Out {
        Out::Link(Arc::new(Mutex::new(Some(link))))
    }

    /// One frame on the link; false once it is gone.
    fn send(&self, kind: &str, line: &str) -> bool {
        let Out::Link(l) = self else {
            return true;
        };
        let mut g = l.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(link) = g.as_mut() else {
            return false;
        };
        if link.send(&[kind.to_owned(), line.to_owned()]).is_err() {
            crate::log!("qsaccess run went away; its transfer is cancelled");
            *g = None;
            return false;
        }
        true
    }

    /// A progress line, stderr's; false once nobody reads it (the run that
    /// asked went away).
    pub fn tell(&self, line: &str) -> bool {
        match self {
            Out::Console => {
                let mut e = std::io::stderr();
                let _ = writeln!(e, "{line}");
                let _ = e.flush();
                true
            }
            Out::Link(_) => self.send("progress", line),
        }
    }

    /// A summary line, stdout's.
    pub fn say(&self, line: &str) {
        match self {
            Out::Console => {
                let mut o = std::io::stdout();
                let _ = writeln!(o, "{line}");
                let _ = o.flush();
            }
            Out::Link(_) => {
                self.send("summary", line);
            }
        }
    }

    /// The last frame: the exit code.
    fn exit(&self, exit: Exit) {
        self.send("exit", &(exit as i32).to_string());
    }
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

/// `qsaccess run <file.qdx>`; `args` are those after `run`.
pub fn run(args: &[String], dir: Option<ConfigDir>) -> Exit {
    attach_console();
    let out = Out::Console;
    let [file] = args else {
        out.tell("usage: qsaccess run <file.qdx>");
        return Exit::BadInput;
    };
    let path = std::path::absolute(PathBuf::from(file)).unwrap_or_else(|_| PathBuf::from(file));
    let r = match request::load(&path) {
        Ok(r) => r,
        Err(e) => {
            out.tell(&format!("{}: {e}", path.display()));
            return Exit::BadInput;
        }
    };
    if let Ok(main) = Endpoint::for_user(dir.as_ref().map(ConfigDir::path))
        && let Some(exit) = delegate(&main, &path, &out)
    {
        return exit;
    }
    let state = AppState::load(dir).shared();
    let _app = fltk::app::App::default();
    if let Err(e) = qsa_ui::theme::init() {
        crate::log!("fonts not loaded: {e}");
    }
    run_request(&state, &r, &out, true, true)
}

/// Hands the run of `path` to the instance listening at `main` and relays
/// what it sends back to `out`; `None` when no instance is listening.
pub fn delegate(main: &Endpoint, path: &Path, out: &Out) -> Option<Exit> {
    let mut link = match main.open() {
        Ok(Some(l)) => l,
        Ok(None) => return None,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            out.tell(&e.to_string());
            return Some(Exit::Failed);
        }
        Err(e) => {
            crate::log!("the running program could not be asked: {e}");
            return None;
        }
    };
    if let Err(e) = link.send(&["--run".to_owned(), path.display().to_string()]) {
        crate::log!("the running program could not be asked: {e}");
        return None;
    }
    loop {
        match link.recv() {
            Ok(Some(f)) => match f.as_slice() {
                [k, line] if k == "progress" => {
                    out.tell(line);
                }
                [k, line] if k == "summary" => out.say(line),
                [k, code] if k == "exit" => return Some(Exit::from_code(code)),
                other => crate::log!("qsaccess run: unexpected frame {other:?}"),
            },
            Ok(None) | Err(_) => {
                out.tell("The running program closed the link before the transfer ended.");
                return Some(Exit::Failed);
            }
        }
    }
}

/// The running instance's side of `qsaccess run`: the request at `path`
/// run on this instance's session, its lines and exit code sent back on
/// `link`. `show` puts up the progress dialog.
pub fn serve(state: &Shared, path: &Path, link: Link, show: bool) {
    let out = Out::link(link);
    crate::log!("qsaccess run asked to run {}", path.display());
    let exit = match request::load(path) {
        Ok(r) => run_request(state, &r, &out, false, show),
        Err(e) => {
            out.tell(&format!("{}: {e}", path.display()));
            Exit::BadInput
        }
    };
    out.exit(exit);
}

/// Runs `r` on the session there is on its system, or one signed on now;
/// `own_session` signs that one off at the end.
fn run_request(state: &Shared, r: &Request, out: &Out, own_session: bool, show: bool) -> Exit {
    let Some(sys) = state.borrow().systems.get(r.system()).cloned() else {
        out.tell(&qsa_msg::message(qsa_msg::id::SYSTEM_NOT_CONFIGURED, &[r.system()]).to_string());
        return Exit::BadInput;
    };
    let Some(h) = qap::session(None, state, &sys.name) else {
        out.tell(&format!("Not signed on to {}.", sys.name));
        return Exit::SignOn;
    };
    if !own_session {
        out.tell(&format!(
            "Using the running program's session on {}.",
            sys.name
        ));
    }
    let exit = transfer_on(&h, &sys, r, out, show);
    if own_session {
        let _ = qap::busy(|| qap::lock(&h).sign_off());
    }
    exit
}

/// Runs `r` on a connection of its own attached to `h`'s session.
fn transfer_on(
    h: &qap::Handle,
    sys: &qsa_config::System,
    r: &Request,
    out: &Out,
    show: bool,
) -> Exit {
    let refused = qap::lock(h).supports(qap_client::proto::Op::Prepare).err();
    if let Some(e) = refused {
        out.tell(&qap::explain(sys, &e).line());
        return Exit::SignOn;
    }
    let attached = match qap::busy(|| qap::lock(h).attach()) {
        Ok(a) => a,
        Err(e) => {
            out.tell(&qap::explain(sys, &e).line());
            return Exit::SignOn;
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
    out.tell(&format!("Transfer {name} on {}...", sys.name));
    go(q, r, &name, dates, ccsid, out, show)
}

/// How a transfer stopped short: cancelled or not, the upload's failing
/// row, line, rows kept and resume row, and why.
type Stopped = (
    bool,
    Option<(Option<u64>, Option<usize>, u64, Option<u64>)>,
    String,
);

/// Runs the transfer, with the progress dialog when `show`; what to exit
/// with.
fn go(
    q: SqlConnection,
    r: &Request,
    name: &str,
    dates: qsa_sql::DateStyle,
    ccsid: u16,
    out: &Out,
    show: bool,
) -> Exit {
    let req = r.clone();
    let lines = out.clone();
    let work = move |progress: &dyn Fn(qsa_transfer::Progress), cancel: &AtomicBool| {
        let mut q = q;
        let report = |p: qsa_transfer::Progress| {
            if !lines.tell(&super::progress_text(p)) {
                cancel.store(true, Ordering::SeqCst);
            }
            progress(p);
        };
        let done: Result<qsa_transfer::Summary, Stopped> = match &req {
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
        done
    };
    let result = if show {
        super::with_progress(None, name, work)
    } else {
        work(&|_| {}, &AtomicBool::new(false))
    };
    match result {
        Ok(s) => {
            out.say(&super::complete(name, s.rows, s.elapsed).to_string());
            Exit::Done
        }
        Err((true, at, _)) => {
            out.say(&super::cancelled(name).to_string());
            if let Some((_, _, kept, Some(x))) = at {
                out.say(&resumable(name, kept, x));
            }
            Exit::Cancelled
        }
        Err((false, at, why)) => {
            let (row, line) = at.map_or((0, 0), |(r, l, _, _)| {
                (r.unwrap_or(0), l.map_or(0, |l| l as u64))
            });
            out.say(
                &super::failed(name, row, line, &r.file().display().to_string(), &why).to_string(),
            );
            if let Some((_, _, kept, Some(x))) = at {
                out.say(&resumable(name, kept, x));
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
    fn exit_codes() {
        for e in [
            Exit::Done,
            Exit::BadInput,
            Exit::SignOn,
            Exit::Failed,
            Exit::Cancelled,
        ] {
            assert_eq!(Exit::from_code(&(e as i32).to_string()), e);
        }
        assert_eq!(Exit::from_code("99"), Exit::Failed);
        assert_eq!(Exit::Cancelled as i32, 4);
    }
}
