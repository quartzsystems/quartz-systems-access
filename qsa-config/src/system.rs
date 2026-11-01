// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The system configuration model and its validation.

use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// How a connection gets its user ID and password.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialMode {
    /// Use the default user ID, shared by every connection to the system.
    #[default]
    DefaultUser,
    /// Prompt once, then reuse the answer (and the saved password).
    PromptOnce,
    /// Prompt for every connection; nothing is saved.
    PromptEveryTime,
}

/// When the system's address is looked up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AddressLookup {
    /// Look the name up on every connection.
    #[default]
    Always,
    /// Look the name up at most once a day.
    Daily,
    /// Never look the name up; use the fixed IP address.
    Never,
}

/// How a host server's port is found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PortLookup {
    /// Ask the server mapper (port 449, never TLS); if it cannot be
    /// reached or refuses, use the configured port and say so once.
    #[default]
    MapperThenStandard,
    /// Use the configured ports; never contact the server mapper.
    StandardOnly,
    /// Use the configured ports, which must each be set and distinct.
    Fixed,
}

impl PortLookup {
    /// Whether the server mapper is asked.
    pub fn uses_mapper(self) -> bool {
        self == PortLookup::MapperThenStandard
    }
}

/// The server ports. Each service has a plain port and a TLS port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ports {
    pub telnet: u16,
    pub telnet_tls: u16,
    pub server_mapper: u16,
    pub signon: u16,
    pub signon_tls: u16,
    pub central: u16,
    pub central_tls: u16,
    pub database: u16,
    pub database_tls: u16,
    pub data_queue: u16,
    pub data_queue_tls: u16,
    pub file: u16,
    pub file_tls: u16,
    pub network_print: u16,
    pub network_print_tls: u16,
    pub remote_command: u16,
    pub remote_command_tls: u16,
}

impl Default for Ports {
    fn default() -> Ports {
        Ports {
            telnet: 23,
            telnet_tls: 992,
            server_mapper: 449,
            signon: 8476,
            signon_tls: 9476,
            central: 8470,
            central_tls: 9470,
            database: 8471,
            database_tls: 9471,
            data_queue: 8472,
            data_queue_tls: 9472,
            file: 8473,
            file_tls: 9473,
            network_print: 8474,
            network_print_tls: 9474,
            remote_command: 8475,
            remote_command_tls: 9475,
        }
    }
}

/// One port, for building forms over [`Ports`].
#[derive(Clone, Copy, Debug)]
pub struct PortField {
    /// Label shown to the user.
    pub label: &'static str,
    pub get: fn(&Ports) -> u16,
    pub set: fn(&mut Ports, u16),
}

macro_rules! port_fields {
    ($($label:literal => $field:ident),* $(,)?) => {
        /// Every port, in the order shown on the Ports tab.
        pub const PORT_FIELDS: &[PortField] = &[
            $(PortField {
                label: $label,
                get: |p| p.$field,
                set: |p, v| p.$field = v,
            }),*
        ];
    };
}

port_fields! {
    "Telnet" => telnet,
    "Telnet (TLS)" => telnet_tls,
    "Server mapper" => server_mapper,
    "Sign-on" => signon,
    "Sign-on (TLS)" => signon_tls,
    "Central" => central,
    "Central (TLS)" => central_tls,
    "Database" => database,
    "Database (TLS)" => database_tls,
    "Data queue" => data_queue,
    "Data queue (TLS)" => data_queue_tls,
    "File" => file,
    "File (TLS)" => file_tls,
    "Network print" => network_print,
    "Network print (TLS)" => network_print_tls,
    "Remote command" => remote_command,
    "Remote command (TLS)" => remote_command_tls,
}

/// A trusted CA certificate: a PEM file on disk, or PEM text kept in the
/// configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaCertificate {
    Path(PathBuf),
    Pem(String),
}

impl CaCertificate {
    /// A one-line description for lists.
    pub fn summary(&self) -> String {
        match self {
            CaCertificate::Path(p) => p.display().to_string(),
            CaCertificate::Pem(pem) => {
                let blocks = pem.matches("-----BEGIN CERTIFICATE-----").count();
                format!(
                    "Embedded PEM ({blocks} certificate{})",
                    if blocks == 1 { "" } else { "s" }
                )
            }
        }
    }
}

/// One system configuration. Never holds a password.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct System {
    /// Host name or IP address; unique among systems, ignoring case.
    pub name: String,
    pub description: String,
    pub use_tls: bool,
    pub credential_mode: CredentialMode,
    pub default_user: String,
    pub address_lookup: AddressLookup,
    /// Used when `address_lookup` is `Never`.
    pub fixed_ip: String,
    /// How host server ports are found. Telnet ports always come from
    /// `ports`. Systems saved before this setting existed read as the
    /// default.
    pub port_lookup: PortLookup,
    pub ports: Ports,
    pub ca_certificates: Vec<CaCertificate>,
}

/// A validation failure. The text is shown in the dialog's status line.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Invalid {
    #[error("System name is required.")]
    NameMissing,
    #[error("System name must be a host name or an IP address.")]
    NameNotHost,
    #[error("A system named {0} already exists.")]
    NameTaken(String),
    #[error("Default user is required when the default user is used for sign-on.")]
    DefaultUserMissing,
    #[error("Default user cannot contain spaces.")]
    DefaultUserSpaces,
    #[error("A fixed IP address is required when address lookup is Never.")]
    FixedIpMissing,
    #[error("{0} is not a valid IP address.")]
    FixedIpBad(String),
    #[error("{0} port must be from 1 to 65535.")]
    PortZero(&'static str),
    #[error("{0} and {1} cannot use the same port when ports are fixed.")]
    PortShared(&'static str, &'static str),
    #[error("A CA certificate entry is empty.")]
    CaEmpty,
}

/// Whether `s` is a plausible host name: labels of letters, digits and
/// hyphens, separated by dots, not starting or ending with a hyphen.
fn is_host_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// Checks a system name: a host name or an IP address.
pub fn check_name(name: &str) -> Result<(), Invalid> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Invalid::NameMissing);
    }
    if name.parse::<IpAddr>().is_err() && !is_host_name(name) {
        return Err(Invalid::NameNotHost);
    }
    Ok(())
}

impl System {
    /// A new system with the given name and defaults for everything else.
    pub fn new(name: &str) -> System {
        System {
            name: name.to_owned(),
            ..System::default()
        }
    }

    /// Checks the fields that a system needs on its own. Uniqueness is
    /// checked by [`crate::Systems`].
    pub fn validate(&self) -> Result<(), Invalid> {
        check_name(&self.name)?;
        if self.default_user.contains(char::is_whitespace) {
            return Err(Invalid::DefaultUserSpaces);
        }
        if self.credential_mode == CredentialMode::DefaultUser && self.default_user.is_empty() {
            return Err(Invalid::DefaultUserMissing);
        }
        if self.address_lookup == AddressLookup::Never {
            let ip = self.fixed_ip.trim();
            if ip.is_empty() {
                return Err(Invalid::FixedIpMissing);
            }
            if ip.parse::<IpAddr>().is_err() {
                return Err(Invalid::FixedIpBad(ip.to_owned()));
            }
        }
        for f in PORT_FIELDS {
            if (f.get)(&self.ports) == 0 {
                return Err(Invalid::PortZero(f.label));
            }
        }
        if self.port_lookup == PortLookup::Fixed {
            // The mapper's own port is not used in this mode.
            let used: Vec<&PortField> = PORT_FIELDS
                .iter()
                .filter(|f| f.label != "Server mapper")
                .collect();
            for (i, a) in used.iter().enumerate() {
                for b in &used[i + 1..] {
                    if (a.get)(&self.ports) == (b.get)(&self.ports) {
                        return Err(Invalid::PortShared(a.label, b.label));
                    }
                }
            }
        }
        for ca in &self.ca_certificates {
            let empty = match ca {
                CaCertificate::Path(p) => p.as_os_str().is_empty(),
                CaCertificate::Pem(s) => s.trim().is_empty(),
            };
            if empty {
                return Err(Invalid::CaEmpty);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ports() {
        let p = Ports::default();
        assert_eq!((p.telnet, p.telnet_tls), (23, 992));
        assert_eq!(p.server_mapper, 449);
        assert_eq!((p.signon, p.signon_tls), (8476, 9476));
        assert_eq!((p.central, p.central_tls), (8470, 9470));
        assert_eq!((p.database, p.database_tls), (8471, 9471));
        assert_eq!((p.data_queue, p.data_queue_tls), (8472, 9472));
        assert_eq!((p.file, p.file_tls), (8473, 9473));
        assert_eq!((p.network_print, p.network_print_tls), (8474, 9474));
        assert_eq!((p.remote_command, p.remote_command_tls), (8475, 9475));
        assert_eq!(PORT_FIELDS.len(), 17);
    }

    #[test]
    fn port_fields_read_and_write_their_own_port() {
        let mut p = Ports::default();
        for (i, f) in PORT_FIELDS.iter().enumerate() {
            (f.set)(&mut p, 10000 + i as u16);
        }
        for (i, f) in PORT_FIELDS.iter().enumerate() {
            assert_eq!((f.get)(&p), 10000 + i as u16, "{}", f.label);
        }
    }

    #[test]
    fn defaults() {
        let s = System::new("sys1");
        assert_eq!(s.credential_mode, CredentialMode::DefaultUser);
        assert_eq!(s.address_lookup, AddressLookup::Always);
        assert!(!s.use_tls);
        assert_eq!(s.ports, Ports::default());
    }

    fn ok() -> System {
        System {
            default_user: "QSECOFR".into(),
            ..System::new("sys1.example.net")
        }
    }

    #[test]
    fn validation() {
        assert_eq!(ok().validate(), Ok(()));
        let v = |f: fn(&mut System)| {
            let mut s = ok();
            f(&mut s);
            s.validate()
        };
        assert_eq!(v(|s| s.name.clear()), Err(Invalid::NameMissing));
        assert_eq!(v(|s| s.name = "bad name".into()), Err(Invalid::NameNotHost));
        assert_eq!(v(|s| s.name = "-x".into()), Err(Invalid::NameNotHost));
        assert_eq!(v(|s| s.name = "10.0.0.1".into()), Ok(()));
        assert_eq!(v(|s| s.name = "fe80::1".into()), Ok(()));
        assert_eq!(
            v(|s| s.default_user.clear()),
            Err(Invalid::DefaultUserMissing)
        );
        assert_eq!(
            v(|s| {
                s.default_user.clear();
                s.credential_mode = CredentialMode::PromptEveryTime;
            }),
            Ok(())
        );
        assert_eq!(
            v(|s| s.default_user = "A B".into()),
            Err(Invalid::DefaultUserSpaces)
        );
        assert_eq!(
            v(|s| s.address_lookup = AddressLookup::Never),
            Err(Invalid::FixedIpMissing)
        );
        assert_eq!(
            v(|s| {
                s.address_lookup = AddressLookup::Never;
                s.fixed_ip = "300.1.1.1".into();
            }),
            Err(Invalid::FixedIpBad("300.1.1.1".into()))
        );
        assert_eq!(
            v(|s| {
                s.address_lookup = AddressLookup::Never;
                s.fixed_ip = "192.0.2.7".into();
            }),
            Ok(())
        );
        assert_eq!(v(|s| s.ports.file = 0), Err(Invalid::PortZero("File")));
        // Shared ports are only an error when ports are fixed.
        assert_eq!(v(|s| s.ports.file = 8476), Ok(()));
        assert_eq!(
            v(|s| {
                s.port_lookup = PortLookup::Fixed;
                s.ports.file = 8476;
            }),
            Err(Invalid::PortShared("Sign-on", "File"))
        );
        assert_eq!(v(|s| s.port_lookup = PortLookup::Fixed), Ok(()));
        assert_eq!(
            v(|s| s.ca_certificates.push(CaCertificate::Pem(" ".into()))),
            Err(Invalid::CaEmpty)
        );
    }

    #[test]
    fn ca_summary() {
        let pem = "-----BEGIN CERTIFICATE-----\nA\n-----END CERTIFICATE-----\n";
        assert_eq!(
            CaCertificate::Pem(pem.repeat(2)).summary(),
            "Embedded PEM (2 certificates)"
        );
        assert_eq!(
            CaCertificate::Pem(pem.into()).summary(),
            "Embedded PEM (1 certificate)"
        );
    }
}
