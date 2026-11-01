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

/// A connected byte pipe. Reads time out so one thread can poll for both
/// directions.
pub trait Transport: Read + Write + Send {
    /// Sets how long a read may block; `None` blocks.
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;
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

/// Plain TCP.
#[derive(Debug)]
pub struct Tcp {
    stream: TcpStream,
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
                    return Ok(Tcp { stream });
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
        self.stream
    }

    /// Wraps a connected socket.
    pub fn from_stream(stream: TcpStream) -> Tcp {
        Tcp { stream }
    }
}

impl Read for Tcp {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buf)
    }
}

impl Write for Tcp {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl Transport for Tcp {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }
    fn shutdown(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
    fn security(&self) -> Security {
        Security::Plain
    }
    fn peer(&self) -> Option<SocketAddr> {
        self.stream.peer_addr().ok()
    }
}

/// Whether a read error only means that nothing arrived in time.
pub fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
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
}
