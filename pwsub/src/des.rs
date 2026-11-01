// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! DES password substitutes, password levels 0 and 1 (RFC 4777 section
//! 5.1, with FIPS 46-2 DES).

use des::Des;
use des::cipher::{BlockEncrypt, KeyInit};
use zeroize::Zeroize;

use crate::ebcdic::{BLANK, DesPassword};
use crate::{Seeds, Wiped, add_be, xor8};

/// One DES block, ECB.
fn encrypt(key: &[u8; 8], block: &[u8; 8]) -> [u8; 8] {
    let cipher = Des::new_from_slice(key).expect("eight-byte key");
    let mut b = (*block).into();
    cipher.encrypt_block(&mut b);
    b.into()
}

/// Steps 1 to 3: the DES key from eight password bytes: XOR with `55`,
/// then the whole value shifted left one bit.
fn key(password: &[u8; 8]) -> Wiped<8> {
    let mut v = u64::from_be_bytes(*password) ^ 0x5555_5555_5555_5555;
    let k = Wiped((v << 1).to_be_bytes());
    v.zeroize();
    k
}

/// Step 4's data: the user ID's first eight bytes, with bytes 9 and 10
/// folded into the top two bits of each when the ID is longer than eight.
fn folded(uid: &[u8; 10]) -> [u8; 8] {
    let mut out: [u8; 8] = uid[..8].try_into().expect("eight bytes");
    if uid[8] != BLANK || uid[9] != BLANK {
        for (half, &extra) in [uid[8], uid[9]].iter().enumerate() {
            for pair in 0..4 {
                let bits = (extra << (2 * pair)) & 0xC0;
                out[half * 4 + pair] ^= bits;
            }
        }
    }
    out
}

/// Step 4: the password token. Passwords of nine or ten characters XOR the
/// tokens of their first eight and their last two characters (step 8).
pub(crate) fn token(uid: &[u8; 10], password: &DesPassword) -> Wiped<8> {
    let id = folded(uid);
    let first: [u8; 8] = password.bytes.0[..8].try_into().expect("eight bytes");
    let mut t = Wiped(encrypt(&key(&first).0, &id));
    if password.len > 8 {
        let mut rest = Wiped([BLANK; 8]);
        rest.0[..2].copy_from_slice(&password.bytes.0[8..10]);
        let b = Wiped(encrypt(&key(&rest.0).0, &id));
        t = Wiped(xor8(&t.0, &b.0));
    }
    t
}

/// Steps 6 and 7 with sequence number `seq`: the DES CBC MAC (initial
/// value zero) keyed with the token over the server seed plus `seq`, the
/// client seed, the user ID padded to 16 bytes and XORed half by half
/// with that sum, and `seq`. The last block is the substitute.
pub(crate) fn substitute(token: &Wiped<8>, uid: &[u8; 10], seeds: &Seeds, seq: u64) -> [u8; 8] {
    let rdr_seq = add_be(&seeds.server, seq);
    let id1: [u8; 8] = uid[..8].try_into().expect("eight bytes");
    let mut id2 = [BLANK; 8];
    id2[..2].copy_from_slice(&uid[8..10]);
    let blocks = [
        rdr_seq,
        seeds.client,
        xor8(&id1, &rdr_seq),
        xor8(&id2, &rdr_seq),
        seq.to_be_bytes(),
    ];
    let mut mac = [0u8; 8];
    for b in &blocks {
        mac = encrypt(&token.0, &xor8(&mac, b));
    }
    mac
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_xor_55_shifted_left() {
        // 'A' (C1) XOR 55 is 94; shifted left across bytes.
        let k = key(&[0xC1, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40]);
        assert_eq!(k.0, [0x28, 0x2A, 0x2A, 0x2A, 0x2A, 0x2A, 0x2A, 0x2A]);
    }

    #[test]
    fn long_user_ids_are_folded() {
        // An eight-character ID is not changed.
        let mut uid = [0xC1; 10];
        uid[8] = BLANK;
        uid[9] = BLANK;
        assert_eq!(folded(&uid), [0xC1; 8]);
        // Byte 9 = 11 01 10 00, byte 10 = 00 00 00 11.
        uid[8] = 0b1101_1000;
        uid[9] = 0b0000_0011;
        assert_eq!(
            folded(&uid),
            [
                0xC1 ^ 0xC0,
                0xC1 ^ 0x40,
                0xC1 ^ 0x80,
                0xC1,
                0xC1,
                0xC1,
                0xC1,
                0xC1 ^ 0xC0
            ]
        );
    }
}
