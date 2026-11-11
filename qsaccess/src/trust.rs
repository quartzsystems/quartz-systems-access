// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Certificate trust for every connection the application makes: 5250
//! display and printer sessions, and QAP. What to trust comes
//! from the system configuration ([`for_system`]); a certificate that does
//! not verify goes through one dialog flow ([`ask_and_trust`]), which adds
//! it to the system's CA list when the user says so.

use fltk::prelude::*;
use qsa_config::System;
use qsa_config::system::CaCertificate;
use qsa_msg::id;
use qsa_net::Trust;
use qsa_net::x509::CertInfo;

use crate::app::Shared;
use crate::session::cert;

/// What a system's CA list says to trust: PEM text kept in the
/// configuration, and certificate files (PEM or DER) read now. These are
/// the certificates Certificate Management lists for the system.
pub fn for_system(system: &System) -> Trust {
    let mut ders = Vec::new();
    for ca in &system.ca_certificates {
        match ca {
            CaCertificate::Pem(p) => ders.extend(qsa_net::x509::certificates_in(p.as_bytes())),
            CaCertificate::Path(path) => match std::fs::read(path) {
                Ok(bytes) => ders.extend(qsa_net::x509::certificates_in(&bytes)),
                Err(e) => crate::log!("CA certificate {}: {e}", path.display()),
            },
        }
    }
    Trust::from_der(ders)
}

/// Adds a certificate to the system's CA list and saves the systems.
/// False when the system is gone or the systems could not be saved.
pub fn add(state: &Shared, system: &str, der: &[u8]) -> bool {
    let saved = {
        let mut app = state.borrow_mut();
        let Some(mut sys) = app.systems.get(system).cloned() else {
            return false;
        };
        sys.ca_certificates
            .push(CaCertificate::Pem(qsa_net::tls::to_pem(der)));
        app.systems.upsert(sys);
        app.save_systems()
    };
    // Certificate Management lists it at once.
    crate::certs::changed(state);
    match saved {
        Ok(()) => true,
        Err(e) => {
            crate::log!("systems not saved: {e}");
            false
        }
    }
}

/// The untrusted-certificate flow: shows the certificate, and on "Trust
/// for this system" adds it to the system's CA list. `report` gets
/// `QAD1011` before the dialog and `QAD1012` once trusted. True when the
/// certificate is now trusted, so the caller can connect again.
pub fn ask_and_trust(
    parent: Option<&dyn WindowExt>,
    state: &Shared,
    system: &str,
    reason: &str,
    info: &CertInfo,
    der: &[u8],
    mut report: impl FnMut(&qsa_msg::Message),
) -> bool {
    report(&qsa_msg::message(id::CERT_UNTRUSTED, &[system, reason]));
    if cert::ask(parent, system, reason, info) && add(state, system, der) {
        report(&qsa_msg::message(id::CERT_TRUSTED, &[system]));
        true
    } else {
        false
    }
}
