// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Install, upgrade, repair and uninstall.
//!
//! - Install checks the payload's checksum before anything else, then
//!   unpacks the files, keeps a copy of the installer in the install folder
//!   as the uninstaller, adds shortcuts and file types, and writes the
//!   manifest. If any step fails, whatever was made so far is removed again.
//! - An install over an existing one is an upgrade: the old install is
//!   undone (its manifest says what it made) and the new one made. User
//!   data is never part of a manifest, so it is kept.
//! - Repair undoes the install and makes it again with the same choices.
//! - Uninstall undoes the install. Settings and system configurations go
//!   only when asked; the Sessions folder never does.

use std::path::{Path, PathBuf};

use crate::manifest::{Manifest, Scope};
use crate::payload::{self, PayloadError};
use crate::plan::{self, Options};

/// Why an install, repair or uninstall did not happen.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the installer payload is damaged")]
    Damaged,
    #[error("administrator rights are needed to install for all users")]
    NeedsAdmin,
    #[error("no install was found in {0}")]
    NotInstalled(PathBuf),
    #[error("{0}")]
    Failed(String),
}

impl Error {
    /// The exit code for this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Damaged => crate::exit::DAMAGED,
            Error::NeedsAdmin => crate::exit::NEEDS_ADMIN,
            Error::NotInstalled(_) => crate::exit::NOT_INSTALLED,
            Error::Failed(_) => crate::exit::FAILED,
        }
    }

    /// The `QAD` message for this error, when it has one.
    pub fn message(&self) -> Option<qsa_msg::Message> {
        match self {
            Error::Damaged => Some(qsa_msg::message(qsa_msg::id::PAYLOAD_DAMAGED, &[])),
            Error::NeedsAdmin => Some(qsa_msg::message(qsa_msg::id::ADMIN_NEEDED, &[])),
            _ => None,
        }
    }
}

impl From<PayloadError> for Error {
    fn from(e: PayloadError) -> Error {
        match e {
            PayloadError::Damaged | PayloadError::Missing => Error::Damaged,
            PayloadError::Io(e) => Error::Failed(e.to_string()),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Failed(e.to_string())
    }
}

/// What an install did, beyond succeeding.
#[derive(Debug, Default)]
pub struct Report {
    /// Things the user should know (a tool missing, a folder not on PATH).
    pub warnings: Vec<String>,
    /// The release replaced, for an upgrade.
    pub upgraded_from: Option<String>,
    /// The install folder.
    pub dir: PathBuf,
}

impl Report {
    /// The `QAD` message that says how it went.
    pub fn message(&self) -> qsa_msg::Message {
        match &self.upgraded_from {
            Some(old) => qsa_msg::message(qsa_msg::id::UPGRADED, &[old, crate::RELEASE]),
            None => qsa_msg::message(qsa_msg::id::INSTALLED, &[&self.dir.display().to_string()]),
        }
    }
}

/// Progress: how far (0 to 1) and what is being done.
pub type Progress<'a> = &'a mut dyn FnMut(f32, &str);

/// Checks the installer's payload. Done first, before anything is changed.
pub fn check(installer: &Path) -> Result<payload::Footer, Error> {
    Ok(payload::verify(installer)?)
}

/// Undoes everything a manifest lists, last first. Problems are returned;
/// the rest is still undone.
pub fn undo(m: &Manifest) -> Vec<String> {
    #[cfg(windows)]
    {
        crate::windows::remove(m)
    }
    #[cfg(target_os = "linux")]
    {
        crate::linux::remove(m)
    }
}

fn integrate(opts: &Options, m: &mut Manifest, warnings: &mut Vec<String>) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        crate::windows::integrate(opts, m, warnings)
    }
    #[cfg(target_os = "linux")]
    {
        crate::linux::integrate(opts, m, warnings)
    }
}

/// Installs from the installer file at `installer`, or upgrades the
/// install already in `opts.dir`.
pub fn install(installer: &Path, opts: &Options, progress: Progress) -> Result<Report, Error> {
    let footer = check(installer)?;
    if opts.scope == Scope::Machine && !plan::is_admin() {
        return Err(Error::NeedsAdmin);
    }
    let mut report = Report {
        dir: opts.dir.clone(),
        ..Report::default()
    };
    if let Some(old) = plan::existing_in(&opts.dir) {
        progress(0.05, "Removing the previous version");
        crate::log(&format!(
            "upgrading {} ({}) in {}",
            old.release,
            old.version,
            opts.dir.display()
        ));
        for e in undo(&old) {
            crate::log(&format!("upgrade: {e}"));
        }
        report.upgraded_from = Some(old.release.clone());
    }
    let mut m = Manifest::new(opts.scope, &opts.dir);
    m.start_menu = opts.start_menu;
    m.desktop_shortcut = opts.desktop_shortcut;
    m.associations = opts.associations;
    let result = (|| -> Result<(), Error> {
        progress(0.1, "Creating the install folder");
        m.make_dirs(&opts.dir)?;
        let files = payload::list(installer, &footer)?;
        // The folders inside the install folder are made, and noted, before
        // the files in them: uninstall goes last first, so a folder is only
        // tried once its files are gone.
        let mut dirs: Vec<PathBuf> = files
            .iter()
            .filter_map(|(p, _)| p.parent().map(|d| opts.dir.join(d)))
            .filter(|d| d != &opts.dir)
            .collect();
        dirs.sort();
        dirs.dedup();
        for d in dirs {
            m.make_dirs(&d)?;
        }
        let total = files.len().max(1) as f32;
        let mut done = 0.0;
        payload::extract(installer, &footer, &opts.dir, |inner, to| {
            done += 1.0;
            progress(
                0.1 + 0.6 * done / total,
                &format!("Copying {}", inner.display()),
            );
            m.file(to);
        })?;
        progress(0.75, "Keeping the uninstaller");
        let un = opts.dir.join(crate::uninstaller_name());
        let tmp = un.with_extension("qsa-new");
        std::fs::copy(installer, &tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }
        payload::replace(&tmp, &un)?;
        m.file(&un);
        progress(0.8, "Adding shortcuts and file types");
        integrate(opts, &mut m, &mut report.warnings)?;
        progress(0.95, "Writing the install manifest");
        m.save()?;
        Ok(())
    })();
    if let Err(e) = result {
        crate::log(&format!("install failed: {e}; removing what was made"));
        for u in undo(&m) {
            crate::log(&format!("rollback: {u}"));
        }
        return Err(e);
    }
    progress(1.0, "Done");
    crate::log(&report.message().to_string());
    Ok(report)
}

/// Removes the install in `dir`. With `remove_settings`, the user's
/// settings and system configurations go too.
pub fn uninstall(dir: &Path, remove_settings: bool) -> Result<Vec<String>, Error> {
    let m = Manifest::load(dir).map_err(|_| Error::NotInstalled(dir.to_owned()))?;
    let mut problems = undo(&m);
    if remove_settings {
        problems.extend(remove_user_settings());
    }
    for p in &problems {
        crate::log(&format!("uninstall: {p}"));
    }
    crate::log(&qsa_msg::message(qsa_msg::id::REMOVED, &[]).to_string());
    Ok(problems)
}

/// Makes the install in `dir` again, with the choices it was made with.
/// `installer` must not be inside `dir`: it is read after the install is
/// undone.
pub fn repair(installer: &Path, dir: &Path, progress: Progress) -> Result<Report, Error> {
    check(installer)?;
    let m = Manifest::load(dir).map_err(|_| Error::NotInstalled(dir.to_owned()))?;
    let opts = Options::from_manifest(&m);
    let mut report = install(installer, &opts, progress)?;
    report.upgraded_from = None;
    crate::log(&qsa_msg::message(qsa_msg::id::REPAIRED, &[]).to_string());
    Ok(report)
}

/// Removes this user's settings and system configurations. The Sessions
/// folder is never touched. Passwords an earlier release saved in the
/// platform credential store, if the program never ran since to remove
/// them, go as well.
pub fn remove_user_settings() -> Vec<String> {
    let mut problems = Vec::new();
    let Some(dir) = qsa_config::ConfigDir::default_location() else {
        return problems;
    };
    let forgotten = qsa_config::legacy::forget_saved_passwords();
    if forgotten.removed > 0 {
        crate::log(&format!(
            "{} entries of an earlier release removed from the credential store",
            forgotten.removed
        ));
    }
    if dir.path().exists()
        && let Err(e) = std::fs::remove_dir_all(dir.path())
    {
        problems.push(format!("{}: {e}", dir.path().display()));
    }
    crate::log(&format!("settings removed from {}", dir.path().display()));
    problems
}

/// A copy of `exe` in the temporary folder, for running while the original
/// is removed or replaced.
pub fn temp_copy(exe: &Path) -> std::io::Result<PathBuf> {
    // Not "setup" or "install" in the name: Windows takes such names for
    // installers that want administrator rights.
    let to = std::env::temp_dir().join(format!(
        "qsa-maintenance-{}{}",
        std::process::id(),
        if cfg!(windows) { ".exe" } else { "" }
    ));
    std::fs::copy(exe, &to)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(to)
}
