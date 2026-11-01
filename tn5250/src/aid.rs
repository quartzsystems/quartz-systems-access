// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Attention identifiers: the byte an inbound record names its key with.

pub const ENTER: u8 = 0xF1;
pub const HELP: u8 = 0xF3;
/// Page Up.
pub const ROLL_DOWN: u8 = 0xF4;
/// Page Down.
pub const ROLL_UP: u8 = 0xF5;
pub const PRINT: u8 = 0xF6;
pub const RECORD_BACKSPACE: u8 = 0xF8;
pub const CLEAR: u8 = 0xBD;
/// Sent when the cursor leaves an auto-enter field.
pub const AUTO_ENTER: u8 = 0x3F;
/// The reply to a Query.
pub const INBOUND_WSF: u8 = 0x88;

/// F1 to F24.
pub fn function(n: u8) -> Option<u8> {
    match n {
        1..=12 => Some(0x30 + n),
        13..=24 => Some(0xB0 + n - 12),
        _ => None,
    }
}

/// The function key number of an AID, 1 to 24.
pub fn function_number(aid: u8) -> Option<u8> {
    match aid {
        0x31..=0x3C => Some(aid - 0x30),
        0xB1..=0xBC => Some(aid - 0xB0 + 12),
        _ => None,
    }
}

/// Keys that send only the cursor and the AID, never field data.
pub fn sends_no_data(aid: u8) -> bool {
    matches!(aid, CLEAR | HELP | PRINT | RECORD_BACKSPACE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_keys() {
        assert_eq!(function(1), Some(0x31));
        assert_eq!(function(12), Some(0x3C));
        assert_eq!(function(13), Some(0xB1));
        assert_eq!(function(24), Some(0xBC));
        assert_eq!(function(25), None);
        for n in 1..=24 {
            assert_eq!(function_number(function(n).unwrap()), Some(n));
        }
        assert!(sends_no_data(CLEAR) && !sends_no_data(ENTER));
    }
}
