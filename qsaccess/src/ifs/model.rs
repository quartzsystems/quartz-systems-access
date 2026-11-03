// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! What the Integrated File System window lists: a folder's entries with
//! their links resolved, as rows of cells, sorted within those loaded.

use qap_client::Files;
use qap_client::Status;
use qap_client::ifs::join;
use qap_client::proto::ops::ifs::{Entry, EntryKind};
use qsa_ui::listtable::Cell;

/// The list's columns.
pub const COLUMNS: [(&str, i32); 6] = [
    ("Name", 250),
    ("Type", 100),
    ("Size", 86),
    ("Modified", 116),
    ("Owner", 80),
    ("CCSID", 54),
];

/// What a symbolic link names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// The target's entry, at its absolute path.
    To(String, Entry),
    /// The target is not there (`GetInfo` of it is `NotFound`).
    Broken(String),
    /// The target could not be looked at (not authorized, ...).
    Unknown(String),
}

/// One entry of the folder shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Its path.
    pub path: String,
    pub entry: Entry,
    /// For a symbolic link, what it names.
    pub link: Option<Target>,
}

impl Row {
    /// An entry of `dir`, a link's target looked at with `GetInfo` (a link
    /// is not followed by the listing).
    pub fn of<F: Files + ?Sized>(f: &mut F, dir: &str, entry: Entry) -> Row {
        let path = join(dir, &entry.name);
        let link = match (&entry.kind, &entry.target) {
            (EntryKind::Symlink, Some(t)) => {
                let target = if t.starts_with('/') {
                    t.clone()
                } else {
                    join(dir, t)
                };
                Some(match f.get_info(&target) {
                    Ok(e) => Target::To(target, e),
                    Err(e) if e.status() == Some(Status::NotFound) => Target::Broken(target),
                    Err(_) => Target::Unknown(target),
                })
            }
            _ => None,
        };
        Row { path, entry, link }
    }

    /// Whether it opens as a folder: one, or a link to one.
    pub fn is_folder(&self) -> bool {
        match &self.link {
            Some(Target::To(_, e)) => e.kind == EntryKind::Directory,
            Some(_) => false,
            None => self.entry.kind == EntryKind::Directory,
        }
    }

    /// Whether it is a folder itself (not a link to one): deleted with its
    /// contents.
    pub fn is_real_folder(&self) -> bool {
        self.entry.kind == EntryKind::Directory
    }

    /// Whether it is a link whose target is not there.
    pub fn broken(&self) -> bool {
        matches!(self.link, Some(Target::Broken(_)))
    }

    /// The path and entry it opens as: a link's target, else itself.
    pub fn opens_as(&self) -> Option<(String, Entry)> {
        match &self.link {
            Some(Target::To(p, e)) => Some((p.clone(), e.clone())),
            Some(_) => None,
            None => Some((self.path.clone(), self.entry.clone())),
        }
    }

    /// What the Type column says.
    pub fn type_text(&self) -> String {
        match &self.link {
            Some(Target::To(_, e)) if e.kind == EntryKind::Directory => "Link to folder".into(),
            Some(Target::To(..)) => "Link to file".into(),
            Some(Target::Broken(_)) => "Broken link".into(),
            Some(Target::Unknown(_)) => "Link".into(),
            None => match self.entry.kind {
                EntryKind::File => "File".into(),
                EntryKind::Directory => "Folder".into(),
                EntryKind::Symlink => "Link".into(),
                EntryKind::Other => "Other".into(),
            },
        }
    }

    /// What the Name column says: a link's name with its target.
    pub fn name_text(&self) -> String {
        match &self.link {
            Some(Target::To(t, _) | Target::Broken(t) | Target::Unknown(t)) => {
                format!("{} -> {t}", self.entry.name)
            }
            None => self.entry.name.clone(),
        }
    }

    /// Its cells; a broken link's in red.
    pub fn cells(&self) -> Vec<Cell> {
        let e = &self.entry;
        // A link's size is its target's, for a file.
        let size = match &self.link {
            Some(Target::To(_, t)) if t.kind != EntryKind::Directory => size_text(t.size),
            Some(_) => String::new(),
            None if self.is_folder() => String::new(),
            None => size_text(e.size),
        };
        let ccsid = match &self.link {
            Some(Target::To(_, t)) if t.kind != EntryKind::Directory => t.ccsid.to_string(),
            Some(_) => String::new(),
            None if e.ccsid == 0 || self.is_real_folder() => String::new(),
            None => e.ccsid.to_string(),
        };
        let mut cells: Vec<Cell> = vec![
            Cell::from(self.name_text()),
            Cell::from(self.type_text()),
            Cell::from(size),
            Cell::from(modified_text(e.modified)),
            Cell::from(e.owner.as_str()),
            Cell::from(ccsid),
        ];
        if self.broken() {
            for c in &mut cells {
                c.status = qsa_ui::Status::Error;
            }
        }
        cells
    }

    /// Its glyph in the list.
    pub fn glyph(&self) -> qsa_ui::Glyph {
        if self.is_folder() {
            qsa_ui::Glyph::Folder
        } else {
            qsa_ui::Glyph::Document
        }
    }
}

/// `n` with thousands separated by commas.
pub fn grouped(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A size as the list shows it: kilobytes rounded up, as a file manager
/// does (`0 KB` only for an empty file).
pub fn size_text(n: u64) -> String {
    format!("{} KB", grouped(n.div_ceil(1024)))
}

/// A size in words for messages: `512 bytes`, `1.5 KiB`, `200.0 MiB`.
pub fn bytes_text(n: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if n < 1024 {
        return format!("{n} bytes");
    }
    let mut v = n as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}

/// Milliseconds since 1970 as local date and time.
pub fn modified_text(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    crate::localtime::format(ms.div_euclid(1000))
}

/// Sorts `rows` by column `col` (as [`COLUMNS`]), folders before files
/// whichever way.
pub fn sort(rows: &mut [Row], col: usize, ascending: bool) {
    rows.sort_by(|a, b| {
        let by = match col {
            1 => a.type_text().cmp(&b.type_text()),
            2 => a.entry.size.cmp(&b.entry.size),
            3 => a.entry.modified.cmp(&b.entry.modified),
            4 => a.entry.owner.cmp(&b.entry.owner),
            5 => a.entry.ccsid.cmp(&b.entry.ccsid),
            _ => a
                .entry
                .name
                .to_lowercase()
                .cmp(&b.entry.name.to_lowercase())
                .then_with(|| a.entry.name.cmp(&b.entry.name)),
        };
        let by = if ascending { by } else { by.reverse() };
        b.is_folder().cmp(&a.is_folder()).then(by)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, kind: EntryKind, size: u64) -> Entry {
        Entry {
            name: name.into(),
            kind,
            size,
            modified: 1_793_000_000_000,
            owner: "BOB".into(),
            ccsid: 1208,
            target: None,
        }
    }

    fn row(name: &str, kind: EntryKind, size: u64) -> Row {
        Row {
            path: format!("/home/BOB/{name}"),
            entry: entry(name, kind, size),
            link: None,
        }
    }

    #[test]
    fn sizes() {
        assert_eq!(size_text(0), "0 KB");
        assert_eq!(size_text(1), "1 KB");
        assert_eq!(size_text(1024), "1 KB");
        assert_eq!(size_text(200 << 20), "204,800 KB");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(bytes_text(512), "512 bytes");
        assert_eq!(bytes_text(1536), "1.5 KiB");
        assert_eq!(bytes_text(200 << 20), "200.0 MiB");
    }

    #[test]
    fn links_and_their_cells() {
        let mut l = row("link1208", EntryKind::Symlink, 0);
        l.link = Some(Target::To(
            "/home/BOB/text/text1208.txt".into(),
            entry("text1208.txt", EntryKind::File, 80),
        ));
        assert_eq!(l.name_text(), "link1208 -> /home/BOB/text/text1208.txt");
        assert_eq!(l.type_text(), "Link to file");
        assert!(!l.is_folder() && !l.broken());
        assert_eq!(l.opens_as().unwrap().0, "/home/BOB/text/text1208.txt");
        let mut b = row("broken", EntryKind::Symlink, 0);
        b.link = Some(Target::Broken("/home/BOB/gone.txt".into()));
        assert!(b.broken() && b.opens_as().is_none());
        assert!(b.cells().iter().all(|c| c.status == qsa_ui::Status::Error));
        assert_eq!(b.type_text(), "Broken link");
        let d = row("many", EntryKind::Directory, 4096);
        let c = d.cells();
        assert_eq!(
            (c[1].text.as_str(), c[2].text.as_str(), c[5].text.as_str()),
            ("Folder", "", "")
        );
        assert!(c.iter().all(|c| c.status == qsa_ui::Status::Neutral));
    }

    #[test]
    fn folders_first_then_by_column() {
        let mut rows = vec![
            row("b.txt", EntryKind::File, 10),
            row("Z", EntryKind::Directory, 0),
            row("a.txt", EntryKind::File, 30),
            row("c", EntryKind::Directory, 0),
        ];
        sort(&mut rows, 0, true);
        let names: Vec<&str> = rows.iter().map(|r| r.entry.name.as_str()).collect();
        assert_eq!(names, ["c", "Z", "a.txt", "b.txt"]);
        sort(&mut rows, 2, false);
        let names: Vec<&str> = rows.iter().map(|r| r.entry.name.as_str()).collect();
        assert_eq!(&names[2..], ["a.txt", "b.txt"]);
        assert!(rows[0].is_folder() && rows[1].is_folder());
    }
}
