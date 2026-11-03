// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! Line endings for text-mode file transfers: LF on the system, CR LF on
//! this computer when asked (by default on Windows). Only the line endings
//! change; the characters are converted by the system.
//!
//! Lines are converted whole, so a CR at the end of one chunk is decided
//! with the next: [`CrlfToLf`] holds it back until it sees whether an LF
//! follows, and [`LfToCrlf`] remembers it so that an LF opening the next
//! chunk is not given a second CR.

use std::io::{self, Read, Write};

/// A writer that turns each LF not already after a CR into CR LF: for a
/// download in text mode.
pub struct LfToCrlf<W> {
    inner: W,
    /// The last byte written was a CR.
    after_cr: bool,
    buf: Vec<u8>,
}

impl<W: Write> LfToCrlf<W> {
    /// Over `inner`, its last byte so far a CR when `after_cr` (a resumed
    /// download's partial file ending in one).
    pub fn new(inner: W, after_cr: bool) -> LfToCrlf<W> {
        LfToCrlf {
            inner,
            after_cr,
            buf: Vec::new(),
        }
    }

    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for LfToCrlf<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.clear();
        self.buf.reserve(data.len() + data.len() / 16);
        for &b in data {
            if b == b'\n' && !self.after_cr {
                self.buf.push(b'\r');
            }
            self.buf.push(b);
            self.after_cr = b == b'\r';
        }
        // All of it, so the caller's count is of its own bytes.
        self.inner.write_all(&self.buf)?;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// A reader that turns each CR LF into LF, a CR alone left as it is: for an
/// upload in text mode. A CR that ends what the source gave waits for the
/// next read.
pub struct CrlfToLf<R> {
    inner: R,
    /// A CR not yet given out: whether an LF follows is not known yet.
    held_cr: bool,
    done: bool,
    /// Converted bytes not yet given out, from `at`.
    out: Vec<u8>,
    at: usize,
    raw: Vec<u8>,
}

impl<R: Read> CrlfToLf<R> {
    pub fn new(inner: R) -> CrlfToLf<R> {
        CrlfToLf {
            inner,
            held_cr: false,
            done: false,
            out: Vec::new(),
            at: 0,
            raw: vec![0; 64 * 1024],
        }
    }

    /// Reads and converts the next part of the source into `out`; false
    /// at its end with nothing more to give.
    fn fill(&mut self) -> io::Result<bool> {
        self.out.clear();
        self.at = 0;
        while self.out.is_empty() {
            if self.done {
                if self.held_cr {
                    self.held_cr = false;
                    self.out.push(b'\r');
                    return Ok(true);
                }
                return Ok(false);
            }
            let n = loop {
                match self.inner.read(&mut self.raw) {
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    r => break r?,
                }
            };
            if n == 0 {
                self.done = true;
                continue;
            }
            for &b in &self.raw[..n] {
                if self.held_cr && b != b'\n' {
                    self.out.push(b'\r');
                }
                self.held_cr = b == b'\r';
                if !self.held_cr {
                    self.out.push(b);
                }
            }
        }
        Ok(true)
    }
}

impl<R: Read> Read for CrlfToLf<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.at >= self.out.len() && !self.fill()? {
            return Ok(0);
        }
        let n = (self.out.len() - self.at).min(buf.len());
        buf[..n].copy_from_slice(&self.out[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that gives its parts one per read.
    struct Parts(Vec<&'static [u8]>);

    impl Read for Parts {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            let p = self.0.remove(0);
            let n = p.len().min(out.len());
            out[..n].copy_from_slice(&p[..n]);
            if n < p.len() {
                self.0.insert(0, &p[n..]);
            }
            Ok(n)
        }
    }

    fn down(parts: &[&[u8]], after_cr: bool) -> Vec<u8> {
        let mut w = LfToCrlf::new(Vec::new(), after_cr);
        for p in parts {
            w.write_all(p).unwrap();
        }
        w.into_inner()
    }

    fn up(parts: Vec<&'static [u8]>, step: usize) -> Vec<u8> {
        let mut r = CrlfToLf::new(Parts(parts));
        let mut out = Vec::new();
        let mut buf = vec![0u8; step];
        loop {
            let n = r.read(&mut buf).unwrap();
            if n == 0 {
                return out;
            }
            out.extend_from_slice(&buf[..n]);
        }
    }

    #[test]
    fn lf_becomes_crlf_once() {
        assert_eq!(down(&[b"a\nb\n"], false), b"a\r\nb\r\n");
        // Already CR LF: left as it is, even split between chunks.
        assert_eq!(down(&[b"a\r\nb"], false), b"a\r\nb");
        assert_eq!(down(&[b"a\r", b"\nb\n"], false), b"a\r\nb\r\n");
        // A resumed file that ended in CR.
        assert_eq!(down(&[b"\nc"], true), b"\nc");
        assert_eq!(down(&[b"\n"], false), b"\r\n");
        assert_eq!(down(&[b"x\r", b"y"], false), b"x\ry");
    }

    #[test]
    fn crlf_becomes_lf_even_split_between_chunks() {
        assert_eq!(up(vec![b"a\r\nb\r\n"], 64), b"a\nb\n");
        // The pair split across the source's reads.
        assert_eq!(up(vec![b"a\r", b"\nb"], 64), b"a\nb");
        // And across this reader's own: every buffer size.
        let text: &'static [u8] = b"one\r\ntwo\r\n\r\nthree\rfour\r";
        for step in 1..12 {
            assert_eq!(up(vec![text], step), b"one\ntwo\n\nthree\rfour\r", "{step}");
        }
        // A CR alone stays; one at the very end too.
        assert_eq!(up(vec![b"a\r", b"b"], 64), b"a\rb");
        assert_eq!(up(vec![b"a\r"], 64), b"a\r");
        assert_eq!(up(vec![b"\r", b"\r", b"\n"], 64), b"\r\n");
    }
}
