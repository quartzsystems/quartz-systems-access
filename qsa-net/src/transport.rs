// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The byte pipe under every protocol: plain TCP here, TLS in
//! [`crate::tls`].

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// How a connection is protected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Security {
    Plain,
    /// TLS, with the negotiated protocol version and cipher suite.
    Tls {
        protocol: String,
        cipher: String,
    },
}

/// A connected byte pipe. Reads time out, or are polled, so one thread can
/// look after both directions.
pub trait Transport: Read + Write + Send {
    /// Sets how long a read may block; `None` blocks.
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;
    /// Makes the socket non-blocking: a read waits up to `wait` for data,
    /// then fails with [`io::ErrorKind::WouldBlock`] ([`is_timeout`]), and a
    /// write waits for room up to the write timeout. No socket read timeout
    /// is used: on Windows a read that hits one can fail with os error 997
    /// (`ERROR_IO_PENDING`) and leave the socket unusable.
    fn poll_reads(&mut self, wait: Duration) -> io::Result<()>;
    /// Closes the connection; reads and writes fail afterwards.
    fn shutdown(&mut self);
    fn security(&self) -> Security;
    /// The address connected to.
    fn peer(&self) -> Option<SocketAddr>;
}

/// Why a connection could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("host {0} not found")]
    NotFound(String),
    #[error("connection to {0} refused")]
    Refused(String),
    #[error("connection to {0} timed out")]
    TimedOut(String),
    #[error("connection to {host} failed: {source}")]
    Io {
        host: String,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Tls(#[from] crate::tls::TlsError),
}

/// A connected socket, its reads blocking with a timeout or polled
/// ([`Transport::poll_reads`]).
#[derive(Debug)]
pub(crate) struct Sock {
    pub stream: TcpStream,
    /// How long a polled read waits; `None` while reads block.
    poll: Option<Duration>,
    /// How long a polled write waits for room.
    write_timeout: Duration,
}

impl Sock {
    pub fn new(stream: TcpStream) -> Sock {
        let write_timeout = stream
            .write_timeout()
            .ok()
            .flatten()
            .unwrap_or(crate::CONNECT_TIMEOUT);
        Sock {
            stream,
            poll: None,
            write_timeout,
        }
    }

    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        if self.poll.take().is_some() {
            self.stream.set_nonblocking(false)?;
        }
        self.stream.set_read_timeout(timeout)
    }

    pub fn poll_reads(&mut self, wait: Duration) -> io::Result<()> {
        self.stream.set_read_timeout(None)?;
        self.stream.set_nonblocking(true)?;
        self.poll = Some(wait);
        Ok(())
    }
}

impl Read for Sock {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match (self.stream.read(buf), self.poll) {
            (Err(e), Some(wait)) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(wait);
                self.stream.read(buf)
            }
            (r, _) => r,
        }
    }
}

impl Write for Sock {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.poll.is_none() {
            return self.stream.write(buf);
        }
        let deadline = Instant::now() + self.write_timeout;
        loop {
            match self.stream.write(buf) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "no room to write in time",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                r => return r,
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

/// Plain TCP.
#[derive(Debug)]
pub struct Tcp {
    sock: Sock,
}

impl Tcp {
    /// Connects to `host:port`, trying each address the name resolves to,
    /// within `timeout` overall.
    pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<Tcp, ConnectError> {
        let start = Instant::now();
        let addrs: Vec<SocketAddr> = (host, port)
            .to_socket_addrs()
            .map_err(|_| ConnectError::NotFound(host.to_owned()))?
            .collect();
        if addrs.is_empty() {
            return Err(ConnectError::NotFound(host.to_owned()));
        }
        let mut last = None;
        for addr in addrs {
            let left = timeout.saturating_sub(start.elapsed());
            if left.is_zero() {
                break;
            }
            crate::debug!("tcp: connecting to {addr}");
            match TcpStream::connect_timeout(&addr, left) {
                Ok(stream) => {
                    stream.set_nodelay(true).ok();
                    stream.set_write_timeout(Some(timeout)).ok();
                    return Ok(Tcp::from_stream(stream));
                }
                Err(e) => {
                    crate::debug!("tcp: {addr}: {e}");
                    last = Some(e);
                }
            }
        }
        Err(match last {
            Some(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                ConnectError::Refused(host.to_owned())
            }
            Some(e) if e.kind() == io::ErrorKind::TimedOut => {
                ConnectError::TimedOut(host.to_owned())
            }
            Some(e) => ConnectError::Io {
                host: host.to_owned(),
                source: e,
            },
            None => ConnectError::TimedOut(host.to_owned()),
        })
    }

    /// The socket, for layering TLS on it.
    pub fn into_stream(self) -> TcpStream {
        self.sock.stream
    }

    /// Wraps a connected socket.
    pub fn from_stream(stream: TcpStream) -> Tcp {
        Tcp {
            sock: Sock::new(stream),
        }
    }
}

impl Read for Tcp {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.sock.read(buf)
    }
}

impl Write for Tcp {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.sock.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.sock.flush()
    }
}

impl Transport for Tcp {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.sock.set_read_timeout(timeout)
    }
    fn poll_reads(&mut self, wait: Duration) -> io::Result<()> {
        self.sock.poll_reads(wait)
    }
    fn shutdown(&mut self) {
        let _ = self.sock.stream.shutdown(Shutdown::Both);
    }
    fn security(&self) -> Security {
        Security::Plain
    }
    fn peer(&self) -> Option<SocketAddr> {
        self.sock.stream.peer_addr().ok()
    }
}

/// `ERROR_IO_PENDING`: on Windows, how a socket read that hits its read
/// timeout can fail instead of `WSAETIMEDOUT`.
const ERROR_IO_PENDING: i32 = 997;

/// Whether a read error only means that nothing arrived in time: a timeout,
/// a polled read with nothing there, an interrupted call, or on Windows os
/// error 997 from a read that hit its timeout.
pub fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    ) || (cfg!(windows) && e.raw_os_error() == Some(ERROR_IO_PENDING))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn connects_and_carries_bytes() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            s.write_all(b"\xFF\xFD\x18").unwrap();
        });
        let mut c = Tcp::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let mut buf = [0u8; 3];
        c.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [0xFF, 0xFD, 0x18]);
        assert_eq!(c.security(), Security::Plain);
        t.join().unwrap();
    }

    #[test]
    fn refused_is_reported_as_such() {
        // Bind then drop: the port is very likely closed.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        match Tcp::connect("127.0.0.1", port, Duration::from_secs(5)) {
            Err(ConnectError::Refused(h)) => assert_eq!(h, "127.0.0.1"),
            other => panic!("expected refused, got {other:?}"),
        }
    }

    #[test]
    fn unknown_host_is_not_found() {
        match Tcp::connect("no-such-host.invalid", 23, Duration::from_secs(5)) {
            Err(ConnectError::NotFound(h)) => assert_eq!(h, "no-such-host.invalid"),
            other => panic!("expected not found, got {other:?}"),
        }
    }

    #[test]
    fn read_timeout_is_recognised() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let mut c = Tcp::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let _held = l.accept().unwrap();
        c.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
        let e = c.read(&mut [0u8; 1]).unwrap_err();
        assert!(is_timeout(&e), "{e:?}");
    }

    #[test]
    fn polled_reads_wait_then_say_nothing_came() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let mut c = Tcp::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        let (mut s, _) = l.accept().unwrap();
        c.poll_reads(Duration::from_millis(5)).unwrap();
        let mut b = [0u8; 4];
        for _ in 0..20 {
            let e = c.read(&mut b).unwrap_err();
            assert!(is_timeout(&e), "{e:?}");
        }
        s.write_all(b"QAP!").unwrap();
        let mut got = Vec::new();
        while got.len() < 4 {
            match c.read(&mut b) {
                Ok(n) => got.extend_from_slice(&b[..n]),
                Err(e) => assert!(is_timeout(&e), "{e:?}"),
            }
        }
        assert_eq!(got, b"QAP!");
        // Writes still go whole.
        c.write_all(&[7u8; 300_000]).unwrap();
        let mut n = 0;
        let mut buf = vec![0u8; 65536];
        while n < 300_000 {
            n += s.read(&mut buf).unwrap();
        }
        // The peer's close is an end of stream, not a timeout.
        drop(s);
        let mut end = false;
        for _ in 0..200 {
            match c.read(&mut b) {
                Ok(0) => {
                    end = true;
                    break;
                }
                Ok(_) => {}
                Err(e) if is_timeout(&e) => {}
                Err(_) => {
                    end = true;
                    break;
                }
            }
        }
        assert!(end);
    }

    #[test]
    fn error_io_pending_is_a_timeout_on_windows() {
        let e = io::Error::from_raw_os_error(ERROR_IO_PENDING);
        assert_eq!(is_timeout(&e), cfg!(windows));
    }
}
