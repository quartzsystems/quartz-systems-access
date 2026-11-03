// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Folders on the system's integrated file system, walked by this program:
//! the system's `Delete` takes only an empty directory and its `Copy` only
//! a file, so a folder is counted, deleted and copied here, an entry at a
//! time.
//!
//! - [`count`]: the files and folders under a folder, for a confirmation
//!   that says how many before anything is deleted.
//! - [`delete_tree`]: depth-first, each folder's entries before the folder.
//!   It stops at the first entry the system refuses, with the system's
//!   message (a file checked out to another user is `InUse`, `CPFA09E`);
//!   what was deleted before stays deleted.
//! - [`copy_tree`]: the folders made and each file copied by the system's
//!   `Copy`, so no data passes through this computer.
//!
//! A symbolic link is an entry like a file: deleted or copied as itself,
//! never followed into.
//!
//! [`transfer`] moves one file either way, binary or text, with the
//! existing-file policy, a partial download kept beside its target, and
//! resume; [`lines`] changes line endings for text.

pub mod lines;
pub mod transfer;

pub use transfer::{
    Choice, Conflict, Direction, Failed, Failure, Job, Options, Outcome, Policy, Resume,
};

use qap_client::Error;
use qap_client::ifs::{Files, join};
use qap_client::proto::ops::ifs::{Entry, EntryKind};

/// What is under a folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Files, links and other entries that are not folders.
    pub files: u64,
    /// Folders under it, itself not counted.
    pub folders: u64,
    /// The files' bytes.
    pub bytes: u64,
}

/// Where a walk stopped: the entry and the system's refusal.
#[derive(Debug)]
pub struct Stopped {
    pub at: String,
    pub error: Box<Error>,
}

/// The entries of `dir`, the names `.` and `..` left out.
fn entries<F: Files + ?Sized>(f: &mut F, dir: &str) -> Result<Vec<Entry>, Error> {
    Ok(f.all_entries(dir, "")?
        .into_iter()
        .filter(|e| e.name != "." && e.name != "..")
        .collect())
}

/// Counts what is under the folder `path`.
pub fn count<F: Files + ?Sized>(f: &mut F, path: &str) -> Result<Counts, Stopped> {
    let mut c = Counts::default();
    let mut todo = vec![path.to_owned()];
    while let Some(dir) = todo.pop() {
        let list = entries(f, &dir).map_err(|error| Stopped {
            at: dir.clone(),
            error: Box::new(error),
        })?;
        for e in list {
            if e.kind == EntryKind::Directory {
                c.folders += 1;
                todo.push(join(&dir, &e.name));
            } else {
                c.files += 1;
                c.bytes += e.size;
            }
        }
    }
    Ok(c)
}

/// Deletes the folder `path` and everything in it, depth-first: each
/// folder's entries, then the folder. `done` is told each path deleted.
/// Stops at the first refusal; how many were deleted is in `done`'s calls.
pub fn delete_tree<F: Files + ?Sized>(
    f: &mut F,
    path: &str,
    done: &mut dyn FnMut(&str),
) -> Result<(), Stopped> {
    let list = entries(f, path).map_err(|error| Stopped {
        at: path.to_owned(),
        error: Box::new(error),
    })?;
    for e in list {
        let p = join(path, &e.name);
        if e.kind == EntryKind::Directory {
            delete_tree(f, &p, done)?;
        } else {
            f.delete(&p).map_err(|error| Stopped {
                at: p.clone(),
                error: Box::new(error),
            })?;
            done(&p);
        }
    }
    f.delete(path).map_err(|error| Stopped {
        at: path.to_owned(),
        error: Box::new(error),
    })?;
    done(path);
    Ok(())
}

/// Copies the folder `from` to `to` on the system: `to` made (its parents
/// too), then each folder made and each file copied by the system.
/// `replace` replaces files already there. `done` is told each file
/// copied, by its new path.
pub fn copy_tree<F: Files + ?Sized>(
    f: &mut F,
    from: &str,
    to: &str,
    replace: bool,
    done: &mut dyn FnMut(&str),
) -> Result<(), Stopped> {
    f.create_directory(to, true).map_err(|error| Stopped {
        at: to.to_owned(),
        error: Box::new(error),
    })?;
    let list = entries(f, from).map_err(|error| Stopped {
        at: from.to_owned(),
        error: Box::new(error),
    })?;
    for e in list {
        let (src, dst) = (join(from, &e.name), join(to, &e.name));
        if e.kind == EntryKind::Directory {
            copy_tree(f, &src, &dst, replace, done)?;
        } else {
            f.copy(&src, &dst, replace).map_err(|error| Stopped {
                at: src,
                error: Box::new(error),
            })?;
            done(&dst);
        }
    }
    Ok(())
}
