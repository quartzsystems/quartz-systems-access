// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The system configuration model and its validation.

use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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

/// The ports a system is reached on: Telnet for 5250 display and printer
/// sessions (plain and TLS), and QAP for everything else (always TLS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ports {
    pub telnet: u16,
    pub telnet_tls: u16,
    /// The Quartz Access Protocol port (`docs/qap-protocol-spec.md`).
    pub qap: u16,
}

/// The default QAP port.
pub const QAP_PORT: u16 = 2320;

impl Default for Ports {
    fn default() -> Ports {
        Ports {
            telnet: 23,
            telnet_tls: 992,
            qap: QAP_PORT,
        }
    }
}

impl Ports {
    /// The Telnet port for a system that does or does not use TLS.
    pub fn telnet_for(&self, tls: bool) -> u16 {
        if tls { self.telnet_tls } else { self.telnet }
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
    "QAP" => qap,
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

/// One system configuration. Never holds a password: passwords are never
/// saved anywhere. The program signs on to a system once per run, when a
/// tool first needs it, and keeps that QAP session.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct System {
    /// Host name or IP address; unique among systems, ignoring case.
    pub name: String,
    pub description: String,
    pub use_tls: bool,
    /// The user ID the Sign On dialog starts with; may be empty.
    pub default_user: String,
    pub address_lookup: AddressLookup,
    /// Used when `address_lookup` is `Never`.
    pub fixed_ip: String,
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
    #[error("Default user cannot contain spaces.")]
    DefaultUserSpaces,
    #[error("A fixed IP address is required when address lookup is Never.")]
    FixedIpMissing,
    #[error("{0} is not a valid IP address.")]
    FixedIpBad(String),
    #[error("{0} port must be from 1 to 65535.")]
    PortZero(&'static str),
    #[error("{0} and {1} cannot use the same port.")]
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
        // QAP cannot share a port with Telnet.
        for f in PORT_FIELDS.iter().filter(|f| f.label != "QAP") {
            if (f.get)(&self.ports) == self.ports.qap {
                return Err(Invalid::PortShared(f.label, "QAP"));
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
        assert_eq!((p.telnet, p.telnet_tls, p.qap), (23, 992, 2320));
        assert_eq!((p.telnet_for(false), p.telnet_for(true)), (23, 992));
        assert_eq!(PORT_FIELDS.len(), 3);
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
        assert!(s.default_user.is_empty());
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
        // The default user ID is optional.
        assert_eq!(v(|s| s.default_user.clear()), Ok(()));
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
        assert_eq!(v(|s| s.ports.qap = 0), Err(Invalid::PortZero("QAP")));
        assert_eq!(
            v(|s| s.ports.qap = 992),
            Err(Invalid::PortShared("Telnet (TLS)", "QAP"))
        );
        // The two Telnet ports may be the same; only one is used.
        assert_eq!(v(|s| s.ports.telnet_tls = 23), Ok(()));
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
