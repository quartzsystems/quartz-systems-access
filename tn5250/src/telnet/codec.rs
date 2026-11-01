// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The Telnet byte layer (RFC 854, RFC 885): splitting the stream into
//! records ended by `IAC EOR`, option commands and subnegotiations, and the
//! reverse. `IAC` in data is doubled on the wire.

use super::{DO, DONT, EOR, IAC, SB, SE, WILL, WONT};

/// The longest record kept. A 5250 record carries a 16-bit length, so
/// nothing valid is longer.
pub const MAX_RECORD: usize = 65_535;
/// The longest subnegotiation kept.
pub const MAX_SUBNEG: usize = 4_096;

/// An option verb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Will,
    Wont,
    Do,
    Dont,
}

impl Verb {
    /// The command byte.
    pub fn byte(self) -> u8 {
        match self {
            Verb::Will => WILL,
            Verb::Wont => WONT,
            Verb::Do => DO,
            Verb::Dont => DONT,
        }
    }

    fn from_byte(b: u8) -> Option<Verb> {
        Some(match b {
            WILL => Verb::Will,
            WONT => Verb::Wont,
            DO => Verb::Do,
            DONT => Verb::Dont,
            _ => return None,
        })
    }

    /// Name for logs.
    pub fn name(self) -> &'static str {
        match self {
            Verb::Will => "WILL",
            Verb::Wont => "WONT",
            Verb::Do => "DO",
            Verb::Dont => "DONT",
        }
    }
}

/// Something wrong in the byte stream. Decoding carries on after each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// A record grew past [`MAX_RECORD`]; it is dropped up to its `IAC EOR`.
    RecordTooLong,
    /// A subnegotiation grew past [`MAX_SUBNEG`]; it is dropped.
    SubnegTooLong(u8),
    /// `IAC` followed by a byte other than `SE` or `IAC` inside a
    /// subnegotiation. The subnegotiation is ended there.
    UnterminatedSubneg(u8),
}

/// What the decoder found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    /// A whole record, `IAC` doubling undone, without its `IAC EOR`.
    Record(Vec<u8>),
    /// `IAC WILL|WONT|DO|DONT option`.
    Option(Verb, u8),
    /// `IAC SB option ... IAC SE`, `IAC` doubling undone.
    Subneg(u8, Vec<u8>),
    /// Any other two-byte command: `NOP`, `GA`, `AYT`, a stray `SE` ...
    Command(u8),
    Error(DecodeError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Data,
    Iac,
    Verb(Verb),
    SbOption,
    Sb,
    SbIac,
}

/// A streaming decoder: bytes in, tokens out, in any split.
#[derive(Debug)]
pub struct Decoder {
    state: State,
    record: Vec<u8>,
    overflow: bool,
    sb_option: u8,
    sb: Vec<u8>,
    sb_overflow: bool,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub fn new() -> Decoder {
        Decoder {
            state: State::Data,
            record: Vec::new(),
            overflow: false,
            sb_option: 0,
            sb: Vec::new(),
            sb_overflow: false,
        }
    }

    /// Bytes of the record not yet ended by `IAC EOR`.
    pub fn pending(&self) -> usize {
        self.record.len()
    }

    /// Decodes `bytes`, appending what they complete to `out`.
    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<Token>) {
        for &b in bytes {
            self.state = match self.state {
                State::Data => {
                    if b == IAC {
                        State::Iac
                    } else {
                        self.data(b, out);
                        State::Data
                    }
                }
                State::Iac => self.after_iac(b, out),
                State::Verb(v) => {
                    out.push(Token::Option(v, b));
                    State::Data
                }
                State::SbOption => {
                    self.sb_option = b;
                    self.sb.clear();
                    self.sb_overflow = false;
                    State::Sb
                }
                State::Sb => {
                    if b == IAC {
                        State::SbIac
                    } else {
                        self.sb_byte(b, out);
                        State::Sb
                    }
                }
                State::SbIac => match b {
                    IAC => {
                        self.sb_byte(IAC, out);
                        State::Sb
                    }
                    SE => {
                        self.end_subneg(out);
                        State::Data
                    }
                    other => {
                        out.push(Token::Error(DecodeError::UnterminatedSubneg(
                            self.sb_option,
                        )));
                        self.end_subneg(out);
                        self.after_iac(other, out)
                    }
                },
            };
        }
    }

    fn after_iac(&mut self, b: u8, out: &mut Vec<Token>) -> State {
        if let Some(v) = Verb::from_byte(b) {
            return State::Verb(v);
        }
        match b {
            IAC => {
                self.data(IAC, out);
                State::Data
            }
            EOR => {
                if self.overflow {
                    self.overflow = false;
                    self.record.clear();
                } else {
                    out.push(Token::Record(std::mem::take(&mut self.record)));
                }
                State::Data
            }
            SB => State::SbOption,
            other => {
                out.push(Token::Command(other));
                State::Data
            }
        }
    }

    fn data(&mut self, b: u8, out: &mut Vec<Token>) {
        if self.overflow {
            return;
        }
        if self.record.len() >= MAX_RECORD {
            self.overflow = true;
            self.record = Vec::new();
            out.push(Token::Error(DecodeError::RecordTooLong));
            return;
        }
        self.record.push(b);
    }

    fn sb_byte(&mut self, b: u8, out: &mut Vec<Token>) {
        if self.sb_overflow {
            return;
        }
        if self.sb.len() >= MAX_SUBNEG {
            self.sb_overflow = true;
            self.sb = Vec::new();
            out.push(Token::Error(DecodeError::SubnegTooLong(self.sb_option)));
            return;
        }
        self.sb.push(b);
    }

    fn end_subneg(&mut self, out: &mut Vec<Token>) {
        if self.sb_overflow {
            self.sb_overflow = false;
        } else {
            out.push(Token::Subneg(self.sb_option, std::mem::take(&mut self.sb)));
        }
    }
}

/// Appends `data` to `out` with every `IAC` doubled.
pub fn escape_into(data: &[u8], out: &mut Vec<u8>) {
    for &b in data {
        out.push(b);
        if b == IAC {
            out.push(IAC);
        }
    }
}

/// A record as it goes on the wire: escaped, then `IAC EOR`.
pub fn encode_record(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 2);
    escape_into(data, &mut out);
    out.extend_from_slice(&[IAC, EOR]);
    out
}

/// `IAC verb option`.
pub fn encode_option(verb: Verb, option: u8) -> [u8; 3] {
    [IAC, verb.byte(), option]
}

/// `IAC SB option payload IAC SE`, the payload escaped.
pub fn encode_subneg(option: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 5);
    out.extend_from_slice(&[IAC, SB, option]);
    escape_into(payload, &mut out);
    out.extend_from_slice(&[IAC, SE]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telnet::{GA, NOP, TERMINAL_TYPE};

    fn decode(bytes: &[u8]) -> Vec<Token> {
        let mut d = Decoder::new();
        let mut out = Vec::new();
        d.feed(bytes, &mut out);
        out
    }

    /// Feeds one byte at a time: the result must not depend on the split.
    fn decode_bytewise(bytes: &[u8]) -> Vec<Token> {
        let mut d = Decoder::new();
        let mut out = Vec::new();
        for b in bytes {
            d.feed(std::slice::from_ref(b), &mut out);
        }
        out
    }

    #[test]
    fn records_end_at_iac_eor() {
        let wire = [0x00, 0x0A, 0x12, 0xA0, IAC, EOR, 0x41, IAC, EOR];
        let want = vec![
            Token::Record(vec![0x00, 0x0A, 0x12, 0xA0]),
            Token::Record(vec![0x41]),
        ];
        assert_eq!(decode(&wire), want);
        assert_eq!(decode_bytewise(&wire), want);
    }

    #[test]
    fn doubled_iac_is_one_data_byte() {
        let wire = [0x01, IAC, IAC, 0x02, IAC, IAC, IAC, IAC, IAC, EOR];
        assert_eq!(
            decode(&wire),
            vec![Token::Record(vec![0x01, 0xFF, 0x02, 0xFF, 0xFF])]
        );
    }

    #[test]
    fn options_and_commands_between_and_inside_records() {
        let wire = [
            IAC, DO, 0x27, 0x41, IAC, WILL, 0x19, 0x42, IAC, NOP, IAC, GA, IAC, EOR,
        ];
        assert_eq!(
            decode_bytewise(&wire),
            vec![
                Token::Option(Verb::Do, 0x27),
                Token::Option(Verb::Will, 0x19),
                Token::Command(NOP),
                Token::Command(GA),
                Token::Record(vec![0x41, 0x42]),
            ]
        );
    }

    #[test]
    fn subnegotiation_with_escaped_iac() {
        // IAC SB TERMINAL-TYPE SEND IAC SE, then one with an FF in it.
        let wire = [
            IAC, SB, 24, 1, IAC, SE, IAC, SB, 39, 0x00, IAC, IAC, 0x01, IAC, SE,
        ];
        assert_eq!(
            decode_bytewise(&wire),
            vec![
                Token::Subneg(TERMINAL_TYPE, vec![1]),
                Token::Subneg(39, vec![0x00, 0xFF, 0x01]),
            ]
        );
    }

    #[test]
    fn unterminated_subnegotiation_ends_at_the_next_command() {
        let wire = [IAC, SB, 24, 1, IAC, DO, 0, 0x41, IAC, EOR];
        assert_eq!(
            decode(&wire),
            vec![
                Token::Error(DecodeError::UnterminatedSubneg(24)),
                Token::Subneg(24, vec![1]),
                Token::Option(Verb::Do, 0),
                Token::Record(vec![0x41]),
            ]
        );
    }

    #[test]
    fn oversized_record_is_dropped_and_decoding_resumes() {
        let mut wire = vec![0x40; MAX_RECORD + 10];
        wire.extend_from_slice(&[IAC, EOR, 0x41, IAC, EOR]);
        let got = decode(&wire);
        assert_eq!(
            got,
            vec![
                Token::Error(DecodeError::RecordTooLong),
                Token::Record(vec![0x41]),
            ]
        );
    }

    #[test]
    fn oversized_subnegotiation_is_dropped() {
        let mut wire = vec![IAC, SB, 39];
        wire.extend(std::iter::repeat_n(0x41, MAX_SUBNEG + 1));
        wire.extend_from_slice(&[IAC, SE, IAC, SB, 24, 1, IAC, SE]);
        assert_eq!(
            decode(&wire),
            vec![
                Token::Error(DecodeError::SubnegTooLong(39)),
                Token::Subneg(24, vec![1]),
            ]
        );
    }

    #[test]
    fn encoding_doubles_iac() {
        assert_eq!(
            encode_record(&[0x01, 0xFF, 0x02]),
            vec![0x01, 0xFF, 0xFF, 0x02, IAC, EOR]
        );
        assert_eq!(encode_option(Verb::Wont, 6), [IAC, WONT, 6]);
        assert_eq!(
            encode_subneg(24, &[0, b'I', 0xFF]),
            vec![IAC, SB, 24, 0, b'I', 0xFF, 0xFF, IAC, SE]
        );
    }

    #[test]
    fn encode_then_decode_round_trips() {
        let data: Vec<u8> = (0..=255u8).chain(0..=255u8).collect();
        assert_eq!(
            decode_bytewise(&encode_record(&data)),
            vec![Token::Record(data)]
        );
    }
}
