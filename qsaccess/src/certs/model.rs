// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! What Certificate Management shows and changes, without its window: the
//! certificates every system's CA list holds, one entry per certificate,
//! and adding a certificate to a list or taking one out.
//!
//! A CA list ([`CaCertificate`]) holds PEM text kept in the configuration
//! and references to certificate files. A certificate is the same one
//! wherever it is kept when its SHA-256 fingerprint is the same.

use std::path::{Path, PathBuf};

use qsa_config::system::CaCertificate;
use qsa_net::x509::{self, CertInfo};

/// Reads a referenced certificate file.
pub type ReadFile<'a> = &'a dyn Fn(&Path) -> std::io::Result<Vec<u8>>;

/// Reads a referenced certificate file from disk.
pub fn read_file(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// A certificate expiring within this many days says how many are left.
pub const SOON_DAYS: i64 = 30;

/// One certificate: its DER and what is shown of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certificate {
    pub der: Vec<u8>,
    pub info: CertInfo,
}

impl Certificate {
    pub fn new(der: Vec<u8>) -> Certificate {
        let info = x509::describe(&der);
        Certificate { der, info }
    }

    /// The certificate as PEM text.
    pub fn pem(&self) -> String {
        qsa_net::tls::to_pem(&self.der)
    }

    /// What messages call the certificate: its subject's common name, or
    /// the whole subject when it has none.
    pub fn name(&self) -> &str {
        common_name(&self.info.subject).unwrap_or(&self.info.subject)
    }

    /// Where the certificate is in its life at `now` (Unix time).
    pub fn validity(&self, now: i64) -> Validity {
        let (Some(from), Some(to)) = (
            x509::unix_time(&self.info.not_before),
            x509::unix_time(&self.info.not_after),
        ) else {
            return Validity::Unknown;
        };
        if now > to {
            Validity::Expired
        } else if now < from {
            Validity::NotYet
        } else {
            let days = (to - now) / 86_400;
            if days <= SOON_DAYS {
                Validity::Soon(days)
            } else {
                Validity::Valid
            }
        }
    }

    /// The Valid to column: the date; `Expired` once it has passed; the
    /// date and the days left when they are few.
    pub fn valid_to(&self, now: i64) -> String {
        let date = date_of(&self.info.not_after);
        match self.validity(now) {
            Validity::Expired => "Expired".to_owned(),
            Validity::Soon(0) => format!("{date} (expires today)"),
            Validity::Soon(1) => format!("{date} (1 day left)"),
            Validity::Soon(n) => format!("{date} ({n} days left)"),
            Validity::Valid | Validity::NotYet | Validity::Unknown => date.to_owned(),
        }
    }
}

/// Where a certificate is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validity {
    Valid,
    /// Valid, with this many whole days left ([`SOON_DAYS`] at most).
    Soon(i64),
    Expired,
    /// Its Valid from date has not come yet.
    NotYet,
    /// Its dates could not be read.
    Unknown,
}

/// The date of a time as [`CertInfo`] shows it.
pub fn date_of(shown: &str) -> &str {
    match shown.split_once(' ') {
        Some((date, _)) if x509::unix_time(shown).is_some() => date,
        _ => shown,
    }
}

/// The `CN=` value of a name as [`CertInfo`] shows it.
fn common_name(name: &str) -> Option<&str> {
    name.split(", ")
        .find_map(|part| part.strip_prefix("CN="))
        .filter(|cn| !cn.is_empty())
}

/// What one entry of a CA list holds: its certificates, or why its file
/// gave none.
pub fn certificates_of(ca: &CaCertificate, read: ReadFile) -> Result<Vec<Certificate>, String> {
    let certs = match ca {
        CaCertificate::Pem(pem) => x509::certificates_in(pem.as_bytes()),
        CaCertificate::Path(path) => match read(path) {
            Ok(bytes) => {
                let certs = x509::certificates_in(&bytes);
                if certs.is_empty() {
                    return Err("The file does not contain a certificate.".to_owned());
                }
                certs
            }
            Err(e) => return Err(e.to_string()),
        },
    };
    Ok(certs.into_iter().map(Certificate::new).collect())
}

/// The certificates Import takes from a file's bytes (PEM or DER): those
/// that read as certificates. Empty when the file holds none.
pub fn importable(file: &[u8]) -> Vec<Certificate> {
    x509::certificates_in(file)
        .into_iter()
        .filter(|der| x509::is_certificate(der))
        .map(Certificate::new)
        .collect()
}

/// One row of Certificate Management.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub what: What,
    /// The systems whose lists hold it, in the order the lists were given.
    pub systems: Vec<String>,
}

/// What a row is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum What {
    Certificate(Certificate),
    /// A referenced file that gives no certificate: it cannot be read, or
    /// holds none. The row is there so that the reference can be removed.
    Unreadable {
        path: PathBuf,
        why: String,
    },
}

impl Entry {
    /// The certificate, unless the row is a file that gave none.
    pub fn certificate(&self) -> Option<&Certificate> {
        match &self.what {
            What::Certificate(c) => Some(c),
            What::Unreadable { .. } => None,
        }
    }

    /// What messages call the row.
    pub fn name(&self) -> String {
        match &self.what {
            What::Certificate(c) => c.name().to_owned(),
            What::Unreadable { path, .. } => path.display().to_string(),
        }
    }

    /// What identifies the row from one listing to the next.
    pub fn key(&self) -> String {
        match &self.what {
            What::Certificate(c) => c.info.sha256.clone(),
            What::Unreadable { path, .. } => format!("file:{}", path.display()),
        }
    }
}

/// Every certificate in `lists` (system name, its CA list), one entry per
/// certificate, sorted by subject; then the referenced files that gave no
/// certificate, by path.
pub fn entries(lists: &[(String, Vec<CaCertificate>)], read: ReadFile) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut note = |what: What, system: &str| {
        let found = out.iter_mut().find(|e| match (&e.what, &what) {
            (What::Certificate(a), What::Certificate(b)) => a.info.sha256 == b.info.sha256,
            (What::Unreadable { path: a, .. }, What::Unreadable { path: b, .. }) => a == b,
            _ => false,
        });
        match found {
            Some(e) if e.systems.iter().any(|s| s == system) => {}
            Some(e) => e.systems.push(system.to_owned()),
            None => out.push(Entry {
                what,
                systems: vec![system.to_owned()],
            }),
        }
    };
    for (system, list) in lists {
        for ca in list {
            match certificates_of(ca, read) {
                Ok(certs) => {
                    for c in certs {
                        note(What::Certificate(c), system);
                    }
                }
                Err(why) => {
                    if let CaCertificate::Path(path) = ca {
                        let path = path.clone();
                        note(What::Unreadable { path, why }, system);
                    }
                }
            }
        }
    }
    out.sort_by_key(|e| match &e.what {
        What::Certificate(c) => (0, c.info.subject.to_lowercase(), c.info.sha256.clone()),
        What::Unreadable { path, .. } => {
            (1, path.display().to_string().to_lowercase(), String::new())
        }
    });
    out
}

/// Whether `list` holds the certificate with this fingerprint.
pub fn trusts(list: &[CaCertificate], sha256: &str, read: ReadFile) -> bool {
    list.iter().any(|ca| {
        certificates_of(ca, read).is_ok_and(|certs| certs.iter().any(|c| c.info.sha256 == sha256))
    })
}

/// Adds a certificate to `list`, as PEM text kept in the configuration.
/// False when the list holds it already.
pub fn add(list: &mut Vec<CaCertificate>, cert: &Certificate, read: ReadFile) -> bool {
    if trusts(list, &cert.info.sha256, read) {
        return false;
    }
    list.push(CaCertificate::Pem(cert.pem()));
    true
}

/// Takes the certificate with this fingerprint out of `list`. False when
/// the list does not hold it.
///
/// An entry that holds other certificates too keeps them: PEM text is
/// written again without the one removed, and a reference to a file (which
/// is never changed) is replaced by the file's other certificates as PEM
/// text.
pub fn remove(list: &mut Vec<CaCertificate>, sha256: &str, read: ReadFile) -> bool {
    let mut removed = false;
    let mut kept = Vec::with_capacity(list.len());
    for ca in list.drain(..) {
        match certificates_of(&ca, read) {
            Ok(certs) if certs.iter().any(|c| c.info.sha256 == sha256) => {
                removed = true;
                let rest: String = certs
                    .iter()
                    .filter(|c| c.info.sha256 != sha256)
                    .map(Certificate::pem)
                    .collect();
                if !rest.is_empty() {
                    kept.push(CaCertificate::Pem(rest));
                }
            }
            _ => kept.push(ca),
        }
    }
    *list = kept;
    removed
}

/// Takes the reference to `path` out of `list`. False when there is none.
pub fn remove_reference(list: &mut Vec<CaCertificate>, path: &Path) -> bool {
    let before = list.len();
    list.retain(|ca| !matches!(ca, CaCertificate::Path(p) if p == path));
    list.len() != before
}

/// Takes what `entry` is out of `list`.
pub fn remove_entry(list: &mut Vec<CaCertificate>, entry: &Entry, read: ReadFile) -> bool {
    match &entry.what {
        What::Certificate(c) => remove(list, &c.info.sha256, read),
        What::Unreadable { path, .. } => remove_reference(list, path),
    }
}

/// `a`, `a and b`, or `a, b and c`.
pub fn list_of(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [first @ .., last] => format!("{} and {last}", first.join(", ")),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A certificate built by hand: enough of one for [`x509`] to read.
    pub(crate) fn cert(cn: &str, issuer: &str, not_before: &str, not_after: &str) -> Certificate {
        Certificate::new(test_der(cn, issuer, not_before, not_after))
    }

    /// DER of a certificate shape with the given names and UTC times
    /// (`YYMMDDHHMMSSZ`).
    pub(crate) fn test_der(cn: &str, issuer: &str, not_before: &str, not_after: &str) -> Vec<u8> {
        crate::certs::sample::der(cn, issuer, not_before, not_after)
    }

    fn no_files(_: &Path) -> std::io::Result<Vec<u8>> {
        Err(std::io::Error::from(std::io::ErrorKind::NotFound))
    }

    const NOW: i64 = 1_793_534_400; // 2026-11-01 12:00:00 UTC

    #[test]
    fn valid_to_says_expired_or_how_long_is_left() {
        let c = |to: &str| cert("a", "ca", "250101000000Z", to);
        assert_eq!(c("361101120000Z").validity(NOW), Validity::Valid);
        assert_eq!(c("361101120000Z").valid_to(NOW), "2036-11-01");
        assert_eq!(c("261101115959Z").validity(NOW), Validity::Expired);
        assert_eq!(c("261101115959Z").valid_to(NOW), "Expired");
        assert_eq!(
            c("261101120000Z").valid_to(NOW),
            "2026-11-01 (expires today)"
        );
        assert_eq!(c("261102120000Z").valid_to(NOW), "2026-11-02 (1 day left)");
        assert_eq!(
            c("261122120000Z").valid_to(NOW),
            "2026-11-22 (21 days left)"
        );
        assert_eq!(c("261201120000Z").validity(NOW), Validity::Soon(30));
        assert_eq!(c("261202120000Z").validity(NOW), Validity::Valid);
        assert_eq!(
            cert("a", "ca", "270101000000Z", "280101000000Z").validity(NOW),
            Validity::NotYet
        );
        assert_eq!(
            Certificate::new(vec![1, 2, 3]).validity(NOW),
            Validity::Unknown
        );
        assert_eq!(
            Certificate::new(vec![1, 2, 3]).valid_to(NOW),
            "(unreadable)"
        );
    }

    #[test]
    fn a_certificate_is_named_by_its_common_name() {
        assert_eq!(
            cert("sys1.example.net", "ca", "250101000000Z", "270101000000Z").name(),
            "sys1.example.net"
        );
        assert_eq!(common_name("O=Example Corp, C=GB"), None);
        assert_eq!(common_name("OU=x, CN=a b, O=y"), Some("a b"));
    }

    #[test]
    fn one_entry_per_certificate_with_the_systems_that_trust_it() {
        let ca = cert(
            "Example Root",
            "Example Root",
            "250101000000Z",
            "350101000000Z",
        );
        let own = cert(
            "dev.example.net",
            "dev.example.net",
            "250101000000Z",
            "270101000000Z",
        );
        let lists = vec![
            (
                "dev.example.net".to_owned(),
                vec![
                    CaCertificate::Pem(format!("{}{}", own.pem(), ca.pem())),
                    CaCertificate::Path("/gone.pem".into()),
                ],
            ),
            (
                "sys1.example.net".to_owned(),
                vec![CaCertificate::Pem(ca.pem())],
            ),
            ("10.1.1.7".to_owned(), Vec::new()),
        ];
        let e = entries(&lists, &no_files);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].certificate(), Some(&own));
        assert_eq!(e[0].systems, ["dev.example.net"]);
        assert_eq!(e[1].certificate(), Some(&ca));
        assert_eq!(e[1].systems, ["dev.example.net", "sys1.example.net"]);
        assert!(
            matches!(&e[2].what, What::Unreadable { path, .. } if path == Path::new("/gone.pem"))
        );
        assert_eq!(e[2].name(), "/gone.pem");
    }

    #[test]
    fn a_referenced_file_is_read_as_pem_or_der() {
        let a = cert("a", "ca", "250101000000Z", "270101000000Z");
        let b = cert("b", "ca", "250101000000Z", "270101000000Z");
        let (a2, b2) = (a.clone(), b.clone());
        let read = move |p: &Path| -> std::io::Result<Vec<u8>> {
            match p.to_str() {
                Some("/a.der") => Ok(a2.der.clone()),
                Some("/both.pem") => Ok(format!("{}{}", a2.pem(), b2.pem()).into_bytes()),
                Some("/notes.txt") => Ok(b"nothing here".to_vec()),
                _ => Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
            }
        };
        let der = CaCertificate::Path("/a.der".into());
        assert_eq!(
            certificates_of(&der, &read).unwrap(),
            std::slice::from_ref(&a)
        );
        let both = CaCertificate::Path("/both.pem".into());
        assert_eq!(
            certificates_of(&both, &read).unwrap(),
            [a.clone(), b.clone()]
        );
        let notes = CaCertificate::Path("/notes.txt".into());
        assert_eq!(
            certificates_of(&notes, &read).unwrap_err(),
            "The file does not contain a certificate."
        );
        assert!(certificates_of(&CaCertificate::Path("/x".into()), &read).is_err());
    }

    #[test]
    fn adding_is_once_and_removing_keeps_the_others() {
        let a = cert("a", "ca", "250101000000Z", "270101000000Z");
        let b = cert("b", "ca", "250101000000Z", "270101000000Z");
        let c = cert("c", "ca", "250101000000Z", "270101000000Z");
        let (b2, c2) = (b.clone(), c.clone());
        let read = move |p: &Path| -> std::io::Result<Vec<u8>> {
            match p.to_str() {
                Some("/bc.pem") => Ok(format!("{}{}", b2.pem(), c2.pem()).into_bytes()),
                _ => Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
            }
        };
        let mut list = vec![CaCertificate::Path("/bc.pem".into())];
        assert!(add(&mut list, &a, &read));
        assert!(!add(&mut list, &a, &read), "already there");
        assert!(!add(&mut list, &b, &read), "the file has it");
        assert_eq!(list.len(), 2);
        assert!(trusts(&list, &c.info.sha256, &read));

        // Out of the file's reference: the file's other certificate stays,
        // as PEM text.
        assert!(remove(&mut list, &b.info.sha256, &read));
        assert_eq!(
            list,
            [CaCertificate::Pem(c.pem()), CaCertificate::Pem(a.pem())]
        );
        assert!(!remove(&mut list, &b.info.sha256, &read));
        assert!(remove(&mut list, &a.info.sha256, &read));
        assert!(remove(&mut list, &c.info.sha256, &read));
        assert!(list.is_empty());

        let mut list = vec![
            CaCertificate::Path("/gone.pem".into()),
            CaCertificate::Pem(a.pem()),
        ];
        assert!(remove_reference(&mut list, Path::new("/gone.pem")));
        assert!(!remove_reference(&mut list, Path::new("/gone.pem")));
        assert_eq!(list, [CaCertificate::Pem(a.pem())]);
    }

    #[test]
    fn import_takes_pem_der_several_and_refuses_what_is_not_a_certificate() {
        let a = cert("a", "ca", "250101000000Z", "270101000000Z");
        let b = cert("b", "ca", "250101000000Z", "270101000000Z");
        // PEM, with text around it.
        let pem = format!("# exported\n{}", a.pem());
        assert_eq!(importable(pem.as_bytes()), std::slice::from_ref(&a));
        // DER.
        assert_eq!(importable(&a.der), std::slice::from_ref(&a));
        // Several in one file, in the file's order, a key between them
        // left out.
        let key = "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n";
        let several = format!("{}{key}{}", b.pem(), a.pem());
        assert_eq!(importable(several.as_bytes()), [b.clone(), a.clone()]);
        // Not a certificate: text, a key alone, DER cut short, and a
        // CERTIFICATE block that does not hold one.
        assert!(importable(b"hello").is_empty());
        assert!(importable(key.as_bytes()).is_empty());
        assert!(importable(&a.der[..a.der.len() - 2]).is_empty());
        let fake = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";
        assert!(importable(fake.as_bytes()).is_empty());
        assert!(importable(b"").is_empty());
    }

    #[test]
    fn names_are_listed_as_a_sentence_reads() {
        let n = |v: &[&str]| list_of(&v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert_eq!(n(&[]), "");
        assert_eq!(n(&["a"]), "a");
        assert_eq!(n(&["a", "b"]), "a and b");
        assert_eq!(n(&["a", "b", "c"]), "a, b and c");
    }
}
