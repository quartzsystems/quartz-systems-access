// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Certificate shapes built by hand, for tests and screenshots: the fields
//! Certificate Management shows, with dates of the caller's choosing. They
//! carry no key and no signature, so nothing would ever verify with one.

const SEQUENCE: u8 = 0x30;
const SET: u8 = 0x31;
const INTEGER: u8 = 0x02;
const OID: u8 = 0x06;
const UTF8_STRING: u8 = 0x0C;
const UTC_TIME: u8 = 0x17;
const VERSION: u8 = 0xA0;

fn tlv(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    if body.len() < 0x80 {
        v.push(body.len() as u8);
    } else {
        v.extend_from_slice(&[0x82, (body.len() >> 8) as u8, body.len() as u8]);
    }
    v.extend_from_slice(body);
    v
}

/// A name of an organization and a common name.
fn name(cn: &str) -> Vec<u8> {
    let rdn = |oid: &[u8], value: &str| {
        let atv = [tlv(OID, oid), tlv(UTF8_STRING, value.as_bytes())].concat();
        tlv(SET, &tlv(SEQUENCE, &atv))
    };
    tlv(
        SEQUENCE,
        &[rdn(&[0x55, 4, 10], "Example Corp"), rdn(&[0x55, 4, 3], cn)].concat(),
    )
}

/// DER in the shape of a certificate for `subject`, issued by `issuer`
/// (common names), valid between two UTC times written `YYMMDDHHMMSSZ`.
/// The serial number is taken from the names, so two samples differ.
pub fn der(subject: &str, issuer: &str, not_before: &str, not_after: &str) -> Vec<u8> {
    let serial = subject
        .bytes()
        .chain(issuer.bytes())
        .fold(7u16, |a, b| a.wrapping_mul(31).wrapping_add(u16::from(b)));
    let validity = tlv(
        SEQUENCE,
        &[
            tlv(UTC_TIME, not_before.as_bytes()),
            tlv(UTC_TIME, not_after.as_bytes()),
        ]
        .concat(),
    );
    let tbs = tlv(
        SEQUENCE,
        &[
            tlv(VERSION, &tlv(INTEGER, &[2])),
            tlv(INTEGER, &[0x01, (serial >> 8) as u8 & 0x7F, serial as u8]),
            // ecdsa-with-SHA256.
            tlv(
                SEQUENCE,
                &tlv(OID, &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 4, 3, 2]),
            ),
            name(issuer),
            validity,
            name(subject),
        ]
        .concat(),
    );
    tlv(SEQUENCE, &tbs)
}

#[cfg(test)]
mod tests {
    use qsa_net::x509;

    #[test]
    fn a_sample_reads_as_a_certificate() {
        let der = super::der(
            "sys1.example.net",
            "Example Root CA",
            "260101000000Z",
            "270101000000Z",
        );
        assert!(x509::is_certificate(&der));
        let info = x509::describe(&der);
        assert_eq!(info.subject, "CN=sys1.example.net, O=Example Corp");
        assert_eq!(info.issuer, "CN=Example Root CA, O=Example Corp");
        assert_eq!(info.not_before, "2026-01-01 00:00:00 UTC");
        assert_eq!(info.not_after, "2027-01-01 00:00:00 UTC");
        assert_eq!(x509::certificates_in(&der), std::slice::from_ref(&der));
        let other = super::der("a", "b", "260101000000Z", "270101000000Z");
        assert_ne!(x509::fingerprint(&der), x509::fingerprint(&other));
    }
}
