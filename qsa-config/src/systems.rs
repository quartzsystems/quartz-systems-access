// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The list of configured systems.

use serde::{Deserialize, Serialize};

use crate::system::{Invalid, System};

/// Every configured system, kept sorted by name (ignoring case). Names are
/// unique, ignoring case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Systems {
    list: Vec<System>,
}

/// The `systems.toml` version written.
///
/// - 1: the first.
/// - 2: added `port_lookup` and the host-server ports.
/// - 3: QAP replaced the host servers: `ports` holds `telnet`, `telnet_tls`
///   and `qap`. Reading an older file keeps its Telnet ports, gives QAP
///   its default port, and drops `port_lookup` and the host-server ports,
///   which nothing uses any more.
/// - 4: passwords are never saved, so `credential_mode` is gone and
///   `default_user` is only what the Sign On dialog starts with. Reading an
///   older file keeps `default_user` and drops `credential_mode`. A program
///   that reads a file older than 4 deletes the passwords earlier releases
///   saved ([`crate::legacy`]).
pub const VERSION: u32 = 4;

/// On-disk form of `systems.toml`.
#[derive(Serialize, Deserialize)]
pub(crate) struct SystemsFile {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default, rename = "system")]
    pub systems: Vec<System>,
}

fn one() -> u32 {
    1
}

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

impl Systems {
    /// An empty list.
    pub fn new() -> Systems {
        Systems::default()
    }

    /// Builds a list from systems read from a file. Later duplicates replace
    /// earlier ones.
    pub fn from_vec(systems: Vec<System>) -> Systems {
        let mut s = Systems::new();
        for sys in systems {
            s.upsert(sys);
        }
        s
    }

    /// The systems, sorted by name.
    pub fn iter(&self) -> impl Iterator<Item = &System> {
        self.list.iter()
    }

    /// The systems as a slice, sorted by name.
    pub fn as_slice(&self) -> &[System] {
        &self.list
    }

    /// Number of systems.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The system with this name, ignoring case.
    pub fn get(&self, name: &str) -> Option<&System> {
        self.list.iter().find(|s| same(&s.name, name))
    }

    /// Whether a system has this name, ignoring case.
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    fn sort(&mut self) {
        self.list
            .sort_by_key(|s| (s.name.to_ascii_lowercase(), s.name.clone()));
    }

    /// Adds a new system after validating it.
    pub fn add(&mut self, mut system: System) -> Result<(), Invalid> {
        system.name = system.name.trim().to_owned();
        system.validate()?;
        if self.contains(&system.name) {
            return Err(Invalid::NameTaken(system.name));
        }
        self.list.push(system);
        self.sort();
        Ok(())
    }

    /// Replaces the system now named `old_name` with `system`, which may
    /// have a new name, after validating it.
    pub fn change(&mut self, old_name: &str, mut system: System) -> Result<(), Invalid> {
        system.name = system.name.trim().to_owned();
        system.validate()?;
        if !same(old_name, &system.name) && self.contains(&system.name) {
            return Err(Invalid::NameTaken(system.name));
        }
        self.list.retain(|s| !same(&s.name, old_name));
        self.list.push(system);
        self.sort();
        Ok(())
    }

    /// Checks `system` as it would be saved under `old_name` (or as new when
    /// `old_name` is `None`), without changing the list.
    pub fn check(&self, old_name: Option<&str>, system: &System) -> Result<(), Invalid> {
        system.validate()?;
        let renamed = old_name.is_none_or(|old| !same(old, &system.name));
        if renamed && self.contains(&system.name) {
            return Err(Invalid::NameTaken(system.name.trim().to_owned()));
        }
        Ok(())
    }

    /// Adds or replaces a system by name, without validation. Used for
    /// systems read from files.
    pub fn upsert(&mut self, mut system: System) {
        system.name = system.name.trim().to_owned();
        self.list.retain(|s| !same(&s.name, &system.name));
        self.list.push(system);
        self.sort();
    }

    /// Removes the system with this name.
    pub fn remove(&mut self, name: &str) -> Option<System> {
        let i = self.list.iter().position(|s| same(&s.name, name))?;
        Some(self.list.remove(i))
    }

    pub(crate) fn to_file(&self) -> SystemsFile {
        SystemsFile {
            version: VERSION,
            systems: self.list.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sys(name: &str) -> System {
        System::new(name)
    }

    #[test]
    fn sorted_and_unique_ignoring_case() {
        let mut s = Systems::new();
        s.add(sys("beta")).unwrap();
        s.add(sys("Alpha")).unwrap();
        s.add(sys("gamma")).unwrap();
        let names: Vec<_> = s.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "beta", "gamma"]);
        assert_eq!(s.add(sys("ALPHA")), Err(Invalid::NameTaken("ALPHA".into())));
        assert!(s.get("BETA").is_some());
    }

    #[test]
    fn change_and_rename() {
        let mut s = Systems::new();
        s.add(sys("a")).unwrap();
        s.add(sys("b")).unwrap();
        let mut a = s.get("a").unwrap().clone();
        a.description = "changed".into();
        s.change("a", a.clone()).unwrap();
        assert_eq!(s.get("a").unwrap().description, "changed");
        // Same name in another case is allowed for the same system.
        a.name = "A".into();
        s.change("a", a.clone()).unwrap();
        assert_eq!(s.len(), 2);
        // Renaming onto another system is not.
        a.name = "b".into();
        assert_eq!(s.change("A", a), Err(Invalid::NameTaken("b".into())));
        assert!(s.check(None, &sys("c")).is_ok());
        assert!(s.check(Some("b"), &sys("B")).is_ok());
        assert!(s.check(None, &sys("b")).is_err());
    }

    #[test]
    fn version_2_files_keep_their_telnet_ports_and_get_the_qap_default() {
        let old = "version = 2

[[system]]
name = \"old.example\"
use_tls = true
port_lookup = \"fixed\"

[system.ports]
telnet = 2323
telnet_tls = 9992
server_mapper = 449
signon = 8476
signon_tls = 19476
";
        let f: SystemsFile = toml::from_str(old).unwrap();
        assert_eq!(f.version, 2);
        let s = Systems::from_vec(f.systems);
        let sys = s.get("old.example").unwrap();
        assert_eq!(sys.ports.telnet, 2323);
        assert_eq!(sys.ports.telnet_tls, 9992);
        assert_eq!(sys.ports.qap, crate::system::QAP_PORT);
        assert!(sys.use_tls);
        let written = toml::to_string(&s.to_file()).unwrap();
        assert!(written.starts_with("version = 4"), "{written}");
        assert!(!written.contains("port_lookup"), "{written}");
        assert!(!written.contains("signon"), "{written}");
        assert!(written.contains("qap = 2320"), "{written}");
    }

    #[test]
    fn version_3_files_keep_the_default_user_and_drop_the_credential_mode() {
        let old = "version = 3

[[system]]
name = \"v3.example\"
credential_mode = \"prompt-once\"
default_user = \"DEVUSR\"

[[system]]
name = \"every.example\"
credential_mode = \"prompt-every-time\"
";
        let f: SystemsFile = toml::from_str(old).unwrap();
        assert_eq!(f.version, 3);
        let s = Systems::from_vec(f.systems);
        assert_eq!(s.get("v3.example").unwrap().default_user, "DEVUSR");
        assert_eq!(s.get("every.example").unwrap().default_user, "");
        let written = toml::to_string(&s.to_file()).unwrap();
        assert!(written.starts_with("version = 4"), "{written}");
        assert!(!written.contains("credential_mode"), "{written}");
        assert!(written.contains("default_user = \"DEVUSR\""), "{written}");
    }

    #[test]
    fn version_1_files_read_with_the_defaults() {
        let old = "version = 1

[[system]]
name = \"older.example\"
";
        let f: SystemsFile = toml::from_str(old).unwrap();
        let s = Systems::from_vec(f.systems);
        assert_eq!(
            s.get("older.example").unwrap().ports,
            crate::Ports::default()
        );
    }

    #[test]
    fn remove_and_upsert() {
        let mut s = Systems::from_vec(vec![sys("x"), sys("y"), sys("X")]);
        assert_eq!(s.len(), 2);
        assert_eq!(s.remove("x").unwrap().name, "X");
        assert!(s.remove("x").is_none());
    }
}
