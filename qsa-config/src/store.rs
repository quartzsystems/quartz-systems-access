// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Where settings live, and reading and writing them safely.
//!
//! Writes are atomic: the new contents go to a temporary file beside the
//! target, are flushed to disk, and are then renamed over it. A reader never
//! sees half a file.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::settings::Settings;
use crate::systems::{Systems, SystemsFile};

/// Settings file name.
pub const SETTINGS_FILE: &str = "settings.toml";
/// Systems file name.
pub const SYSTEMS_FILE: &str = "systems.toml";
/// Suffix given to a file that could not be read, when it is moved aside.
pub const UNREADABLE_SUFFIX: &str = "unreadable";

/// A file that exists but could not be read. The caller shows `QAD0030`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadError {
    pub path: PathBuf,
    pub reason: String,
}

impl ReadError {
    /// The `QAD0030` message for this error.
    pub fn message(&self) -> qsa_msg::Message {
        let path = self.path.display().to_string();
        qsa_msg::message(qsa_msg::id::SETTINGS_UNREADABLE, &[&path, &self.reason])
    }
}

/// The settings directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDir {
    dir: PathBuf,
}

impl ConfigDir {
    /// The per-user location:
    /// `%APPDATA%\Quartz Systems\Access` on Windows, and
    /// `$XDG_CONFIG_HOME/quartz-access` (else `~/.config/quartz-access`)
    /// elsewhere.
    pub fn default_location() -> Option<ConfigDir> {
        let base = directories::BaseDirs::new()?;
        let root = base.config_dir();
        let dir = if cfg!(windows) {
            root.join("Quartz Systems").join("Access")
        } else {
            root.join("quartz-access")
        };
        Some(ConfigDir { dir })
    }

    /// A settings directory at an explicit path (tests, portable use).
    pub fn at(dir: impl Into<PathBuf>) -> ConfigDir {
        ConfigDir { dir: dir.into() }
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// Path of `settings.toml`.
    pub fn settings_path(&self) -> PathBuf {
        self.dir.join(SETTINGS_FILE)
    }

    /// Path of `systems.toml`.
    pub fn systems_path(&self) -> PathBuf {
        self.dir.join(SYSTEMS_FILE)
    }

    /// Reads `settings.toml`. A missing file gives defaults; an unreadable
    /// one gives defaults and the error.
    pub fn load_settings(&self) -> (Settings, Option<ReadError>) {
        load::<Settings>(&self.settings_path())
    }

    /// Writes `settings.toml` atomically.
    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        save::<Settings>(&self.settings_path(), settings)
    }

    /// Reads `systems.toml`. A missing file gives an empty list; an
    /// unreadable one gives an empty list and the error.
    pub fn load_systems(&self) -> (Systems, Option<ReadError>) {
        let (file, err) = load::<SystemsFile>(&self.systems_path());
        let systems = file
            .map(|f| Systems::from_vec(f.systems))
            .unwrap_or_default();
        (systems, err)
    }

    /// Writes `systems.toml` atomically.
    pub fn save_systems(&self, systems: &Systems) -> io::Result<()> {
        save::<SystemsFile>(&self.systems_path(), &systems.to_file())
    }
}

/// The folder session files are kept in unless the user picks another:
/// `Quartz Access/Sessions` in the user's Documents folder. It may not
/// exist yet.
///
/// - Windows: the Documents known folder, so it follows Documents wherever
///   it has been moved, OneDrive included; else
///   `%USERPROFILE%\Documents`.
/// - Linux: `$XDG_DOCUMENTS_DIR`, else `~/Documents`.
pub fn default_sessions_dir() -> Option<PathBuf> {
    let documents = directories::UserDirs::new().map(|u| {
        u.document_dir()
            .map(Path::to_owned)
            .unwrap_or_else(|| u.home_dir().join("Documents"))
    });
    documents.map(|d| d.join("Quartz Access").join("Sessions"))
}

/// Reads and parses a TOML file. `None` (or the default) for a missing file.
fn load<T: DeserializeOwned + LoadDefault>(path: &Path) -> (T::Out, Option<ReadError>) {
    match fs::read_to_string(path) {
        Ok(text) => match toml::from_str::<T>(&text) {
            Ok(v) => (T::some(v), None),
            Err(e) => (
                T::none(),
                Some(ReadError {
                    path: path.to_owned(),
                    reason: e.message().to_owned(),
                }),
            ),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => (T::none(), None),
        Err(e) => (
            T::none(),
            Some(ReadError {
                path: path.to_owned(),
                reason: e.to_string(),
            }),
        ),
    }
}

/// What a load yields for each file type.
trait LoadDefault: Sized {
    type Out;
    fn some(v: Self) -> Self::Out;
    fn none() -> Self::Out;
}

impl LoadDefault for Settings {
    type Out = Settings;
    fn some(v: Settings) -> Settings {
        v
    }
    fn none() -> Settings {
        Settings::default()
    }
}

impl LoadDefault for SystemsFile {
    type Out = Option<SystemsFile>;
    fn some(v: SystemsFile) -> Option<SystemsFile> {
        Some(v)
    }
    fn none() -> Option<SystemsFile> {
        None
    }
}

/// Serialises and writes atomically. If the file on disk exists but cannot
/// be parsed, it is first moved aside to `<name>.unreadable`, so nothing the
/// user may want to recover is overwritten.
fn save<T: Serialize + DeserializeOwned>(path: &Path, value: &T) -> io::Result<()> {
    let text = toml::to_string_pretty(value).map_err(io::Error::other)?;
    if let Ok(existing) = fs::read_to_string(path)
        && toml::from_str::<T>(&existing).is_err()
    {
        let aside = path.with_extension(format!("toml.{UNREADABLE_SUFFIX}"));
        fs::rename(path, aside)?;
    }
    write_atomic(path, text.as_bytes())
}

/// Writes `bytes` to `path` through a temporary file and a rename. The
/// directory is created if needed. On Unix the file is private to the user.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut f = open_private(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(unix)]
fn open_private(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::system::{AddressLookup, CaCertificate, CredentialMode, System};

    /// A fresh, empty directory under the system temp dir.
    pub fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "qsa-config-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    pub fn full_system(name: &str) -> System {
        let mut s = System::new(name);
        s.description = "Production — main \"site\"".into();
        s.use_tls = true;
        s.credential_mode = CredentialMode::PromptOnce;
        s.default_user = "QSECOFR".into();
        s.address_lookup = AddressLookup::Never;
        s.fixed_ip = "192.0.2.10".into();
        s.ports.database_tls = 19471;
        s.ca_certificates = vec![
            CaCertificate::Path(PathBuf::from("/etc/ssl/quartz-ca.pem")),
            CaCertificate::Pem(
                "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n".into(),
            ),
        ];
        s
    }

    #[test]
    fn systems_round_trip_through_systems_toml() {
        let dir = ConfigDir::at(temp_dir("rt"));
        let mut systems = Systems::new();
        systems.add(full_system("sys1.example.net")).unwrap();
        systems
            .add(System {
                default_user: "U".into(),
                ..System::new("10.1.1.1")
            })
            .unwrap();
        dir.save_systems(&systems).unwrap();
        let (back, err) = dir.load_systems();
        assert_eq!(err, None);
        assert_eq!(back, systems);
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, [SYSTEMS_FILE], "temporary file left behind");
        let _ = fs::remove_dir_all(dir.path());
    }

    #[test]
    fn missing_files_give_defaults() {
        let dir = ConfigDir::at(temp_dir("missing"));
        assert_eq!(dir.load_systems(), (Systems::new(), None));
        assert_eq!(dir.load_settings(), (Settings::default(), None));
        let _ = fs::remove_dir_all(dir.path());
    }

    #[test]
    fn minimal_file_fills_defaults() {
        let dir = ConfigDir::at(temp_dir("min"));
        fs::write(dir.systems_path(), "[[system]]\nname = \"a\"\n").unwrap();
        let (s, err) = dir.load_systems();
        assert_eq!(err, None);
        assert_eq!(s.get("a").unwrap(), &System::new("a"));
        let _ = fs::remove_dir_all(dir.path());
    }

    #[test]
    fn unreadable_file_is_reported_then_kept_aside() {
        let dir = ConfigDir::at(temp_dir("bad"));
        fs::write(dir.systems_path(), "[[system]\nname = ").unwrap();
        let (s, err) = dir.load_systems();
        assert!(s.is_empty());
        let err = err.expect("error reported");
        assert_eq!(err.path, dir.systems_path());
        let m = err.message();
        assert_eq!(m.id, "QAD0030");
        assert!(m.text.contains(SYSTEMS_FILE));
        dir.save_systems(&Systems::new()).unwrap();
        let aside = dir.path().join("systems.toml.unreadable");
        assert_eq!(fs::read_to_string(aside).unwrap(), "[[system]\nname = ");
        assert_eq!(dir.load_systems().1, None);
        let _ = fs::remove_dir_all(dir.path());
    }

    #[test]
    fn settings_round_trip() {
        let dir = ConfigDir::at(temp_dir("settings"));
        let s = Settings {
            last_system: "sys1".into(),
            ..Settings::default()
        };
        dir.save_settings(&s).unwrap();
        assert_eq!(dir.load_settings(), (s, None));
        let _ = fs::remove_dir_all(dir.path());
    }

    #[test]
    fn default_location_matches_platform() {
        if let Some(d) = ConfigDir::default_location() {
            let p = d.path().to_string_lossy().replace('\\', "/");
            if cfg!(windows) {
                assert!(p.ends_with("/Quartz Systems/Access"), "{p}");
            } else {
                assert!(p.ends_with("/quartz-access"), "{p}");
            }
        }
    }

    #[test]
    fn default_sessions_dir_is_under_documents() {
        if let Some(d) = default_sessions_dir() {
            let p = d.to_string_lossy().replace('\\', "/");
            assert!(p.ends_with("/Quartz Access/Sessions"), "{p}");
            // It is under the Documents folder the platform reports, which
            // on Windows is wherever Documents has been moved to.
            if let Some(docs) =
                directories::UserDirs::new().and_then(|u| u.document_dir().map(Path::to_owned))
            {
                assert_eq!(d, docs.join("Quartz Access").join("Sessions"));
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = ConfigDir::at(temp_dir("perm"));
        dir.save_systems(&Systems::new()).unwrap();
        let mode = fs::metadata(dir.systems_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = fs::remove_dir_all(dir.path());
    }
}
