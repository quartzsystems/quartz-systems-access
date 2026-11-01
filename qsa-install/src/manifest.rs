// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! `install-manifest.toml`: every file, folder, link, registry key and
//! desktop entry an install created, in the order it created them.
//! Uninstall undoes exactly these, last first, and touches nothing else.
//!
//! - A folder is removed only if it is empty by then: whatever someone
//!   else put in it stays, and so does the folder.
//! - A registry key the installer created is removed with everything
//!   under it; a value it set in a key that was already there is removed
//!   on its own.
//! - A cache an install made (the MIME database, say) is removed if the
//!   install brought it into being, and otherwise rebuilt without our
//!   entries.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The manifest's file name, in the install folder.
pub const FILE: &str = "install-manifest.toml";

/// Who an install is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Just the user running the installer.
    User,
    /// Everyone on the computer.
    Machine,
}

/// A registry hive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hive {
    #[serde(rename = "HKCU")]
    CurrentUser,
    #[serde(rename = "HKLM")]
    LocalMachine,
}

/// One thing an install made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Item {
    /// A file.
    File { path: PathBuf },
    /// A folder the install created (it did not exist before).
    Dir { path: PathBuf },
    /// A symbolic link.
    Symlink { path: PathBuf },
    /// A registry key the install created, removed with its subkeys.
    RegKey { hive: Hive, key: String },
    /// A value the install set in a key that already existed.
    RegValue {
        hive: Hive,
        key: String,
        name: String,
    },
    /// A default application set in a `mimeapps.list`.
    MimeDefault {
        file: PathBuf,
        mime: String,
        desktop: String,
    },
    /// A cache folder or file made by a tool the install ran. `created`:
    /// it did not exist before, so uninstall removes it; otherwise
    /// uninstall runs `rebuild` again.
    Generated {
        path: PathBuf,
        created: bool,
        #[serde(default)]
        rebuild: Vec<String>,
    },
}

/// The manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// The product version installed, `1.0.0`.
    pub version: String,
    /// The release, `V1R0M0`.
    pub release: String,
    pub scope: Scope,
    pub dir: PathBuf,
    pub desktop_shortcut: bool,
    pub associations: bool,
    pub start_menu: bool,
    #[serde(rename = "item", default)]
    pub items: Vec<Item>,
}

impl Manifest {
    pub fn new(scope: Scope, dir: &Path) -> Manifest {
        Manifest {
            version: crate::VERSION.into(),
            release: crate::RELEASE.into(),
            scope,
            dir: dir.to_owned(),
            desktop_shortcut: false,
            associations: false,
            start_menu: false,
            items: Vec::new(),
        }
    }

    /// Notes an item, once.
    pub fn add(&mut self, item: Item) {
        if !self.items.contains(&item) {
            self.items.push(item);
        }
    }

    /// Notes a file.
    pub fn file(&mut self, path: &Path) {
        self.add(Item::File {
            path: path.to_owned(),
        });
    }

    /// Creates `dir` and every missing folder above it, noting the ones
    /// made (outermost first).
    pub fn make_dirs(&mut self, dir: &Path) -> std::io::Result<()> {
        let mut missing = Vec::new();
        let mut d = Some(dir);
        while let Some(p) = d {
            if p.as_os_str().is_empty() || p.exists() {
                break;
            }
            missing.push(p.to_owned());
            d = p.parent();
        }
        std::fs::create_dir_all(dir)?;
        for p in missing.into_iter().rev() {
            self.add(Item::Dir { path: p });
        }
        Ok(())
    }

    pub fn to_text(&self) -> String {
        format!(
            "# Everything QS/400 Access for Desktop's installer created. Uninstall\n\
             # removes exactly these, last first. Do not edit.\n\n{}",
            toml::to_string_pretty(self).expect("manifest serialises")
        )
    }

    pub fn parse(text: &str) -> Result<Manifest, String> {
        toml::from_str(text).map_err(|e| e.message().to_owned())
    }

    /// Reads the manifest in an install folder.
    pub fn load(dir: &Path) -> Result<Manifest, String> {
        let p = dir.join(FILE);
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        Manifest::parse(&text)
    }

    /// Writes the manifest into its install folder, and notes it.
    pub fn save(&mut self) -> std::io::Result<()> {
        let p = self.dir.join(FILE);
        self.file(&p);
        let tmp = p.with_extension("toml.new");
        std::fs::write(&tmp, self.to_text())?;
        std::fs::rename(&tmp, &p)
    }
}

/// Removes the files, links and (empty) folders of a manifest, last first.
/// Every other item (registry, MIME defaults, generated caches) is given in
/// turn to `other`, the platform's code. Every failure is collected; the
/// rest still goes.
pub fn remove_files(
    m: &Manifest,
    mut other: impl FnMut(&Item) -> Result<(), String>,
) -> Vec<String> {
    let mut errors = Vec::new();
    for item in m.items.iter().rev() {
        let r = match item {
            Item::File { path } | Item::Symlink { path } => match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("{}: {e}", path.display())),
            },
            Item::Dir { path } => {
                // Only an empty folder goes.
                match std::fs::remove_dir(path) {
                    Ok(()) => Ok(()),
                    Err(_) => Ok(()),
                }
            }
            other_item => other(other_item),
        };
        if let Err(e) = r {
            errors.push(e);
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qsa-manifest-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn round_trip() {
        let mut m = Manifest::new(Scope::User, Path::new("/opt/q"));
        m.file(Path::new("/opt/q/qsaccess"));
        m.add(Item::RegKey {
            hive: Hive::CurrentUser,
            key: r"Software\Classes\.qsd".into(),
        });
        m.add(Item::RegValue {
            hive: Hive::LocalMachine,
            key: r"Software\Classes\.qkm".into(),
            name: String::new(),
        });
        m.add(Item::MimeDefault {
            file: "/home/u/.config/mimeapps.list".into(),
            mime: "application/x-qsaccess-session".into(),
            desktop: "quartz-access.desktop".into(),
        });
        m.add(Item::Generated {
            path: "/home/u/.local/share/mime".into(),
            created: true,
            rebuild: vec![],
        });
        m.file(Path::new("/opt/q/qsaccess"));
        assert_eq!(m.items.len(), 5, "an item is noted once");
        let text = m.to_text();
        assert!(text.contains("kind = \"reg-key\""), "{text}");
        assert!(text.contains("hive = \"HKCU\""), "{text}");
        assert_eq!(Manifest::parse(&text).unwrap(), m);
    }

    #[test]
    fn uninstall_removes_exactly_what_was_noted() {
        let root = temp("rm");
        let mut m = Manifest::new(Scope::User, &root.join("opt/q"));
        m.make_dirs(&root.join("opt/q/icons")).unwrap();
        for f in ["opt/q/qsaccess", "opt/q/icons/a.png"] {
            std::fs::write(root.join(f), "x").unwrap();
            m.file(&root.join(f));
        }
        // Something the user put in the install folder stays, and so does
        // the folder holding it.
        std::fs::write(root.join("opt/q/notes.txt"), "mine").unwrap();
        let gen_dir = root.join("mime");
        std::fs::create_dir_all(gen_dir.join("x")).unwrap();
        m.add(Item::Generated {
            path: gen_dir.clone(),
            created: true,
            rebuild: vec![],
        });
        m.add(Item::RegKey {
            hive: Hive::CurrentUser,
            key: "k".into(),
        });
        let mut others = Vec::new();
        let errors = remove_files(&m, |i| {
            if let Item::Generated { path, .. } = i {
                std::fs::remove_dir_all(path).unwrap();
            }
            others.push(i.clone());
            Ok(())
        });
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(others.len(), 2);
        assert!(matches!(others[0], Item::RegKey { .. }), "last first");
        assert!(!root.join("opt/q/qsaccess").exists());
        assert!(!root.join("opt/q/icons").exists());
        assert!(root.join("opt/q/notes.txt").exists());
        assert!(!gen_dir.exists());
        assert!(root.join("opt").exists(), "a non-empty folder stays");
        assert_eq!(
            m.items[0],
            Item::Dir {
                path: root.join("opt")
            },
            "folders are noted outermost first"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
