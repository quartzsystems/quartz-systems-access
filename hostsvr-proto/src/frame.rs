// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! One data stream unit: the 20-byte common header, a fixed template, then
//! LL/CP parameters.
//!
//! | Bytes | Field |
//! |---|---|
//! | 0-3 | total length, header included |
//! | 4-5 | header ID (byte 4: client attributes, byte 5: server attributes, in some exchanges) |
//! | 6-7 | server ID |
//! | 8-11 | client/server instance |
//! | 12-15 | correlation ID |
//! | 16-17 | template length |
//! | 18-19 | request or reply ID |
//! | 20- | template, then parameters |
//!
//! A parameter is `LL` (u32, counting itself, the code point and the data),
//! `CP` (u16), then the data. Everything is big-endian.

use crate::DecodeError;

/// Bytes in the common header.
pub const HEADER_LEN: usize = 20;
/// Bytes in a parameter's LL and CP.
pub const PARAM_HEADER_LEN: usize = 6;
/// The longest frame this crate reads: far more than any sign-on or
/// central reply needs, small enough that a bad length cannot make a reader
/// allocate without bound.
pub const MAX_FRAME_LEN: u32 = 16 * 1024 * 1024;

/// The common header, less the two lengths (which encoding computes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub header_id: u16,
    pub server_id: u16,
    pub instance: u32,
    pub correlation: u32,
    pub request_id: u16,
}

impl Header {
    /// A header for `server_id` and `request_id`, everything else zero.
    pub fn new(server_id: u16, request_id: u16) -> Header {
        Header {
            server_id,
            request_id,
            ..Header::default()
        }
    }

    /// The header ID's first byte: client attributes.
    pub fn client_attributes(&self) -> u8 {
        (self.header_id >> 8) as u8
    }

    /// The header ID's second byte: server attributes.
    pub fn server_attributes(&self) -> u8 {
        self.header_id as u8
    }
}

/// One LL/CP parameter.
#[derive(Clone, PartialEq, Eq)]
pub struct Param {
    pub cp: u16,
    pub data: Vec<u8>,
}

impl std::fmt::Debug for Param {
    // Parameters can hold substitutes: only their size is shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Param({:04X}, {} bytes)", self.cp, self.data.len())
    }
}

impl Drop for Param {
    fn drop(&mut self) {
        wipe(&mut self.data);
    }
}

/// Overwrites bytes that may have held a secret.
pub(crate) fn wipe(bytes: &mut [u8]) {
    bytes.fill(0);
    std::hint::black_box(bytes);
}

/// A whole unit: header, template and parameters.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub header: Header,
    pub template: Vec<u8>,
    pub params: Vec<Param>,
}

impl std::fmt::Debug for Frame {
    // Templates can hold seeds: only their size is shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("header", &self.header)
            .field("template", &format_args!("{} bytes", self.template.len()))
            .field("params", &self.params)
            .finish()
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        wipe(&mut self.template);
    }
}

impl Frame {
    pub fn new(header: Header, template: Vec<u8>) -> Frame {
        Frame {
            header,
            template,
            params: Vec::new(),
        }
    }

    /// Adds a parameter.
    pub fn push(&mut self, cp: u16, data: impl Into<Vec<u8>>) -> &mut Frame {
        self.params.push(Param {
            cp,
            data: data.into(),
        });
        self
    }

    /// The first parameter with code point `cp`.
    pub fn param(&self, cp: u16) -> Option<&[u8]> {
        self.params
            .iter()
            .find(|p| p.cp == cp)
            .map(|p| p.data.as_slice())
    }

    /// Every parameter with code point `cp`, in order.
    pub fn params_with(&self, cp: u16) -> impl Iterator<Item = &[u8]> {
        self.params
            .iter()
            .filter(move |p| p.cp == cp)
            .map(|p| p.data.as_slice())
    }

    /// Bytes the frame encodes to.
    pub fn len(&self) -> usize {
        HEADER_LEN
            + self.template.len()
            + self
                .params
                .iter()
                .map(|p| PARAM_HEADER_LEN + p.data.len())
                .sum::<usize>()
    }

    /// Never true: a frame has at least its header.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The frame's bytes. The caller wipes them when they held a secret
    /// (the typed requests that carry one do it for you).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len());
        out.extend_from_slice(&(self.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.header.header_id.to_be_bytes());
        out.extend_from_slice(&self.header.server_id.to_be_bytes());
        out.extend_from_slice(&self.header.instance.to_be_bytes());
        out.extend_from_slice(&self.header.correlation.to_be_bytes());
        out.extend_from_slice(&(self.template.len() as u16).to_be_bytes());
        out.extend_from_slice(&self.header.request_id.to_be_bytes());
        out.extend_from_slice(&self.template);
        for p in &self.params {
            out.extend_from_slice(&((PARAM_HEADER_LEN + p.data.len()) as u32).to_be_bytes());
            out.extend_from_slice(&p.cp.to_be_bytes());
            out.extend_from_slice(&p.data);
        }
        out
    }

    /// Reads one whole frame. The length field must match `bytes`; a
    /// trailing partial parameter is an error, not ignored.
    pub fn decode(bytes: &[u8]) -> Result<Frame, DecodeError> {
        if bytes.len() < HEADER_LEN {
            return Err(DecodeError::Short {
                need: HEADER_LEN,
                got: bytes.len(),
            });
        }
        let len = be32(&bytes[0..4]) as usize;
        if len != bytes.len() {
            return Err(DecodeError::Length {
                field: len,
                got: bytes.len(),
            });
        }
        let header = Header {
            header_id: be16(&bytes[4..6]),
            server_id: be16(&bytes[6..8]),
            instance: be32(&bytes[8..12]),
            correlation: be32(&bytes[12..16]),
            request_id: be16(&bytes[18..20]),
        };
        let tlen = be16(&bytes[16..18]) as usize;
        let tend = HEADER_LEN + tlen;
        if tend > bytes.len() {
            return Err(DecodeError::Template {
                len: tlen,
                room: bytes.len() - HEADER_LEN,
            });
        }
        let mut frame = Frame::new(header, bytes[HEADER_LEN..tend].to_vec());
        let mut at = tend;
        while at < bytes.len() {
            let rest = &bytes[at..];
            if rest.len() < PARAM_HEADER_LEN {
                return Err(DecodeError::Param { offset: at });
            }
            let ll = be32(&rest[0..4]) as usize;
            if ll < PARAM_HEADER_LEN || ll > rest.len() {
                return Err(DecodeError::Param { offset: at });
            }
            frame.push(be16(&rest[4..6]), &rest[PARAM_HEADER_LEN..ll]);
            at += ll;
        }
        Ok(frame)
    }

    /// Checks the server and request (or reply) IDs.
    pub fn expect(&self, server_id: u16, request_id: u16) -> Result<(), DecodeError> {
        if self.header.server_id != server_id {
            return Err(DecodeError::Server {
                want: server_id,
                got: self.header.server_id,
            });
        }
        if self.header.request_id != request_id {
            return Err(DecodeError::Request {
                want: request_id,
                got: self.header.request_id,
            });
        }
        Ok(())
    }

    /// The template, which must be at least `n` bytes.
    pub fn template_at_least(&self, n: usize) -> Result<&[u8], DecodeError> {
        if self.template.len() < n {
            return Err(DecodeError::Template {
                len: self.template.len(),
                room: n,
            });
        }
        Ok(&self.template)
    }
}

/// The total length a frame's first four bytes announce, checked against
/// [`MAX_FRAME_LEN`] and the header size: what a reader reads next.
pub fn frame_length(prefix: [u8; 4]) -> Result<usize, DecodeError> {
    let len = u32::from_be_bytes(prefix);
    if (len as usize) < HEADER_LEN || len > MAX_FRAME_LEN {
        return Err(DecodeError::Length {
            field: len as usize,
            got: 4,
        });
    }
    Ok(len as usize)
}

pub(crate) fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

pub(crate) fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Frame {
        let mut f = Frame::new(
            Header {
                header_id: 0x0102,
                server_id: 0xE009,
                instance: 7,
                correlation: 9,
                request_id: 0x7003,
            },
            vec![0xAA, 0xBB],
        );
        f.push(0x1101, 1u32.to_be_bytes()).push(0x1102, [0, 5]);
        f
    }

    #[test]
    fn encodes_the_header_template_and_params() {
        let b = sample().encode();
        assert_eq!(
            b,
            [
                0, 0, 0, 40, 0x01, 0x02, 0xE0, 0x09, 0, 0, 0, 7, 0, 0, 0, 9, 0, 2, 0x70, 0x03,
                0xAA, 0xBB, 0, 0, 0, 10, 0x11, 0x01, 0, 0, 0, 1, 0, 0, 0, 8, 0x11, 0x02, 0, 5
            ]
        );
        assert_eq!(Frame::decode(&b).unwrap(), sample());
        assert_eq!(frame_length(b[..4].try_into().unwrap()).unwrap(), 40);
    }

    #[test]
    fn malformed_frames_are_errors() {
        let b = sample().encode();
        assert!(matches!(
            Frame::decode(&b[..10]),
            Err(DecodeError::Short { .. })
        ));
        assert!(matches!(
            Frame::decode(&b[..39]),
            Err(DecodeError::Length { field: 40, got: 39 })
        ));
        // A parameter whose LL runs past the end.
        let mut bad = b.clone();
        bad[25] = 0xFF;
        assert!(matches!(
            Frame::decode(&bad),
            Err(DecodeError::Param { offset: 22 })
        ));
        // A template longer than the frame.
        let mut bad = b.clone();
        bad[17] = 99;
        assert!(matches!(
            Frame::decode(&bad),
            Err(DecodeError::Template { .. })
        ));
        assert!(frame_length([0, 0, 0, 19]).is_err());
        assert!(frame_length([0x7F, 0, 0, 0]).is_err());
    }

    #[test]
    fn params_never_print_their_data() {
        let mut f = sample();
        f.push(0x1105, *b"SECRETSS");
        let shown = format!("{f:?}");
        assert!(!shown.contains("83, 69"), "{shown}");
        assert!(shown.contains("Param(1105, 8 bytes)"));
    }
}
