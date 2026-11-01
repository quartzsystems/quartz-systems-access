// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The General Data Stream record that carries every 5250 exchange
//! (RFC 1205):
//!
//! | Bytes | Content |
//! |---|---|
//! | 0-1 | length of the whole record |
//! | 2-3 | record type `12A0` |
//! | 4-5 | reserved |
//! | 6 | variable header length, normally 4 |
//! | 7-8 | flags |
//! | 9 | operation code |
//! | 10- | data |

/// The GDS record type for 5250.
pub const RECORD_TYPE: u16 = 0x12A0;

/// Header flags.
pub mod flag {
    /// Data stream error (a negative response).
    pub const ERR: u16 = 0x8000;
    /// Attention key.
    pub const ATN: u16 = 0x4000;
    /// System Request key.
    pub const SRQ: u16 = 0x0400;
    /// Test Request key.
    pub const TRQ: u16 = 0x0200;
    /// Help key pressed in an operator error state.
    pub const HLP: u16 = 0x0100;
}

/// Operation codes.
pub mod opcode {
    pub const NO_OP: u8 = 0;
    pub const INVITE: u8 = 1;
    pub const OUTPUT_ONLY: u8 = 2;
    pub const PUT_GET: u8 = 3;
    pub const SAVE_SCREEN: u8 = 4;
    pub const RESTORE_SCREEN: u8 = 5;
    pub const READ_IMMEDIATE: u8 = 6;
    pub const READ_SCREEN: u8 = 8;
    pub const CANCEL_INVITE: u8 = 10;
    pub const MESSAGE_LIGHT_ON: u8 = 11;
    pub const MESSAGE_LIGHT_OFF: u8 = 12;
}

/// A record that cannot be read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    #[error("record of {0} bytes is shorter than its header")]
    Short(usize),
    #[error("record type {0:04X} is not 12A0")]
    WrongType(u16),
    #[error("variable header length {0} is not valid")]
    BadHeader(u8),
}

/// One record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The reserved field; 0 except in a start-up response.
    pub reserved: u16,
    pub flags: u16,
    pub opcode: u8,
    pub data: Vec<u8>,
}

impl Record {
    /// A record with no flags.
    pub fn new(opcode: u8, data: Vec<u8>) -> Record {
        Record {
            reserved: 0,
            flags: 0,
            opcode,
            data,
        }
    }

    /// Reads a record. A length field that disagrees with the bytes is
    /// logged and the bytes are believed.
    pub fn parse(bytes: &[u8]) -> Result<Record, RecordError> {
        if bytes.len() < 10 {
            return Err(RecordError::Short(bytes.len()));
        }
        let len = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
        let rtype = u16::from_be_bytes([bytes[2], bytes[3]]);
        if rtype != RECORD_TYPE {
            return Err(RecordError::WrongType(rtype));
        }
        let var = bytes[6];
        let start = 6 + var as usize;
        if var < 4 || start > bytes.len() {
            return Err(RecordError::BadHeader(var));
        }
        if len != bytes.len() {
            crate::debug!(
                "record: length field {len} but {} bytes arrived",
                bytes.len()
            );
        }
        Ok(Record {
            reserved: u16::from_be_bytes([bytes[4], bytes[5]]),
            flags: u16::from_be_bytes([bytes[7], bytes[8]]),
            opcode: bytes[9],
            data: bytes[start..].to_vec(),
        })
    }

    /// The record's bytes, before Telnet framing.
    pub fn encode(&self) -> Vec<u8> {
        let len = (10 + self.data.len()).min(u16::MAX as usize) as u16;
        let mut out = Vec::with_capacity(len as usize);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&RECORD_TYPE.to_be_bytes());
        out.extend_from_slice(&self.reserved.to_be_bytes());
        out.push(4);
        out.extend_from_slice(&self.flags.to_be_bytes());
        out.push(self.opcode);
        out.extend_from_slice(&self.data);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_and_parse() {
        let r = Record::new(opcode::PUT_GET, vec![0x04, 0x11, 0x00, 0x18]);
        let b = r.encode();
        assert_eq!(&b[..10], &[0x00, 0x0E, 0x12, 0xA0, 0, 0, 4, 0, 0, 3]);
        assert_eq!(Record::parse(&b), Ok(r));
    }

    #[test]
    fn attention_flags() {
        let r = Record {
            flags: flag::ATN,
            ..Record::new(opcode::NO_OP, vec![])
        };
        assert_eq!(r.encode(), [0x00, 0x0A, 0x12, 0xA0, 0, 0, 4, 0x40, 0, 0]);
    }

    #[test]
    fn a_longer_variable_header_is_skipped() {
        let b = [0x00, 0x0C, 0x12, 0xA0, 0x90, 0x00, 5, 0, 0, 0, 0xEE, 0xC9];
        let r = Record::parse(&b).unwrap();
        assert_eq!(r.reserved, 0x9000);
        assert_eq!(r.data, [0xC9]);
    }

    #[test]
    fn malformed_records_are_errors() {
        assert_eq!(Record::parse(&[0; 4]), Err(RecordError::Short(4)));
        assert_eq!(
            Record::parse(&[0, 10, 0x12, 0xA1, 0, 0, 4, 0, 0, 0]),
            Err(RecordError::WrongType(0x12A1))
        );
        assert_eq!(
            Record::parse(&[0, 10, 0x12, 0xA0, 0, 0, 2, 0, 0, 0]),
            Err(RecordError::BadHeader(2))
        );
        assert_eq!(
            Record::parse(&[0, 10, 0x12, 0xA0, 0, 0, 9, 0, 0, 0]),
            Err(RecordError::BadHeader(9))
        );
    }
}
