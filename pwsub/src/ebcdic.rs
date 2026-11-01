// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! User IDs and level 0 and 1 passwords in EBCDIC. Both may only hold A to
//! Z, 0 to 9, `$`, `#`, `@` and `_`, which have the same code points in
//! every single-byte EBCDIC code page the systems use for names (CCSID 37
//! values below), so no code page table is needed.

use crate::{DES_MAX_PASSWORD, Error, MAX_USER_ID, Wiped};

/// The EBCDIC blank, which pads names and passwords.
pub(crate) const BLANK: u8 = 0x40;

/// One name character, upper case, in EBCDIC.
fn name_char(c: char) -> Option<u8> {
    let c = c.to_ascii_uppercase();
    let b = c as u32;
    Some(match c {
        'A'..='I' => 0xC1 + (b - 'A' as u32) as u8,
        'J'..='R' => 0xD1 + (b - 'J' as u32) as u8,
        'S'..='Z' => 0xE2 + (b - 'S' as u32) as u8,
        '0'..='9' => 0xF0 + (b - '0' as u32) as u8,
        '$' => 0x5B,
        '#' => 0x7B,
        '@' => 0x7C,
        '_' => 0x6D,
        _ => return None,
    })
}

/// A user ID as the servers want it: upper case, EBCDIC, padded with
/// blanks to ten bytes. Surrounding spaces are ignored.
pub fn user_id(user: &str) -> Result<[u8; MAX_USER_ID], Error> {
    let user = user.trim();
    let n = user.chars().count();
    if n == 0 || n > MAX_USER_ID {
        return Err(Error::UserIdLength(n));
    }
    let mut out = [BLANK; MAX_USER_ID];
    for (i, c) in user.chars().enumerate() {
        out[i] = name_char(c).ok_or(Error::UserIdCharacter)?;
    }
    Ok(out)
}

/// A level 0 or 1 password: upper case, EBCDIC, padded with blanks to ten
/// bytes.
pub(crate) struct DesPassword {
    pub bytes: Wiped<10>,
    /// Characters, after any `Q` prefix.
    pub len: usize,
}

/// Converts a level 0 or 1 password. A password that starts with a digit
/// is stored by the system with a `Q` in front, so it is sent that way.
pub(crate) fn des_password(password: &str) -> Result<DesPassword, Error> {
    let q = password.starts_with(|c: char| c.is_ascii_digit());
    let len = password.chars().count() + usize::from(q);
    if password.is_empty() || len > DES_MAX_PASSWORD {
        return Err(Error::PasswordLength(len));
    }
    let mut out = DesPassword {
        bytes: Wiped([BLANK; 10]),
        len,
    };
    let mut i = 0;
    if q {
        out.bytes.0[0] = 0xD8;
        i = 1;
    }
    for c in password.chars() {
        out.bytes.0[i] = name_char(c).ok_or(Error::PasswordCharacter)?;
        i += 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_ids_are_upper_case_ebcdic_padded_to_ten() {
        assert_eq!(
            user_id(" user123 ").unwrap(),
            [0xE4, 0xE2, 0xC5, 0xD9, 0xF1, 0xF2, 0xF3, 0x40, 0x40, 0x40]
        );
        assert_eq!(
            user_id("$#@_JRSZ09").unwrap(),
            [0x5B, 0x7B, 0x7C, 0x6D, 0xD1, 0xD9, 0xE2, 0xE9, 0xF0, 0xF9]
        );
        assert_eq!(user_id(""), Err(Error::UserIdLength(0)));
        assert_eq!(user_id("ELEVENCHARS"), Err(Error::UserIdLength(11)));
        assert_eq!(user_id("A-B"), Err(Error::UserIdCharacter));
        assert_eq!(user_id("ÄBC"), Err(Error::UserIdCharacter));
    }

    #[test]
    fn numeric_passwords_get_a_q() {
        let p = des_password("123abc").unwrap();
        assert_eq!(p.len, 7);
        assert_eq!(&p.bytes.0[..7], &[0xD8, 0xF1, 0xF2, 0xF3, 0xC1, 0xC2, 0xC3]);
        assert_eq!(&p.bytes.0[7..], &[0x40; 3]);
        // Ten digits become eleven characters: too long.
        assert!(matches!(
            des_password("1234567890"),
            Err(Error::PasswordLength(11))
        ));
        assert!(matches!(des_password(""), Err(Error::PasswordLength(0))));
        assert!(matches!(
            des_password("pass word"),
            Err(Error::PasswordCharacter)
        ));
    }
}
