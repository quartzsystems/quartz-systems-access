// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Just enough X.509 (RFC 5280) DER reading to show a certificate to the
//! user: subject, issuer, validity and fingerprint. Verification is
//! rustls's job, not this module's.

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

    #[test]
    fn unknown_attributes_show_as_dotted_oids() {
        assert_eq!(
            dotted(&[0x2B, 0x06, 0x01, 0x04, 0x01, 0x82, 0x37]),
            "1.3.6.1.4.1.311"
        );
        assert_eq!(attribute(&[0x55, 0x04, 0x2A]), "2.5.4.42");
    }
}
