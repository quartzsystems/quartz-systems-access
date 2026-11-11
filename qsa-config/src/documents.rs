// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The one folder in the user's Documents folder that everything the
//! program saves there goes under: [`FOLDER`], with `Sessions`, `Printer
//! Output` and `Screens` in it.
//!
//! Earlier releases used two other folders ([`EARLIER`]): session files and
//! printer output in one, printed screens in another. [`adopt_earlier`]
//! moves what is in them into the one folder, once, and [`reroot`] gives
//! the new place of a path that was kept in settings or in a session file.

use std::path::{Component, Path, PathBuf};

/// The folder's name. A file name cannot hold `/`, so the product's name is
/// written without it, as the Start menu shortcut's is.
pub const FOLDER: &str = "QS400 Access for Desktop";

/// The folders earlier releases used, beside [`FOLDER`] in Documents.
pub const EARLIER: [&str; 2] = ["Quartz Access", "QS400 Access"];

/// The user's Documents folder.
///
/// - Windows: the Documents known folder, so it follows Documents wherever
///   it has been moved, OneDrive included; else `%USERPROFILE%\Documents`.
/// - Linux: `$XDG_DOCUMENTS_DIR`, else `~/Documents`.
pub fn user_documents() -> Option<PathBuf> {
    directories::UserDirs::new().map(|u| {
        u.document_dir()
            .map(Path::to_owned)
            .unwrap_or_else(|| u.home_dir().join("Documents"))
    })
}

/// [`FOLDER`] in the user's Documents folder. It may not exist yet.
pub fn folder() -> Option<PathBuf> {
    user_documents().map(|d| d.join(FOLDER))
}

/// Where Print Screen writes: `Screens` in [`folder`].
pub fn screens_dir() -> Option<PathBuf> {
    folder().map(|d| d.join("Screens"))
}

/// What [`adopt_earlier`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Adopted {
    /// What was moved: from, to. A folder moved whole is one entry.
    pub moved: Vec<(PathBuf, PathBuf)>,
    /// What was left where it was, and why.
    pub left: Vec<(PathBuf, String)>,
}

impl Adopted {
    /// A line for the log for each thing moved or left.
    pub fn log_lines(&self) -> Vec<String> {
        let moved = self
            .moved
            .iter()
            .map(|(from, to)| format!("moved {} to {}", from.display(), to.display()));
        let left = self
            .left
            .iter()
            .map(|(p, why)| format!("left {} where it was: {why}", p.display()));
        moved.chain(left).collect()
    }
}

/// Moves what the [`EARLIER`] folders of `documents` hold into [`FOLDER`]
/// there, and removes each earlier folder once it is empty.
///
/// A folder or file with no namesake in the new place is moved whole (a
/// rename: nothing is copied). Two folders of the same name are merged the
/// same way, entry by entry. A file whose name is taken in the new place is
/// left where it was, and so is anything that cannot be moved; neither is
/// ever overwritten or deleted. With no earlier folder there is nothing to
/// do, so this costs two looks at the disk on every start after the first.
pub fn adopt_earlier(documents: &Path) -> Adopted {
    let mut done = Adopted::default();
    let to = documents.join(FOLDER);
    for earlier in EARLIER {
        let from = documents.join(earlier);
        if from.is_dir() {
            merge(&from, &to, &mut done);
        }
    }
    done
}

/// Moves `from` to `to`, or into it when both are folders.
fn merge(from: &Path, to: &Path, done: &mut Adopted) {
    if !to.exists() {
        let made = match to.parent() {
            Some(p) => std::fs::create_dir_all(p),
            None => Ok(()),
        };
        match made.and_then(|()| std::fs::rename(from, to)) {
            Ok(()) => done.moved.push((from.to_owned(), to.to_owned())),
            Err(e) => done.left.push((from.to_owned(), e.to_string())),
        }
        return;
    }
    if !(from.is_dir() && to.is_dir()) {
        done.left.push((
            from.to_owned(),
            format!("{} is there already", to.display()),
        ));
        return;
    }
    let entries = match std::fs::read_dir(from) {
        Ok(e) => e,
        Err(e) => {
            done.left.push((from.to_owned(), e.to_string()));
            return;
        }
    };
    for entry in entries.flatten() {
        merge(&entry.path(), &to.join(entry.file_name()), done);
    }
    // Gone if everything in it moved; kept, with what was left, if not.
    let _ = std::fs::remove_dir(from);
}

/// Whether two path components are the same name: ignoring case on
/// Windows, where file names do.
fn same(a: Component, b: Component) -> bool {
    if cfg!(windows) {
        a.as_os_str().eq_ignore_ascii_case(b.as_os_str())
    } else {
        a == b
    }
}

/// `path` below `base`, if it is there.
fn below<'a>(path: &'a Path, base: &Path) -> Option<&'a Path> {
    let mut rest = path.components();
    for b in base.components() {
        if !same(rest.next()?, b) {
            return None;
        }
    }
    Some(rest.as_path())
}

/// Where a path in one of `documents`' [`EARLIER`] folders is now: the
/// same path under [`FOLDER`]. `None` for a path anywhere else.
pub fn reroot(path: &Path, documents: &Path) -> Option<PathBuf> {
    EARLIER.iter().find_map(|earlier| {
        below(path, &documents.join(earlier)).map(|rest| documents.join(FOLDER).join(rest))
    })
}

/// A path kept from an earlier release as it is to be used now: its place
/// under [`FOLDER`] when it was in an earlier folder and is no longer
/// there, else the path itself.
pub fn current(path: &Path) -> PathBuf {
    match user_documents().and_then(|d| reroot(path, &d)) {
        Some(new) if !path.exists() => new,
        _ => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qsa-documents-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn the_folders_are_under_documents() {
        if let (Some(docs), Some(f), Some(s)) = (user_documents(), folder(), screens_dir()) {
            assert_eq!(f, docs.join("QS400 Access for Desktop"));
            assert_eq!(s, f.join("Screens"));
        }
    }

    #[test]
    fn earlier_folders_move_whole_into_the_one_folder() {
        let docs = temp("whole");
        put(&docs.join("Quartz Access/Sessions/Payroll.qsd"), "a");
        put(&docs.join("Quartz Access/Printer Output/PRT01/1.pdf"), "b");
        put(&docs.join("QS400 Access/Screens/sys1 2026.pdf"), "c");
        put(&docs.join("Other/keep.txt"), "d");

        let done = adopt_earlier(&docs);
        assert!(done.left.is_empty(), "{done:?}");
        // The first earlier folder is renamed whole; the second merges in.
        assert_eq!(done.moved.len(), 2, "{done:?}");
        let new = docs.join(FOLDER);
        assert_eq!(
            fs::read_to_string(new.join("Sessions/Payroll.qsd")).unwrap(),
            "a"
        );
        assert_eq!(
            fs::read_to_string(new.join("Printer Output/PRT01/1.pdf")).unwrap(),
            "b"
        );
        assert_eq!(
            fs::read_to_string(new.join("Screens/sys1 2026.pdf")).unwrap(),
            "c"
        );
        assert!(!docs.join("Quartz Access").exists());
        assert!(!docs.join("QS400 Access").exists());
        assert!(docs.join("Other/keep.txt").exists());
        assert_eq!(done.log_lines().len(), 2);

        // Nothing to do the next time.
        assert_eq!(adopt_earlier(&docs), Adopted::default());
        let _ = fs::remove_dir_all(&docs);
    }

    #[test]
    fn a_name_that_is_taken_is_left_and_nothing_is_overwritten() {
        let docs = temp("taken");
        let new = docs.join(FOLDER);
        put(&new.join("Sessions/Payroll.qsd"), "new");
        put(&docs.join("Quartz Access/Sessions/Payroll.qsd"), "old");
        put(&docs.join("Quartz Access/Sessions/Stock.qsd"), "stock");
        put(&docs.join("Quartz Access/Sessions/Macros/a.qsm"), "macro");

        let done = adopt_earlier(&docs);
        assert_eq!(
            fs::read_to_string(new.join("Sessions/Payroll.qsd")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(new.join("Sessions/Stock.qsd")).unwrap(),
            "stock"
        );
        assert_eq!(
            fs::read_to_string(new.join("Sessions/Macros/a.qsm")).unwrap(),
            "macro"
        );
        // The file whose name was taken stays, in its folder.
        let kept = docs.join("Quartz Access/Sessions/Payroll.qsd");
        assert_eq!(fs::read_to_string(&kept).unwrap(), "old");
        assert_eq!(done.left.len(), 1, "{done:?}");
        assert_eq!(done.left[0].0, kept);
        assert_eq!(done.moved.len(), 2, "{done:?}");
        let _ = fs::remove_dir_all(&docs);
    }

    #[test]
    fn paths_in_earlier_folders_are_rerooted() {
        let docs = Path::new("/home/bob/Documents");
        assert_eq!(
            reroot(
                Path::new("/home/bob/Documents/Quartz Access/Sessions/a.qsd"),
                docs
            ),
            Some(PathBuf::from(
                "/home/bob/Documents/QS400 Access for Desktop/Sessions/a.qsd"
            ))
        );
        assert_eq!(
            reroot(Path::new("/home/bob/Documents/QS400 Access/Screens"), docs),
            Some(PathBuf::from(
                "/home/bob/Documents/QS400 Access for Desktop/Screens"
            ))
        );
        assert_eq!(
            reroot(Path::new("/home/bob/Documents/Quartz Access"), docs),
            Some(PathBuf::from(
                "/home/bob/Documents/QS400 Access for Desktop"
            ))
        );
        // The one folder itself, a folder that only starts the same, and
        // anywhere else.
        for other in [
            "/home/bob/Documents/QS400 Access for Desktop/Sessions/a.qsd",
            "/home/bob/Documents/Quartz Access 2/a.qsd",
            "/home/bob/Documents/a.qsd",
            "/srv/Quartz Access/a.qsd",
        ] {
            assert_eq!(reroot(Path::new(other), docs), None, "{other}");
        }
    }
}
