// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The network layer every QS/400 Access for Desktop protocol shares: 5250
//! Telnet and the host servers.
//!
//! - [`transport`]: plain TCP, with connect and write timeouts.
//! - [`tls`]: TLS over it (rustls), and the one certificate-trust path:
//!   the system store, the system configuration's CA list, and
//!   certificates trusted as they stand. A certificate that does not
//!   verify comes back in [`TlsError::Untrusted`] for the
//!   untrusted-certificate dialog.
//! - [`x509`]: enough of a certificate to show it to the user.
//! - [`log`]: where the protocol crates' log lines go.
//! - [`connect`]: TCP, then TLS if asked for, in one call.
//!
//! The crate has no user-interface dependency.

pub mod log;
pub mod tls;
pub mod transport;
pub mod x509;

use std::time::Duration;

pub use tls::{Tls, TlsError, TlsOptions, Trust};
pub use transport::{ConnectError, Security, Tcp, Transport, is_timeout};

/// How long a connection may take to open, TLS handshake included.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a long-lived connection may be quiet before the protocol sends
/// something harmless (Telnet `NOP`). It keeps NAT and firewall state
/// alive and lets a dead peer show as a failed write.
pub const KEEPALIVE: Duration = Duration::from_secs(60);

/// Connects to `host:port` within `timeout`, then runs the TLS handshake
/// when `tls` is given. The TLS server name is `host`.
pub fn connect(
    host: &str,
    port: u16,
    tls: Option<&TlsOptions>,
    timeout: Duration,
) -> Result<Box<dyn Transport>, ConnectError> {
    let tcp = Tcp::connect(host, port, timeout)?;
    Ok(match tls {
        Some(opts) => Box::new(Tls::handshake(tcp, host, opts)?),
        None => Box::new(tcp),
    })
}
