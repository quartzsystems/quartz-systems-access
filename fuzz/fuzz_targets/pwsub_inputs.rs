// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Password substitution with any level, user ID, passwords and seeds.
//!
//! Input: a level byte, 16 seed bytes, then the user ID, the old password
//! and the new password as UTF-8 (lossy), separated by zero bytes.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 17 {
        return;
    }
    let level = data[0] % 6;
    let seeds = pwsub::Seeds {
        server: data[1..9].try_into().unwrap(),
        client: data[9..17].try_into().unwrap(),
    };
    let text = String::from_utf8_lossy(&data[17..]);
    let mut parts = text.split('\0');
    let user = parts.next().unwrap_or("");
    let old = parts.next().unwrap_or("");
    let new = parts.next().unwrap_or("");
    let scheme = pwsub::Scheme::for_level(level);
    match pwsub::substitute(level, user, old, &seeds) {
        Ok(s) => {
            assert_eq!(s.len(), scheme.unwrap().substitute_len());
            // The same inputs give the same substitute.
            assert_eq!(pwsub::substitute(level, user, old, &seeds).unwrap(), s);
            let t = pwsub::telnet(level, user, old, &seeds).unwrap();
            assert_eq!(t.substitute, s);
        }
        Err(pwsub::Error::UnsupportedLevel(l)) => assert!(l >= 4),
        Err(_) => {}
    }
    if let Ok(c) = pwsub::change(level, user, old, new, &seeds) {
        let block = c.scheme.substitute_len();
        assert_eq!(c.substitute.len(), block);
        // Whole blocks: 8 bytes for DES, 20 for SHA-1.
        assert!(c.old_protected.len().is_multiple_of(block) && !c.old_protected.is_empty());
        assert!(c.new_protected.len().is_multiple_of(block) && !c.new_protected.is_empty());
        let _ = pwsub::user_id(user).unwrap();
    }
});
