// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! SHA-1 password substitutes, password levels 2 and 3 (RFC 4777 section
//! 5.2, with FIPS 180-1 SHA-1).

use sha1::{Digest, Sha1};
use zeroize::Zeroizing;

use crate::{Error, MAX_USER_ID, SHA_MAX_PASSWORD, Seeds};

/// The user ID as UTF-16 big-endian: upper case, padded with spaces to ten
/// characters, so always 20 bytes.
pub(crate) fn user_id(user: &str) -> Result<[u8; 2 * MAX_USER_ID], Error> {
    // The same characters and length rules as the EBCDIC form.
    crate::ebcdic::user_id(user)?;
    let padded = format!("{:<10}", user.trim().to_ascii_uppercase());
    let mut out = [0u8; 2 * MAX_USER_ID];
    for (i, u) in padded.encode_utf16().enumerate() {
        out[2 * i..2 * i + 2].copy_from_slice(&u.to_be_bytes());
    }
    Ok(out)
}

/// The password as UTF-16 big-endian, case kept, trailing spaces removed.
/// It may not be empty or start with `*` (the system's special values).
pub(crate) fn password(password: &str) -> Result<Zeroizing<Vec<u8>>, Error> {
    let p = password.trim_end_matches(' ');
    let n = p.chars().count();
    if n == 0 || n > SHA_MAX_PASSWORD {
        return Err(Error::PasswordLength(n));
    }
    if p.starts_with('*') {
        return Err(Error::PasswordCharacter);
    }
    let mut out = Zeroizing::new(Vec::with_capacity(2 * n));
    for u in p.encode_utf16() {
        out.extend_from_slice(&u.to_be_bytes());
    }
    Ok(out)
}

/// Step 3: `SHA-1(user ID, password)`.
pub(crate) fn token(uid: &[u8; 20], password: &[u8]) -> Zeroizing<[u8; 20]> {
    let mut h = Sha1::new();
    h.update(uid);
    h.update(password);
    Zeroizing::new(h.finalize().into())
}

/// Step 5: `SHA-1(token, server seed, client seed, user ID, sequence)`.
pub(crate) fn substitute(token: &[u8; 20], uid: &[u8; 20], seeds: &Seeds, seq: u64) -> [u8; 20] {
    let mut h = Sha1::new();
    h.update(token);
    h.update(seeds.server);
    h.update(seeds.client);
    h.update(uid);
    h.update(seq.to_be_bytes());
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_ids_are_twenty_bytes_of_utf16() {
        let u = user_id("user123").unwrap();
        assert_eq!(&u[..4], &[0, b'U', 0, b'S']);
        assert_eq!(&u[14..], &[0, b' ', 0, b' ', 0, b' ']);
    }

    #[test]
    fn passwords_keep_case_and_lose_trailing_spaces() {
        assert_eq!(&password("aB  ").unwrap()[..], &[0, b'a', 0, b'B']);
        // A passphrase may hold spaces and other characters.
        assert_eq!(password("a b€").unwrap().len(), 8);
        assert_eq!(password("   "), Err(Error::PasswordLength(0)));
        assert_eq!(password("*NONE"), Err(Error::PasswordCharacter));
        assert!(password(&"x".repeat(128)).is_ok());
        assert_eq!(password(&"x".repeat(129)), Err(Error::PasswordLength(129)));
    }
}
