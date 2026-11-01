// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Printing on Linux: the rendered PDF goes to CUPS with
//! `lp -d <printer> -t <title> <file>`; printers come from `lpstat`.
//!
//! Without the `lp` and `lpstat` commands, system printing is unavailable
//! ([`CupsError::Unavailable`]); a printer session then keeps saving PDFs.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// System printing failed.
#[derive(Debug, thiserror::Error)]
pub enum CupsError {
    /// The commands are not installed.
    #[error("the {0} command was not found")]
    Unavailable(&'static str),
    /// A command ran and failed.
    #[error("{0}")]
    Failed(String),
}

/// `name` on the `PATH`, if it is there.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// Whether `lp` and `lpstat` are both there.
pub fn available() -> Result<(), CupsError> {
    for cmd in ["lp", "lpstat"] {
        if which(cmd).is_none() {
            return Err(CupsError::Unavailable(cmd));
        }
    }
    Ok(())
}

fn run(cmd: &'static str, args: &[&str]) -> Result<String, CupsError> {
    let program = which(cmd).ok_or(CupsError::Unavailable(cmd))?;
    let out = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| CupsError::Failed(format!("{cmd}: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(CupsError::Failed(if err.is_empty() {
            format!("{cmd} ended with {}", out.status)
        } else {
            err
        }))
    }
}

/// Printer names from `lpstat -e` (one per line).
pub fn parse_destinations(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The default printer from `lpstat -d`.
pub fn parse_default(out: &str) -> Option<String> {
    out.lines()
        .find_map(|l| l.trim().strip_prefix("system default destination:"))
        .map(|d| d.trim().to_owned())
        .filter(|d| !d.is_empty())
}

/// The printers CUPS offers.
pub fn printers() -> Result<Vec<String>, CupsError> {
    run("lpstat", &["-e"]).map(|o| parse_destinations(&o))
}

/// The default printer, if one is set.
pub fn default_printer() -> Option<String> {
    run("lpstat", &["-d"]).ok().and_then(|o| parse_default(&o))
}

/// The arguments `lp` gets.
pub fn lp_args(file: &Path, printer: Option<&str>, title: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();
    if let Some(p) = printer.map(str::trim).filter(|p| !p.is_empty()) {
        args.push("-d".into());
        args.push(p.into());
    }
    args.push("-t".into());
    args.push(title.into());
    args.push("--".into());
    args.push(file.into());
    args
}

/// Sends a PDF to `printer` (the default when `None`). Returns what `lp`
/// said (the request ID).
pub fn print_pdf(file: &Path, printer: Option<&str>, title: &str) -> Result<String, CupsError> {
    let program = which("lp").ok_or(CupsError::Unavailable("lp"))?;
    let out = Command::new(program)
        .args(lp_args(file, printer, title))
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| CupsError::Failed(format!("lp: {e}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(CupsError::Failed(if err.is_empty() {
            format!("lp ended with {}", out.status)
        } else {
            err
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lpstat_output() {
        assert_eq!(
            parse_destinations("Office_Laser\nPDF\n\n"),
            vec!["Office_Laser".to_owned(), "PDF".to_owned()]
        );
        assert_eq!(
            parse_default("system default destination: Office_Laser\n").as_deref(),
            Some("Office_Laser")
        );
        assert_eq!(parse_default("no system default destination\n"), None);
    }

    #[test]
    fn lp_arguments() {
        let a = lp_args(Path::new("/tmp/-job.pdf"), Some(" Laser "), "PRT01 job 3");
        let a: Vec<String> = a.iter().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(
            a,
            ["-d", "Laser", "-t", "PRT01 job 3", "--", "/tmp/-job.pdf"]
        );
        // The default printer: no -d.
        let a = lp_args(Path::new("x.pdf"), Some(""), "t");
        assert_eq!(a.len(), 4);
    }

    #[test]
    fn a_missing_command_is_unavailable() {
        assert!(matches!(
            run("qsa-no-such-command", &[]).unwrap_err(),
            CupsError::Unavailable(_)
        ));
    }
}
