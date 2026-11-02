// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Frames on the wire: a `u32` little-endian length, then one encoded
//! envelope of at most [`MAX_FRAME`] bytes. A longer length is refused from
//! its prefix alone, before its bytes are read. Bytes are wiped once
//! decoded.

use qap_proto::{Envelope, MAX_FRAME, ProtoError};

/// Bytes read and not yet a whole frame.
#[derive(Debug, Default)]
pub struct FrameReader {
    buf: Vec<u8>,
}

impl FrameReader {
    pub fn push(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    /// The next whole envelope, `None` until one is in. An error ends the
    /// connection: a frame cannot be skipped.
    pub fn next_frame(&mut self) -> Result<Option<Envelope>, ProtoError> {
        let Some(head) = self.buf.get(..4) else {
            return Ok(None);
        };
        let n = u32::from_le_bytes(head.try_into().expect("four bytes")) as usize;
        if n > MAX_FRAME {
            return Err(ProtoError::TooLong { len: n });
        }
        if self.buf.len() < 4 + n {
            return Ok(None);
        }
        let env = Envelope::from_frame(&self.buf[4..4 + n]);
        // The frame may hold a token: wipe it before letting it go.
        self.buf[..4 + n].fill(0);
        self.buf.drain(..4 + n);
        env.map(Some)
    }

    /// Wipes and drops what is left.
    pub fn clear(&mut self) {
        self.buf.fill(0);
        std::hint::black_box(&self.buf);
        self.buf.clear();
    }
}

/// `env` as a length-prefixed frame; [`ProtoError::TooLong`] over 1 MiB.
pub fn frame(env: &Envelope) -> Result<Vec<u8>, ProtoError> {
    let mut b = env.to_frame()?;
    let mut out = Vec::with_capacity(4 + b.len());
    out.extend_from_slice(&u32::try_from(b.len()).expect("bounded").to_le_bytes());
    out.extend_from_slice(&b);
    b.fill(0);
    std::hint::black_box(&b);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use qap_proto::Op;

    #[test]
    fn frames_in_pieces_and_the_limit() {
        let a = Envelope::request(1, Op::Ping, &());
        let b = Envelope::cancel(1);
        let mut bytes = frame(&a).unwrap();
        bytes.extend(frame(&b).unwrap());
        let mut r = FrameReader::default();
        for chunk in bytes.chunks(3) {
            r.push(chunk);
        }
        assert_eq!(r.next_frame().unwrap(), Some(a));
        assert_eq!(r.next_frame().unwrap(), Some(b));
        assert_eq!(r.next_frame().unwrap(), None);

        let mut r = FrameReader::default();
        r.push(&u32::try_from(MAX_FRAME + 1).unwrap().to_le_bytes());
        assert!(matches!(r.next_frame(), Err(ProtoError::TooLong { .. })));

        let big = Envelope {
            body: vec![0; MAX_FRAME],
            ..Envelope::request(2, Op::Ping, &())
        };
        assert!(matches!(frame(&big), Err(ProtoError::TooLong { .. })));
    }
}
