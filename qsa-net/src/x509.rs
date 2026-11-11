// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Just enough X.509 (RFC 5280) DER reading to show a certificate to the
//! user: subject, issuer, validity and fingerprint, and to take the
//! certificates out of a PEM or DER file. Verification is rustls's job, not
//! this module's.

use sha2::{Digest, Sha256};

/// What the untrusted-certificate dialog shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertInfo {
    /// `CN=..., O=...`, most specific first.
    pub subject: String,
    pub issuer: String,
    /// `YYYY-MM-DD HH:MM:SS UTC`.
    pub not_before: String,
    pub not_after: String,
    /// SHA-256 of the DER, as colon-separated hex pairs.
    pub sha256: String,
    /// Serial number, hex.
    pub serial: String,
}

/// Reads a certificate. Fields that cannot be read show as `(unreadable)`;
/// the fingerprint is always there.
pub fn describe(der: &[u8]) -> CertInfo {
    let unreadable = || "(unreadable)".to_owned();
    let mut info = CertInfo {
        subject: unreadable(),
        issuer: unreadable(),
        not_before: unreadable(),
        not_after: unreadable(),
        sha256: fingerprint(der),
        serial: unreadable(),
    };
    let _ = read_tbs(der, &mut info);
    info
}

/// More of a certificate than [`CertInfo`] has, for a viewer: how it was
/// signed, its key, the names it is good for and whether it is a
/// certificate authority's. What cannot be read shows as `(unreadable)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertDetails {
    /// The signature algorithm: `ecdsa-with-SHA256`, `sha256WithRSA` and
    /// the like, or its dotted OID when it is not one of the common ones.
    pub signature: String,
    /// The public key: `EC P-256`, `RSA 2048 bits`, `Ed25519`.
    pub public_key: String,
    /// The subject alternative names, as written in the certificate:
    /// `DNS:sys1.example.net`, `IP:192.0.2.1`, `Email:`, `URI:`. Empty when
    /// the certificate has none.
    pub alt_names: Vec<String>,
    /// Whether the certificate says it is a certificate authority's (its
    /// basic constraints): `None` when it does not say.
    pub ca: Option<bool>,
}

/// Reads [`CertDetails`] from a certificate.
pub fn details(der: &[u8]) -> CertDetails {
    let mut d = CertDetails {
        signature: "(unreadable)".to_owned(),
        public_key: "(unreadable)".to_owned(),
        alt_names: Vec::new(),
        ca: None,
    };
    let _ = read_details(der, &mut d);
    d
}

const BOOLEAN: u8 = 0x01;
const BIT_STRING: u8 = 0x03;
const OCTET_STRING: u8 = 0x04;
/// `[3] EXPLICIT Extensions` of a TBSCertificate.
const EXTENSIONS: u8 = 0xA3;

fn read_details(der: &[u8], d: &mut CertDetails) -> Option<()> {
    let (cert, _) = expect(der, SEQUENCE)?;
    let (tbs, _) = expect(cert, SEQUENCE)?;
    let mut rest = tbs;
    if let Some((VERSION, _, r)) = tlv(rest) {
        rest = r;
    }
    let (_serial, r) = expect(rest, INTEGER)?;
    let (sig, r) = expect(r, SEQUENCE)?;
    if let Some((oid, _)) = expect(sig, OID) {
        d.signature = signature_name(oid);
    }
    let (_issuer, r) = expect(r, SEQUENCE)?;
    let (_validity, r) = expect(r, SEQUENCE)?;
    let (_subject, r) = expect(r, SEQUENCE)?;
    let (spki, mut r) = expect(r, SEQUENCE)?;
    if let Some(key) = public_key(spki) {
        d.public_key = key;
    }
    // The unique IDs ([1], [2]) may come before the extensions ([3]).
    while let Some((tag, body, next)) = tlv(r) {
        if tag == EXTENSIONS
            && let Some((list, _)) = expect(body, SEQUENCE)
        {
            extensions(list, d);
        }
        r = next;
    }
    Some(())
}

fn signature_name(oid: &[u8]) -> String {
    const RSA: [u8; 8] = [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01];
    const ECDSA: [u8; 7] = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03];
    let name = match oid {
        [head @ .., 0x05] if head == RSA => "sha1WithRSA",
        [head @ .., 0x0A] if head == RSA => "RSASSA-PSS",
        [head @ .., 0x0B] if head == RSA => "sha256WithRSA",
        [head @ .., 0x0C] if head == RSA => "sha384WithRSA",
        [head @ .., 0x0D] if head == RSA => "sha512WithRSA",
        [head @ .., 0x02] if head == ECDSA => "ecdsa-with-SHA256",
        [head @ .., 0x03] if head == ECDSA => "ecdsa-with-SHA384",
        [head @ .., 0x04] if head == ECDSA => "ecdsa-with-SHA512",
        [0x2B, 0x65, 0x70] => "Ed25519",
        [0x2B, 0x65, 0x71] => "Ed448",
        _ => return dotted(oid),
    };
    name.to_owned()
}

/// A SubjectPublicKeyInfo's key, by kind and size.
fn public_key(spki: &[u8]) -> Option<String> {
    let (algorithm, key) = expect(spki, SEQUENCE)?;
    let (oid, parameters) = expect(algorithm, OID)?;
    Some(match oid {
        // rsaEncryption: the key is a SEQUENCE of modulus and exponent.
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01] => {
            let (bits, _) = expect(key, BIT_STRING)?;
            let (pair, _) = expect(bits.get(1..)?, SEQUENCE)?;
            let (modulus, _) = expect(pair, INTEGER)?;
            let bytes = modulus.iter().skip_while(|b| **b == 0).count();
            format!("RSA {} bits", bytes * 8)
        }
        // id-ecPublicKey: its parameter names the curve.
        [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01] => match expect(parameters, OID) {
            Some(([0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07], _)) => "EC P-256".to_owned(),
            Some(([0x2B, 0x81, 0x04, 0x00, 0x22], _)) => "EC P-384".to_owned(),
            Some(([0x2B, 0x81, 0x04, 0x00, 0x23], _)) => "EC P-521".to_owned(),
            Some((curve, _)) => format!("EC {}", dotted(curve)),
            None => "EC".to_owned(),
        },
        [0x2B, 0x65, 0x70] => "Ed25519".to_owned(),
        [0x2B, 0x65, 0x71] => "Ed448".to_owned(),
        other => dotted(other),
    })
}

/// Takes the alternative names and the basic constraints from a list of
/// extensions.
fn extensions(mut list: &[u8], d: &mut CertDetails) {
    while let Some((SEQUENCE, ext, rest)) = tlv(list) {
        list = rest;
        let Some((oid, mut r)) = expect(ext, OID) else {
            continue;
        };
        // critical BOOLEAN DEFAULT FALSE.
        if let Some((BOOLEAN, _, after)) = tlv(r) {
            r = after;
        }
        let Some((value, _)) = expect(r, OCTET_STRING) else {
            continue;
        };
        match oid {
            // subjectAltName.
            [0x55, 0x1D, 0x11] => {
                if let Some((names, _)) = expect(value, SEQUENCE) {
                    d.alt_names = alt_names(names);
                }
            }
            // basicConstraints: cA BOOLEAN DEFAULT FALSE comes first.
            [0x55, 0x1D, 0x13] => {
                let first = expect(value, SEQUENCE).and_then(|(c, _)| tlv(c));
                d.ca = Some(matches!(first, Some((BOOLEAN, [b], _)) if *b != 0));
            }
            _ => {}
        }
    }
}

/// GeneralNames as text: the kinds a server certificate carries.
fn alt_names(mut names: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    while let Some((tag, body, rest)) = tlv(names) {
        names = rest;
        let text = || String::from_utf8_lossy(body).into_owned();
        match tag {
            0x81 => out.push(format!("Email:{}", text())),
            0x82 => out.push(format!("DNS:{}", text())),
            0x86 => out.push(format!("URI:{}", text())),
            0x87 => {
                if let Ok(v4) = <[u8; 4]>::try_from(body) {
                    out.push(format!("IP:{}", std::net::Ipv4Addr::from(v4)));
                } else if let Ok(v6) = <[u8; 16]>::try_from(body) {
                    out.push(format!("IP:{}", std::net::Ipv6Addr::from(v6)));
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether `der` is one certificate and nothing else: its outer structure
/// and the fields [`describe`] shows can all be read.
pub fn is_certificate(der: &[u8]) -> bool {
    let whole = matches!(tlv(der), Some((SEQUENCE, _, rest)) if rest.is_empty());
    whole && read_tbs(der, &mut describe(&[])).is_some()
}

/// The certificates in a file's bytes, each as DER: every `CERTIFICATE`
/// block of a PEM file as it stands (blocks of other kinds, such as keys,
/// are left out), or the one certificate of a DER file. Empty when the
/// file holds none. This is what a system configuration's CA list trusts
/// of the file; [`is_certificate`] says whether a block reads as one.
pub fn certificates_in(file: &[u8]) -> Vec<Vec<u8>> {
    const BEGIN: &[u8] = b"-----BEGIN ";
    if file.windows(BEGIN.len()).any(|w| w == BEGIN) {
        return match std::str::from_utf8(file) {
            Ok(pem) => crate::tls::certs_from_pem(pem)
                .into_iter()
                .map(|c| c.to_vec())
                .collect(),
            Err(_) => Vec::new(),
        };
    }
    if is_certificate(file) {
        vec![file.to_vec()]
    } else {
        Vec::new()
    }
}

/// A time as [`CertInfo`] shows it (`YYYY-MM-DD HH:MM:SS UTC`) in seconds
/// since 1970-01-01 00:00:00 UTC. `None` for anything else, `(unreadable)`
/// among it.
pub fn unix_time(shown: &str) -> Option<i64> {
    let s = shown.strip_suffix(" UTC")?;
    let b = s.as_bytes();
    if b.len() != 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b' ' || b[13] != b':' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Days from 1970-01-01 to y-m-d in the Gregorian calendar, counting
    // years from 1 March so that the leap day comes last.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// SHA-256 over the DER, `AB:CD:...`.
pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

const SEQUENCE: u8 = 0x30;
const SET: u8 = 0x31;
const INTEGER: u8 = 0x02;
const OID: u8 = 0x06;
const UTC_TIME: u8 = 0x17;
const GENERALIZED_TIME: u8 = 0x18;
const VERSION: u8 = 0xA0;

/// One TLV: its tag and contents, and what follows it.
fn tlv(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, mut rest) = rest.split_first()?;
    let len = if first < 0x80 {
        first as usize
    } else {
        let n = (first & 0x7F) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        let mut len = 0usize;
        for &b in &rest[..n] {
            len = (len << 8) | b as usize;
        }
        rest = &rest[n..];
        len
    };
    if rest.len() < len {
        return None;
    }
    Some((tag, &rest[..len], &rest[len..]))
}

fn expect(input: &[u8], want: u8) -> Option<(&[u8], &[u8])> {
    let (tag, body, rest) = tlv(input)?;
    (tag == want).then_some((body, rest))
}

fn read_tbs(der: &[u8], info: &mut CertInfo) -> Option<()> {
    let (cert, _) = expect(der, SEQUENCE)?;
    let (tbs, _) = expect(cert, SEQUENCE)?;
    let mut rest = tbs;
    if let Some((VERSION, _, r)) = tlv(rest) {
        rest = r;
    }
    let (serial, r) = expect(rest, INTEGER)?;
    info.serial = serial.iter().map(|b| format!("{b:02X}")).collect();
    let (_sig, r) = expect(r, SEQUENCE)?;
    let (issuer, r) = expect(r, SEQUENCE)?;
    info.issuer = name(issuer);
    let (validity, r) = expect(r, SEQUENCE)?;
    let (t1, v) = time(validity)?;
    let (t2, _) = time(v)?;
    info.not_before = t1;
    info.not_after = t2;
    let (subject, _) = expect(r, SEQUENCE)?;
    info.subject = name(subject);
    Some(())
}

fn time(input: &[u8]) -> Option<(String, &[u8])> {
    let (tag, body, rest) = tlv(input)?;
    let s = std::str::from_utf8(body).ok()?;
    let s = s.strip_suffix('Z').unwrap_or(s);
    let full = match tag {
        UTC_TIME if s.len() >= 12 => {
            let yy: u32 = s[..2].parse().ok()?;
            let century = if yy >= 50 { "19" } else { "20" };
            format!("{century}{s}")
        }
        GENERALIZED_TIME if s.len() >= 14 => s.to_owned(),
        _ => return None,
    };
    if !full.bytes().take(14).all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((
        format!(
            "{}-{}-{} {}:{}:{} UTC",
            &full[0..4],
            &full[4..6],
            &full[6..8],
            &full[8..10],
            &full[10..12],
            &full[12..14]
        ),
        rest,
    ))
}

/// A Name as `CN=..., O=...`, most specific first.
fn name(mut input: &[u8]) -> String {
    let mut parts = Vec::new();
    while let Some((SET, mut set, rest)) = tlv(input) {
        while let Some((SEQUENCE, atv, more)) = tlv(set) {
            if let Some((oid, value)) = expect(atv, OID) {
                let text = tlv(value)
                    .map(|(_, v, _)| String::from_utf8_lossy(v).into_owned())
                    .unwrap_or_default();
                parts.push(format!("{}={text}", attribute(oid)));
            }
            set = more;
        }
        input = rest;
    }
    if parts.is_empty() {
        return "(empty)".to_owned();
    }
    parts.reverse();
    parts.join(", ")
}

fn attribute(oid: &[u8]) -> String {
    match oid {
        [0x55, 0x04, 0x03] => "CN".into(),
        [0x55, 0x04, 0x06] => "C".into(),
        [0x55, 0x04, 0x07] => "L".into(),
        [0x55, 0x04, 0x08] => "ST".into(),
        [0x55, 0x04, 0x0A] => "O".into(),
        [0x55, 0x04, 0x0B] => "OU".into(),
        [0x55, 0x04, 0x05] => "SERIALNUMBER".into(),
        [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x01] => "E".into(),
        _ => dotted(oid),
    }
}

fn dotted(oid: &[u8]) -> String {
    let mut arcs = Vec::new();
    let mut v: u64 = 0;
    for &b in oid {
        v = (v << 7) | u64::from(b & 0x7F);
        if b & 0x80 == 0 {
            if arcs.is_empty() {
                let first = (v / 40).min(2);
                arcs.push(first);
                arcs.push(v - first * 40);
            } else {
                arcs.push(v);
            }
            v = 0;
        }
    }
    arcs.iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DER helpers to build a certificate shape by hand.
    fn der(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![tag];
        if body.len() < 0x80 {
            v.push(body.len() as u8);
        } else {
            v.extend_from_slice(&[0x82, (body.len() >> 8) as u8, body.len() as u8]);
        }
        v.extend_from_slice(body);
        v
    }

    fn rdn(oid: &[u8], value: &str) -> Vec<u8> {
        let atv = [der(OID, oid), der(0x0C, value.as_bytes())].concat();
        der(SET, &der(SEQUENCE, &atv))
    }

    fn sample() -> Vec<u8> {
        let issuer = der(
            SEQUENCE,
            &[
                rdn(&[0x55, 4, 6], "US"),
                rdn(&[0x55, 4, 10], "Quartz Test CA"),
            ]
            .concat(),
        );
        let subject = der(
            SEQUENCE,
            &[
                rdn(&[0x55, 4, 10], "Quartz Systems"),
                rdn(&[0x55, 4, 3], "qs400.example"),
            ]
            .concat(),
        );
        let validity = der(
            SEQUENCE,
            &[
                der(UTC_TIME, b"261101120000Z"),
                der(GENERALIZED_TIME, b"20361101120000Z"),
            ]
            .concat(),
        );
        let tbs = der(
            SEQUENCE,
            &[
                der(VERSION, &der(INTEGER, &[2])),
                der(INTEGER, &[0x01, 0xAB]),
                der(
                    SEQUENCE,
                    &der(OID, &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 4, 3, 2]),
                ),
                issuer,
                validity,
                subject,
            ]
            .concat(),
        );
        der(SEQUENCE, &tbs)
    }

    #[test]
    fn reads_the_displayed_fields() {
        let info = describe(&sample());
        assert_eq!(info.subject, "CN=qs400.example, O=Quartz Systems");
        assert_eq!(info.issuer, "O=Quartz Test CA, C=US");
        assert_eq!(info.not_before, "2026-11-01 12:00:00 UTC");
        assert_eq!(info.not_after, "2036-11-01 12:00:00 UTC");
        assert_eq!(info.serial, "01AB");
        assert_eq!(info.sha256.len(), 32 * 3 - 1);
    }

    #[test]
    fn garbage_still_has_a_fingerprint() {
        let info = describe(&[0x30, 0x82, 0xFF]);
        assert_eq!(info.subject, "(unreadable)");
        assert_eq!(info.sha256, fingerprint(&[0x30, 0x82, 0xFF]));
        // The SHA-256 of nothing.
        assert!(fingerprint(&[]).starts_with("E3:B0:C4:42"));
    }

    fn data(name: &str) -> Vec<u8> {
        std::fs::read(format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn a_pem_file_gives_its_certificate() {
        let certs = certificates_in(&data("test-ca.pem"));
        assert_eq!(certs.len(), 1);
        assert!(is_certificate(&certs[0]));
        let info = describe(&certs[0]);
        assert_eq!(info.subject, "CN=Quartz Test CA, O=Quartz Systems Test");
        // CR LF line ends and text around the block change nothing.
        let text = String::from_utf8(data("test-ca.pem")).unwrap();
        let dos = format!(
            "subject=Quartz Test CA\r\n{}",
            text.replace("\r\n", "\n").replace('\n', "\r\n")
        );
        assert_eq!(certificates_in(dos.as_bytes()), certs);
    }

    #[test]
    fn a_der_file_gives_its_certificate() {
        let der = certificates_in(&data("server.pem")).remove(0);
        assert_eq!(certificates_in(&der), vec![der.clone()]);
        // Not with anything after it, and not a part of it.
        let mut longer = der.clone();
        longer.push(0);
        assert!(certificates_in(&longer).is_empty());
        assert!(certificates_in(&der[..der.len() - 1]).is_empty());
    }

    #[test]
    fn several_certificates_in_one_file_come_in_order() {
        let mut all = data("server.pem");
        all.extend_from_slice(&data("test-ca.pem"));
        all.extend_from_slice(&data("selfsigned.pem"));
        let certs = certificates_in(&all);
        assert_eq!(certs.len(), 3);
        let own = certificates_in(&data("selfsigned.pem")).remove(0);
        assert_eq!(
            describe(&certs[0]).subject,
            "CN=localhost, O=Quartz Systems Test"
        );
        assert_eq!(
            describe(&certs[1]).subject,
            "CN=Quartz Test CA, O=Quartz Systems Test"
        );
        assert_eq!(certs[2], own);
    }

    #[test]
    fn a_file_with_no_certificate_gives_none() {
        // A private key, text, nothing, DER that is not a certificate and
        // a part of one.
        assert!(certificates_in(&data("server.key")).is_empty());
        assert!(certificates_in(b"not a certificate\n").is_empty());
        assert!(certificates_in(b"").is_empty());
        assert!(certificates_in(&[0x30, 0x03, 0x02, 0x01, 0x05]).is_empty());
        assert!(certificates_in(&sample()[..20]).is_empty());
        // A CERTIFICATE block that does not hold one is given as it is,
        // and does not read as a certificate.
        let fake = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";
        assert_eq!(certificates_in(fake.as_bytes()), [vec![0u8, 0, 0]]);
        assert!(!is_certificate(&[0, 0, 0]));
        // The hand-built sample is one.
        assert!(is_certificate(&sample()));
    }

    #[test]
    fn details_give_the_key_the_names_and_whether_it_is_a_ca() {
        let server = details(&certificates_in(&data("server.pem"))[0]);
        assert_eq!(server.signature, "ecdsa-with-SHA256");
        assert_eq!(server.public_key, "EC P-256");
        assert_eq!(server.alt_names, ["DNS:localhost", "IP:127.0.0.1"]);
        assert_eq!(server.ca, Some(false));
        let ca = details(&certificates_in(&data("test-ca.pem"))[0]);
        assert_eq!(ca.public_key, "EC P-256");
        assert!(ca.alt_names.is_empty());
        assert_eq!(ca.ca, Some(true));
        let own = details(&certificates_in(&data("selfsigned.pem"))[0]);
        assert_eq!(own.alt_names, ["DNS:localhost", "IP:127.0.0.1"]);
        assert_eq!(own.ca, Some(true));
        // The hand-built sample stops after its subject: its signature
        // algorithm is read, and the rest says so.
        let s = details(&sample());
        assert_eq!(s.signature, "ecdsa-with-SHA256");
        assert_eq!(s.public_key, "(unreadable)");
        assert_eq!((s.alt_names.len(), s.ca), (0, None));
        assert_eq!(details(&[1, 2, 3]).signature, "(unreadable)");
    }

    #[test]
    fn keys_and_signatures_are_named() {
        // An RSA key: SEQUENCE { rsaEncryption, NULL }, BIT STRING holding
        // SEQUENCE { modulus (256 bytes after its sign byte), exponent }.
        let mut modulus = vec![0u8];
        modulus.extend(std::iter::repeat_n(0xC3u8, 256));
        let pair = der(
            SEQUENCE,
            &[der(INTEGER, &modulus), der(INTEGER, &[1, 0, 1])].concat(),
        );
        let mut bits = vec![0u8];
        bits.extend_from_slice(&pair);
        let rsa = [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
        let spki = [
            der(SEQUENCE, &[der(OID, &rsa), vec![0x05, 0x00]].concat()),
            der(BIT_STRING, &bits),
        ]
        .concat();
        assert_eq!(public_key(&spki).as_deref(), Some("RSA 2048 bits"));
        let ec = |curve: &[u8]| {
            let id = [0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
            let alg = der(SEQUENCE, &[der(OID, &id), der(OID, curve)].concat());
            public_key(&[alg, der(BIT_STRING, &[0, 4, 1, 2])].concat())
        };
        assert_eq!(
            ec(&[0x2B, 0x81, 0x04, 0x00, 0x22]).as_deref(),
            Some("EC P-384")
        );
        assert_eq!(
            ec(&[0x2B, 0x81, 0x04, 0x00, 0x0A]).as_deref(),
            Some("EC 1.3.132.0.10")
        );
        let ed = der(SEQUENCE, &der(OID, &[0x2B, 0x65, 0x70]));
        assert_eq!(public_key(&ed).as_deref(), Some("Ed25519"));
        let rsa_sig =
            |last: u8| signature_name(&[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, last]);
        assert_eq!(rsa_sig(0x0B), "sha256WithRSA");
        assert_eq!(rsa_sig(0x0A), "RSASSA-PSS");
        assert_eq!(rsa_sig(0x63), "1.2.840.113549.1.1.99");
        assert_eq!(signature_name(&[0x2B, 0x65, 0x70]), "Ed25519");
    }

    #[test]
    fn alternative_names_of_each_kind_are_read() {
        let names = [
            der(0x82, b"sys1.example.net"),
            der(0x87, &[192, 0, 2, 7]),
            der(
                0x87,
                &[0x20, 0x01, 0x0D, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            ),
            der(0x81, b"ops@example.net"),
            der(0x86, b"urn:example:sys1"),
            // A directory name, and an address of no known length.
            der(0xA4, &[0x30, 0x00]),
            der(0x87, &[1, 2, 3]),
        ]
        .concat();
        assert_eq!(
            alt_names(&names),
            [
                "DNS:sys1.example.net",
                "IP:192.0.2.7",
                "IP:2001:db8::1",
                "Email:ops@example.net",
                "URI:urn:example:sys1"
            ]
        );
        // Basic constraints: an empty SEQUENCE says not a CA.
        let ext = |oid: &[u8], value: &[u8]| {
            der(
                SEQUENCE,
                &[
                    der(OID, oid),
                    der(BOOLEAN, &[0xFF]),
                    der(OCTET_STRING, value),
                ]
                .concat(),
            )
        };
        let mut d = details(&[]);
        extensions(&ext(&[0x55, 0x1D, 0x13], &der(SEQUENCE, &[])), &mut d);
        assert_eq!(d.ca, Some(false));
        let is_ca = der(SEQUENCE, &der(BOOLEAN, &[0xFF]));
        extensions(&ext(&[0x55, 0x1D, 0x13], &is_ca), &mut d);
        assert_eq!(d.ca, Some(true));
    }

    #[test]
    fn shown_times_convert_to_unix_time() {
        assert_eq!(unix_time("1970-01-01 00:00:00 UTC"), Some(0));
        assert_eq!(unix_time("2000-03-01 00:00:00 UTC"), Some(951_868_800));
        assert_eq!(unix_time("2024-02-29 23:59:59 UTC"), Some(1_709_251_199));
        assert_eq!(unix_time("2026-11-01 12:00:00 UTC"), Some(1_793_534_400));
        assert_eq!(unix_time("(unreadable)"), None);
        assert_eq!(unix_time("2026-13-01 00:00:00 UTC"), None);
        assert_eq!(unix_time("2026-11-01 12:00:00"), None);
        // The sample is valid for ten years with three leap days.
        let info = describe(&sample());
        assert_eq!(
            unix_time(&info.not_after).unwrap() - unix_time(&info.not_before).unwrap(),
            3653 * 86_400
        );
    }

    #[test]
    fn unknown_attributes_show_as_dotted_oids() {
        assert_eq!(
            dotted(&[0x2B, 0x06, 0x01, 0x04, 0x01, 0x82, 0x37]),
            "1.3.6.1.4.1.311"
        );
        assert_eq!(attribute(&[0x55, 0x04, 0x2A]), "2.5.4.42");
    }
}
