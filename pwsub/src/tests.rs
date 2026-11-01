// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

use super::*;

fn hex<const N: usize>(s: &str) -> [u8; N] {
    let v: Vec<u8> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect();
    v.try_into().unwrap()
}

fn seeds(server: &str, client: &str) -> Seeds {
    Seeds {
        server: hex(server),
        client: hex(client),
    }
}

/// RFC 4777 section 5.1, item 9: the DES example.
#[test]
fn rfc4777_des_example() {
    let s = seeds("7D4C2319F28004B2", "08BEF662D851F4B1");
    let sub = substitute(0, "USER123", "ABCDEFG", &s).unwrap();
    assert_eq!(sub.expose(), hex::<8>("5A58BD50E4DD9B5F"));
    // Level 1 is the same scheme, and case does not matter at these levels.
    assert_eq!(substitute(1, "user123", "abcdefg", &s).unwrap(), sub);
}

/// RFC 4777 section 5.2, item 6: the SHA-1 example.
#[test]
fn rfc4777_sha1_example() {
    let s = seeds("3E3A71C78795E5F5", "B1C806D5D377D994");
    let sub = substitute(2, "USER123", "AbCdEfGh123?+", &s).unwrap();
    assert_eq!(
        sub.expose(),
        hex::<20>("E7FAB5F034BEDA42E91F439DD07532A24140E3DD")
    );
    assert_eq!(substitute(3, "user123", "AbCdEfGh123?+", &s).unwrap(), sub);
    // Case matters for the password at these levels.
    assert_ne!(substitute(2, "USER123", "ABCDEFGH123?+", &s).unwrap(), sub);
}

/// RFC 4777 section 5: the encrypted automatic sign-on example's bytes
/// (user DUMMYUSR, password DUMMYPW).
#[test]
fn rfc4777_telnet_example() {
    let s = seeds("7D3E488F18080404", "4E4142334E414233");
    let t = telnet(0, "dummyusr", "DUMMYPW", &s).unwrap();
    assert_eq!(t.user, "DUMMYUSR");
    assert_eq!(t.client_seed, hex::<8>("4E4142334E414233"));
    assert_eq!(t.substitute.expose(), hex::<8>("DFB0402F22ABA3BA"));
}

#[test]
fn levels_select_schemes() {
    assert_eq!(Scheme::for_level(0), Ok(Scheme::Des));
    assert_eq!(Scheme::for_level(1), Ok(Scheme::Des));
    assert_eq!(Scheme::for_level(2), Ok(Scheme::Sha1));
    assert_eq!(Scheme::for_level(3), Ok(Scheme::Sha1));
    assert_eq!(Scheme::for_level(4), Err(Error::UnsupportedLevel(4)));
    let s = seeds("0000000000000000", "0000000000000000");
    assert_eq!(substitute(4, "A", "B", &s), Err(Error::UnsupportedLevel(4)));
    assert_eq!(Scheme::Des.substitute_len(), 8);
    assert_eq!(Scheme::Sha1.substitute_len(), 20);
}

#[test]
fn passphrases_only_at_sha_levels() {
    let s = seeds("0102030405060708", "1112131415161718");
    let phrase = "correct horse battery staple";
    assert_eq!(substitute(3, "QSATEST", phrase, &s).unwrap().len(), 20);
    assert_eq!(
        substitute(1, "QSATEST", phrase, &s),
        Err(Error::PasswordLength(28))
    );
}

#[test]
fn nine_and_ten_character_des_passwords_and_ids() {
    let s = seeds("0102030405060708", "1112131415161718");
    let a = substitute(0, "TENCHARSID", "NINECHARS", &s).unwrap();
    let b = substitute(0, "TENCHARSID", "NINECHARX", &s).unwrap();
    let c = substitute(0, "TENCHARSIX", "NINECHARS", &s).unwrap();
    // Characters nine and ten count, in the password and the user ID.
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_eq!(a.len(), 8);
}

#[test]
#[cfg(feature = "unverified-change")]
fn change_password_parts_have_the_right_shapes() {
    let s = seeds("0102030405060708", "1112131415161718");
    let c = change(0, "QSATEST", "OLDPW", "NEWPASSWD9", &s).unwrap();
    assert_eq!(c.scheme, Scheme::Des);
    assert_eq!(
        c.substitute,
        substitute(0, "QSATEST", "OLDPW", &s).unwrap(),
        "the substitute is the old password's"
    );
    assert_eq!(c.old_protected.len(), 8);
    assert_eq!(c.new_protected.len(), 16);
    assert_eq!((c.old_len, c.new_len), (5, 10));

    let c = change(2, "QSATEST", "old pass", "a much longer new passphrase", &s).unwrap();
    assert_eq!(c.scheme, Scheme::Sha1);
    assert_eq!(c.substitute.len(), 20);
    assert_eq!(c.old_protected.len(), 20);
    assert_eq!(c.new_protected.len(), 60);
    assert_eq!((c.old_len, c.new_len), (16, 56));
}

#[test]
fn secrets_never_print() {
    let s = seeds("7D4C2319F28004B2", "08BEF662D851F4B1");
    let t = telnet(0, "USER123", "ABCDEFG", &s).unwrap();
    #[cfg(feature = "unverified-change")]
    let c = format!(
        "{:?}",
        change(2, "USER123", "AbCdEfGh123?+", "N3wPassw0rd!", &s).unwrap()
    );
    #[cfg(not(feature = "unverified-change"))]
    let c = String::new();
    let shown = format!("{t:?} {c} {s:?}");
    for secret in [
        "5A58BD50",
        "5a58bd50",
        "90, 88",
        "ABCDEFG",
        "N3wPassw0rd",
        "7D4C",
    ] {
        assert!(!shown.contains(secret), "{secret} in {shown}");
    }
    assert!(shown.contains("Secret(8 bytes)"));
    // Errors never carry the password either.
    let e = substitute(0, "USER123", "BAD PASSWORD", &s).unwrap_err();
    assert!(!format!("{e} {e:?}").contains("BAD"));
}
