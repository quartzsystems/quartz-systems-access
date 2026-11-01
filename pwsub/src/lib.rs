// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Password substitutes: what a client sends instead of a password, for the
//! host servers' sign-on and for TN5250E automatic sign-on (RFC 4777
//! section 5).
//!
//! Pure functions: no I/O, no randomness, no logging, no UI. The caller
//! supplies both seeds (the client seed from a good random source) and
//! sends the results. Every buffer that holds a password, a password token
//! or a substitute is overwritten when dropped, and no type here shows its
//! secret in `Debug`.
//!
//! The server reports a password level; it selects the algorithm
//! ([`Scheme::for_level`]):
//!
//! | Level | Scheme | Password |
//! |---|---|---|
//! | 0, 1 | [`Scheme::Des`]: RFC 4777 section 5.1 | 1 to 10 characters, upper case |
//! | 2, 3 | [`Scheme::Sha1`]: RFC 4777 section 5.2 | 1 to 128 characters, case kept |
//! | 4 and up | not supported: [`Error::UnsupportedLevel`] | |
//!
//! - [`substitute`]: the substitute for a sign-on (sequence number 1).
//! - [`telnet`]: the same, in the form TN5250E automatic sign-on sends
//!   (user, client seed, substitute).
//! - `change` (feature `unverified-change`, off by default): the
//!   substitute and the protected old and new passwords of a
//!   change-password request. Not yet checked against a real system.
//! - [`user_id`]: a user ID as the servers want it: ten EBCDIC bytes.

mod des;
mod ebcdic;
mod sha;

use std::fmt;

use zeroize::{Zeroize, Zeroizing};

pub use ebcdic::user_id;

/// The longest password at levels 0 and 1, after any `Q` prefix.
pub const DES_MAX_PASSWORD: usize = 10;
/// The longest password (passphrase) at levels 2 and 3, in characters.
pub const SHA_MAX_PASSWORD: usize = 128;
/// The longest user ID, in characters.
pub const MAX_USER_ID: usize = 10;

/// Why a substitute could not be made. Never carries the password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The password level selects an algorithm this crate does not have.
    UnsupportedLevel(u8),
    /// The user ID is empty or longer than ten characters.
    UserIdLength(usize),
    /// The user ID has a character outside A to Z, 0 to 9, `$`, `#`, `@`
    /// and `_`.
    UserIdCharacter,
    /// The password is empty or too long for the level (length in
    /// characters, after any `Q` prefix at levels 0 and 1).
    PasswordLength(usize),
    /// The password has a character the level cannot take: at levels 0 and
    /// 1, anything outside A to Z, 0 to 9, `$`, `#`, `@` and `_` (after
    /// upper-casing); at levels 2 and 3, a leading `*`.
    PasswordCharacter,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnsupportedLevel(l) => write!(f, "password level {l} is not supported"),
            Error::UserIdLength(n) => write!(f, "user ID of {n} characters is not 1 to 10"),
            Error::UserIdCharacter => f.write_str("user ID has a character that is not valid"),
            Error::PasswordLength(n) => {
                write!(f, "password of {n} characters is not valid for the level")
            }
            Error::PasswordCharacter => {
                f.write_str("password has a character that is not valid for the level")
            }
        }
    }
}

impl std::error::Error for Error {}

/// The algorithm a password level selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    /// DES, 8-byte substitutes: levels 0 and 1.
    Des,
    /// SHA-1, 20-byte substitutes: levels 2 and 3.
    Sha1,
}

impl Scheme {
    /// The scheme for the password level a server reports.
    pub fn for_level(level: u8) -> Result<Scheme, Error> {
        match level {
            0 | 1 => Ok(Scheme::Des),
            2 | 3 => Ok(Scheme::Sha1),
            l => Err(Error::UnsupportedLevel(l)),
        }
    }

    /// Bytes in a substitute.
    pub fn substitute_len(self) -> usize {
        match self {
            Scheme::Des => 8,
            Scheme::Sha1 => 20,
        }
    }
}

/// The two random seeds of an exchange. Not secret on their own, but kept
/// out of recordings with the substitutes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Seeds {
    /// The server's seed (RFC 4777's RDr).
    pub server: [u8; 8],
    /// The client's seed (RDs).
    pub client: [u8; 8],
}

impl fmt::Debug for Seeds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Seeds(***)")
    }
}

/// Secret bytes: a substitute or a protected password. Overwritten when
/// dropped; `Debug` shows only the length.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Zeroizing<Vec<u8>>);

impl Secret {
    fn new(bytes: Vec<u8>) -> Secret {
        Secret(Zeroizing::new(bytes))
    }

    /// Takes ownership of secret bytes: a request being built, or a
    /// substitute read from a request (on a server).
    pub fn from_vec(bytes: Vec<u8>) -> Secret {
        Secret::new(bytes)
    }

    /// The bytes, for the request being built.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({} bytes)", self.0.len())
    }
}

/// The substitute for signing on as `user` with `password` at the server's
/// password `level`, sequence number 1 (the host servers and the Telnet
/// server always use 1).
pub fn substitute(level: u8, user: &str, password: &str, seeds: &Seeds) -> Result<Secret, Error> {
    match Scheme::for_level(level)? {
        Scheme::Des => {
            let uid = ebcdic::user_id(user)?;
            let pw = ebcdic::des_password(password)?;
            let token = des::token(&uid, &pw);
            Ok(Secret::new(
                des::substitute(&token, &uid, seeds, 1).to_vec(),
            ))
        }
        Scheme::Sha1 => {
            let uid = sha::user_id(user)?;
            let pw = sha::password(password)?;
            let token = sha::token(&uid, &pw);
            Ok(Secret::new(
                sha::substitute(&token, &uid, seeds, 1).to_vec(),
            ))
        }
    }
}

/// What TN5250E automatic sign-on sends (RFC 4777 section 5): the `USER`
/// variable, the client seed, and the substitute. Never a plain-text
/// password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelnetSignOn {
    /// The user ID, upper case, for the `USER` variable.
    pub user: String,
    /// The client seed, for the seed variable's value.
    pub client_seed: [u8; 8],
    /// The substitute, for the substitute variable's value: 8 bytes (DES)
    /// or 20 (SHA-1).
    pub substitute: Secret,
}

/// The encrypted automatic sign-on for a Telnet server whose seed is
/// `seeds.server`, at the system's password `level` (which Telnet does not
/// report: the sign-on server's exchange of attributes does).
pub fn telnet(level: u8, user: &str, password: &str, seeds: &Seeds) -> Result<TelnetSignOn, Error> {
    let substitute = substitute(level, user, password, seeds)?;
    Ok(TelnetSignOn {
        user: user.trim().to_ascii_uppercase(),
        client_seed: seeds.client,
        substitute,
    })
}

/// UNVERIFIED. The change-password protected forms are not in RFC 4777.
/// They were written from the author's prose notes taken after reading
/// the open-source Java toolbox's sign-on code (as a specification, no
/// code copied), and have not been checked against a real system. They
/// need a clean re-derivation and capture vectors (see
/// `hostsvr-proto/SIGNON-SPEC.md`) before the feature is turned on by
/// default.
#[cfg(feature = "unverified-change")]
/// The secret parts of a change-password request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub scheme: Scheme,
    /// The substitute for the old password (sequence number 1).
    pub substitute: Secret,
    /// The old password, protected with the new password's token.
    pub old_protected: Secret,
    /// The new password, protected with the old password's token.
    pub new_protected: Secret,
    /// Length of the old password as sent: bytes of UTF-16 at levels 2 and
    /// 3, characters at levels 0 and 1.
    pub old_len: u32,
    /// Length of the new password, likewise.
    pub new_len: u32,
}

/// UNVERIFIED. The change-password protected forms are not in RFC 4777.
/// They were written from the author's prose notes taken after reading
/// the open-source Java toolbox's sign-on code (as a specification, no
/// code copied), and have not been checked against a real system. They
/// need a clean re-derivation and capture vectors (see
/// `hostsvr-proto/SIGNON-SPEC.md`) before the feature is turned on by
/// default.
///
/// The parts of a change-password request from `old` to `new` for `user`
/// at password `level`. Whether the new password meets the system's rules
/// is the server's decision, not this function's.
#[cfg(feature = "unverified-change")]
pub fn change(level: u8, user: &str, old: &str, new: &str, seeds: &Seeds) -> Result<Change, Error> {
    let scheme = Scheme::for_level(level)?;
    match scheme {
        Scheme::Des => {
            let uid = ebcdic::user_id(user)?;
            let old_pw = ebcdic::des_password(old)?;
            let new_pw = ebcdic::des_password(new)?;
            let parts = des::change(&uid, &old_pw, &new_pw, seeds);
            Ok(Change {
                scheme,
                substitute: Secret::new(parts.substitute.to_vec()),
                old_protected: Secret::new(parts.old_protected),
                new_protected: Secret::new(parts.new_protected),
                old_len: old_pw.len as u32,
                new_len: new_pw.len as u32,
            })
        }
        Scheme::Sha1 => {
            let uid = sha::user_id(user)?;
            let old_pw = sha::password(old)?;
            let new_pw = sha::password(new)?;
            let parts = sha::change(&uid, &old_pw, &new_pw, seeds);
            Ok(Change {
                scheme,
                substitute: Secret::new(parts.substitute.to_vec()),
                old_protected: Secret::new(parts.old_protected),
                new_protected: Secret::new(parts.new_protected),
                old_len: old_pw.len() as u32,
                new_len: new_pw.len() as u32,
            })
        }
    }
}

/// Adds `n` to an 8-byte big-endian number, ignoring overflow.
fn add_be(a: &[u8; 8], n: u64) -> [u8; 8] {
    u64::from_be_bytes(*a).wrapping_add(n).to_be_bytes()
}

/// XOR of two 8-byte blocks.
fn xor8(a: &[u8; 8], b: &[u8; 8]) -> [u8; 8] {
    let mut out = [0u8; 8];
    for i in 0..8 {
        out[i] = a[i] ^ b[i];
    }
    out
}

/// Overwrites a fixed array when dropped.
struct Wiped<const N: usize>([u8; N]);

impl<const N: usize> Drop for Wiped<N> {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests;
