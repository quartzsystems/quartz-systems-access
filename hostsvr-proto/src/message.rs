// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! System messages a server returns with a failure, when the client asked
//! for them: a count parameter, then one parameter per message.
//!
//! A message's data: text CCSID (u32), substitution data CCSID (u32),
//! severity (u16), then fields each prefixed with a u32 length: the type
//! (two EBCDIC digits), the message ID, the message file, its library, the
//! first-level text, and the substitution data. Anything after those is
//! kept as it came (second-level help, where a server sends it).
//!
//! The texts stay as bytes in their CCSID: the caller decodes them with
//! its code pages.

use crate::frame::{Frame, be16, be32};
use crate::{DecodeError, wire};

/// One message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostMessage {
    pub text_ccsid: u32,
    pub substitution_ccsid: u32,
    pub severity: u16,
    /// The message type, two EBCDIC digits as sent.
    pub kind: Vec<u8>,
    /// The message ID (seven EBCDIC characters, such as a CPF ID).
    pub id: Vec<u8>,
    pub file: Vec<u8>,
    pub library: Vec<u8>,
    /// First-level text, in `text_ccsid`.
    pub text: Vec<u8>,
    pub substitution: Vec<u8>,
    /// What follows the substitution data, unread.
    pub rest: Vec<u8>,
}

impl HostMessage {
    fn fields(&self) -> [&Vec<u8>; 6] {
        [
            &self.kind,
            &self.id,
            &self.file,
            &self.library,
            &self.text,
            &self.substitution,
        ]
    }

    /// The parameter data.
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&self.text_ccsid.to_be_bytes());
        v.extend_from_slice(&self.substitution_ccsid.to_be_bytes());
        v.extend_from_slice(&self.severity.to_be_bytes());
        for f in self.fields() {
            v.extend_from_slice(&(f.len() as u32).to_be_bytes());
            v.extend_from_slice(f);
        }
        v.extend_from_slice(&self.rest);
        v
    }

    /// From the parameter data.
    pub fn decode(data: &[u8]) -> Result<HostMessage, DecodeError> {
        let bad = || DecodeError::Value {
            cp: wire::CP_MESSAGE,
            len: data.len(),
        };
        if data.len() < 10 {
            return Err(bad());
        }
        let mut m = HostMessage {
            text_ccsid: be32(&data[0..4]),
            substitution_ccsid: be32(&data[4..8]),
            severity: be16(&data[8..10]),
            ..HostMessage::default()
        };
        let mut at = 10;
        let mut take = || -> Result<Vec<u8>, DecodeError> {
            if data.len() < at + 4 {
                return Err(bad());
            }
            let n = be32(&data[at..at + 4]) as usize;
            let start = at + 4;
            let end = start
                .checked_add(n)
                .filter(|&e| e <= data.len())
                .ok_or_else(bad)?;
            at = end;
            Ok(data[start..end].to_vec())
        };
        m.kind = take()?;
        m.id = take()?;
        m.file = take()?;
        m.library = take()?;
        m.text = take()?;
        m.substitution = take()?;
        m.rest = data[at..].to_vec();
        Ok(m)
    }
}

/// The messages in a reply. A count parameter that disagrees with the
/// messages present is believed less than the messages themselves.
pub fn from_frame(frame: &Frame) -> Result<Vec<HostMessage>, DecodeError> {
    frame
        .params_with(wire::CP_MESSAGE)
        .map(HostMessage::decode)
        .collect()
}

/// Adds the count and the messages to a reply (nothing when there are
/// none).
pub fn push_into(frame: &mut Frame, messages: &[HostMessage]) {
    if messages.is_empty() {
        return;
    }
    frame.push(
        wire::CP_MESSAGE_COUNT,
        (messages.len() as u16).to_be_bytes(),
    );
    for m in messages {
        frame.push(wire::CP_MESSAGE, m.encode());
    }
}

#[cfg(test)]
pub(crate) fn sample() -> HostMessage {
    HostMessage {
        text_ccsid: 37,
        substitution_ccsid: 37,
        severity: 40,
        kind: vec![0xF0, 0xF2],
        id: vec![0xC3, 0xD7, 0xC6, 0xF2, 0xF2, 0xF0, 0xF4],
        file: b"QCPFMSG   ".to_vec(),
        library: b"QSYS      ".to_vec(),
        text: vec![0xE3, 0xC5, 0xE7, 0xE3],
        substitution: vec![1, 2, 3],
        rest: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Header;

    #[test]
    fn messages_round_trip_alone_and_in_a_frame() {
        let m = sample();
        assert_eq!(HostMessage::decode(&m.encode()), Ok(m.clone()));
        let with_rest = HostMessage {
            rest: vec![0, 0, 0, 0],
            ..m.clone()
        };
        assert_eq!(HostMessage::decode(&with_rest.encode()), Ok(with_rest));
        let mut f = Frame::new(Header::new(0xE009, 0xF004), vec![0; 4]);
        push_into(&mut f, &[m.clone(), m.clone()]);
        let back = Frame::decode(&f.encode()).unwrap();
        assert_eq!(back.param(wire::CP_MESSAGE_COUNT), Some(&[0u8, 2][..]));
        assert_eq!(from_frame(&back).unwrap(), vec![m.clone(), m]);
    }

    #[test]
    fn truncated_messages_are_errors() {
        let b = sample().encode();
        for n in [0, 9, 13, b.len() - 1] {
            assert!(HostMessage::decode(&b[..n]).is_err(), "{n}");
        }
        // A field length that overflows.
        let mut bad = b.clone();
        bad[10..14].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(HostMessage::decode(&bad).is_err());
    }
}
