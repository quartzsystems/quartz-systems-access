// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! What a launch asks for, from its command line:
//!
//! - `qsaccess open <file>...`: open each file (a double-click on a `.qsd`,
//!   `.qkm` or `.qsm` runs this);
//! - `qsaccess --session-manager`: show Session Manager;
//! - `--record <file>`: record the next 5250 session;
//! - `--quit`: close every window and exit (the installer sends this before
//!   an upgrade, once the user has agreed);
//! - `--run <file.qdx>`: `qsaccess run` asks the running instance to run a
//!   saved transfer on its own session, sending back progress, the summary
//!   and the exit code on the same link; only the file's path goes over it;
//! - nothing: show the main window.
//!
//! A second launch hands its request to the running instance over
//! [`crate::ipc`]; relative paths are made absolute first, since the two
//! processes may not share a working directory.

use std::path::PathBuf;

/// A launch's request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub open: Vec<PathBuf>,
    pub session_manager: bool,
    pub record: Option<PathBuf>,
    pub quit: bool,
    /// `--run <file.qdx>`: a saved transfer to run here, for `qsaccess run`.
    pub run: Option<PathBuf>,
    /// Arguments not understood; logged.
    pub unknown: Vec<String>,
}

fn absolute(p: PathBuf) -> PathBuf {
    std::path::absolute(&p).unwrap_or(p)
}

impl Request {
    /// Parses arguments (without the program name). Paths are made
    /// absolute.
    pub fn parse(args: &[String]) -> Request {
        let mut r = Request::default();
        let mut it = args.iter().peekable();
        let mut opening = false;
        while let Some(a) = it.next() {
            match a.as_str() {
                "open" if r.open.is_empty() && !opening => opening = true,
                "--session-manager" => r.session_manager = true,
                "--quit" => r.quit = true,
                "--run" => {
                    if let Some(p) = it.next() {
                        r.run = Some(absolute(PathBuf::from(p)));
                    }
                }
                "--record" => {
                    if let Some(p) = it.next() {
                        r.record = Some(absolute(PathBuf::from(p)));
                    }
                }
                _ if opening && !a.starts_with("--") => {
                    r.open.push(absolute(PathBuf::from(a)));
                }
                _ => r.unknown.push(a.clone()),
            }
        }
        r
    }

    /// The arguments that ask for the same again, for handing to the
    /// running instance.
    pub fn to_args(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.open.is_empty() {
            out.push("open".to_owned());
            out.extend(self.open.iter().map(|p| p.display().to_string()));
        }
        if self.session_manager {
            out.push("--session-manager".to_owned());
        }
        if let Some(p) = &self.record {
            out.push("--record".to_owned());
            out.push(p.display().to_string());
        }
        if self.quit {
            out.push("--quit".to_owned());
        }
        if let Some(p) = &self.run {
            out.push("--run".to_owned());
            out.push(p.display().to_string());
        }
        out
    }

    /// Whether the request opens a window of its own, so the main window
    /// need not be shown for it.
    pub fn opens_window(&self) -> bool {
        !self.open.is_empty() || self.session_manager || self.quit || self.run.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn parses_open_and_flags() {
        let r = Request::parse(&s(&["open", "a.qsd", "b.qkm"]));
        assert_eq!(r.open.len(), 2);
        assert!(r.open.iter().all(|p| p.is_absolute()));
        assert!(r.open[0].ends_with("a.qsd"));
        assert!(r.opens_window());
        let r = Request::parse(&s(&["--session-manager"]));
        assert!(r.session_manager && r.open.is_empty() && r.opens_window());
        assert!(Request::parse(&s(&["--quit"])).quit);
        let r = Request::parse(&s(&["--record", "x.qsr", "--bogus"]));
        assert!(r.record.as_ref().unwrap().is_absolute());
        assert_eq!(r.unknown, ["--bogus"]);
        assert!(!r.opens_window());
        assert_eq!(Request::parse(&[]), Request::default());
    }

    #[test]
    fn round_trips_through_arguments() {
        let r = Request::parse(&s(&[
            "open",
            "a.qsd",
            "--session-manager",
            "--record",
            "r.qsr",
        ]));
        assert_eq!(Request::parse(&r.to_args()), r);
        // A file literally named "open" is a file once opening has begun.
        let r = Request::parse(&s(&["open", "open"]));
        assert_eq!(r.open.len(), 1);
        let r = Request::parse(&s(&["--run", "t.qdx"]));
        assert!(r.run.as_ref().unwrap().is_absolute() && r.opens_window());
        assert_eq!(Request::parse(&r.to_args()), r);
    }
}
