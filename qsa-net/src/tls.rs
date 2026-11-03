// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! TLS over TCP with rustls (ring provider).
//!
//! A server certificate is trusted when it chains to a root in the
//! system's certificate store or in the system configuration's CA list.
//! A certificate that is itself in that list is trusted as it stands
//! (this is how "Trust for this system" works for self-signed server
//! certificates). Nothing else is ever accepted: there is no switch to
//! ignore verification.
//!
//! When verification fails the server's certificate is kept and returned
//! in [`TlsError::Untrusted`] so the user can look at it.

use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore, SignatureScheme,
};

use crate::transport::{Security, Sock, Tcp, Transport};
use crate::x509::{self, CertInfo};

/// Why a TLS session could not be set up.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    /// The certificate did not verify. `reason` is rustls's.
    #[error("certificate is not trusted: {reason}")]
    Untrusted {
        reason: String,
        /// The server's certificate, DER.
        certificate: Vec<u8>,
        info: Box<CertInfo>,
    },
    /// Anything else: protocol, alerts, I/O.
    #[error("{0}")]
    Handshake(String),
}

/// Certificates the system configuration adds: roots, and server
/// certificates trusted as they stand.
#[derive(Clone, Debug, Default)]
pub struct Trust {
    pub certificates: Vec<CertificateDer<'static>>,
}

impl Trust {
    /// From PEM texts; blocks that are not certificates are skipped.
    pub fn from_pem<'a>(pems: impl IntoIterator<Item = &'a str>) -> Trust {
        Trust {
            certificates: pems.into_iter().flat_map(certs_from_pem).collect(),
        }
    }
}

/// Every certificate in a PEM text.
pub fn certs_from_pem(pem: &str) -> Vec<CertificateDer<'static>> {
    CertificateDer::pem_slice_iter(pem.as_bytes())
        .filter_map(Result::ok)
        .collect()
}

/// A certificate as PEM text.
pub fn to_pem(der: &[u8]) -> String {
    let b64 = base64(der);
    let mut s = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in b64.as_bytes().chunks(64) {
        s.push_str(std::str::from_utf8(line).unwrap_or_default());
        s.push('\n');
    }
    s.push_str("-----END CERTIFICATE-----\n");
    s
}

fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.len();
        let v = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= n {
                s.push(A[(v >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

fn provider() -> Arc<CryptoProvider> {
    static P: OnceLock<Arc<CryptoProvider>> = OnceLock::new();
    P.get_or_init(|| Arc::new(rustls::crypto::ring::default_provider()))
        .clone()
}

/// The platform's trusted roots, read once.
fn native_roots() -> &'static [CertificateDer<'static>] {
    static R: OnceLock<Vec<CertificateDer<'static>>> = OnceLock::new();
    R.get_or_init(|| {
        let r = rustls_native_certs::load_native_certs();
        for e in &r.errors {
            crate::debug!("tls: system certificate store: {e}");
        }
        crate::debug!("tls: {} certificates from the system store", r.certs.len());
        r.certs
    })
}

#[derive(Debug)]
struct Verifier {
    webpki: Option<Arc<WebPkiServerVerifier>>,
    pinned: Vec<CertificateDer<'static>>,
    algorithms: WebPkiSupportedAlgorithms,
    /// The server's certificate, kept when verification failed.
    rejected: Mutex<Option<CertificateDer<'static>>>,
}

impl Verifier {
    fn new(trust: &Trust, include_native: bool) -> Verifier {
        let mut roots = RootCertStore::empty();
        if include_native {
            let (added, ignored) = roots.add_parsable_certificates(native_roots().iter().cloned());
            crate::debug!("tls: {added} system roots, {ignored} unusable");
        }
        let (added, ignored) = roots.add_parsable_certificates(trust.certificates.iter().cloned());
        crate::debug!(
            "tls: {added} roots from the system configuration, {ignored} not usable as roots"
        );
        let p = provider();
        let webpki = if roots.is_empty() {
            None
        } else {
            WebPkiServerVerifier::builder_with_provider(Arc::new(roots), p.clone())
                .build()
                .ok()
        };
        Verifier {
            webpki,
            pinned: trust.certificates.clone(),
            algorithms: p.signature_verification_algorithms,
            rejected: Mutex::new(None),
        }
    }
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let result = match &self.webpki {
            Some(v) => v.verify_server_cert(end_entity, intermediates, server_name, ocsp, now),
            None => Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::UnknownIssuer,
            )),
        };
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                if self
                    .pinned
                    .iter()
                    .any(|p| p.as_ref() == end_entity.as_ref())
                {
                    crate::debug!("tls: {e}; certificate is in the system's trusted list");
                    return Ok(ServerCertVerified::assertion());
                }
                if let Ok(mut r) = self.rejected.lock() {
                    *r = Some(end_entity.clone().into_owned());
                }
                Err(e)
            }
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// TLS over a TCP connection.
pub struct Tls {
    conn: ClientConnection,
    sock: Sock,
    peer: Option<SocketAddr>,
}

impl std::fmt::Debug for Tls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tls").field("peer", &self.peer).finish()
    }
}

/// Options for [`Tls::handshake`].
#[derive(Clone, Debug)]
pub struct TlsOptions {
    pub trust: Trust,
    /// Use the platform's store as well. Off only in tests.
    pub system_roots: bool,
    pub timeout: Duration,
    /// Offer TLS 1.3 alone (QAP); otherwise TLS 1.2 and 1.3 (Telnet).
    pub tls13_only: bool,
}

impl Tls {
    /// Runs the handshake over `tcp`, for the server named `host`.
    pub fn handshake(tcp: Tcp, host: &str, opts: &TlsOptions) -> Result<Tls, TlsError> {
        let verifier = Arc::new(Verifier::new(&opts.trust, opts.system_roots));
        let builder = ClientConfig::builder_with_provider(provider());
        let builder = if opts.tls13_only {
            builder.with_protocol_versions(&[&rustls::version::TLS13])
        } else {
            builder.with_safe_default_protocol_versions()
        };
        let config = builder
            .map_err(|e| TlsError::Handshake(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(verifier.clone())
            .with_no_client_auth();
        let name = ServerName::try_from(host.to_owned())
            .map_err(|e| TlsError::Handshake(format!("{host}: {e}")))?;
        let mut conn = ClientConnection::new(Arc::new(config), name)
            .map_err(|e| TlsError::Handshake(e.to_string()))?;
        let mut sock = tcp.into_stream();
        let peer = sock.peer_addr().ok();
        sock.set_read_timeout(Some(opts.timeout)).ok();
        crate::debug!("tls: handshake with {host}");
        while conn.is_handshaking() {
            if let Err(e) = conn.complete_io(&mut sock) {
                let rejected = verifier.rejected.lock().ok().and_then(|mut r| r.take());
                if let Some(cert) = rejected {
                    let info = x509::describe(&cert);
                    crate::debug!(
                        "tls: certificate rejected: {e}; subject {}, issuer {}, SHA-256 {}",
                        info.subject,
                        info.issuer,
                        info.sha256
                    );
                    return Err(TlsError::Untrusted {
                        reason: rustls_reason(&e),
                        certificate: cert.to_vec(),
                        info: Box::new(info),
                    });
                }
                crate::debug!("tls: handshake failed: {e}");
                return Err(TlsError::Handshake(rustls_reason(&e)));
            }
        }
        let t = Tls {
            conn,
            sock: Sock::new(sock),
            peer,
        };
        if let Security::Tls { protocol, cipher } = t.security() {
            crate::debug!("tls: established, {protocol}, {cipher}");
        }
        Ok(t)
    }
}

/// The message of an I/O error that wraps a rustls error.
fn rustls_reason(e: &io::Error) -> String {
    match e.get_ref().and_then(|i| i.downcast_ref::<rustls::Error>()) {
        Some(r) => r.to_string(),
        None => e.to_string(),
    }
}

impl Read for Tls {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        rustls::Stream::new(&mut self.conn, &mut self.sock).read(buf)
    }
}

impl Write for Tls {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        rustls::Stream::new(&mut self.conn, &mut self.sock).write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        rustls::Stream::new(&mut self.conn, &mut self.sock).flush()
    }
}

impl Transport for Tls {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.sock.set_read_timeout(timeout)
    }
    fn poll_reads(&mut self, wait: Duration) -> io::Result<()> {
        self.sock.poll_reads(wait)
    }
    fn shutdown(&mut self) {
        self.conn.send_close_notify();
        let _ = self.conn.complete_io(&mut self.sock);
        let _ = self.sock.stream.shutdown(std::net::Shutdown::Both);
    }
    fn security(&self) -> Security {
        Security::Tls {
            protocol: self
                .conn
                .protocol_version()
                .map(|v| format!("{v:?}"))
                .unwrap_or_default(),
            cipher: self
                .conn
                .negotiated_cipher_suite()
                .map(|c| format!("{:?}", c.suite()))
                .unwrap_or_default(),
        }
    }
    fn peer(&self) -> Option<SocketAddr> {
        self.peer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::PrivateKeyDer;
    use std::net::TcpListener;

    const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/");

    fn file(name: &str) -> String {
        std::fs::read_to_string(format!("{DATA}{name}")).unwrap()
    }

    /// A TLS server on a free port that sends `hello` and closes.
    fn server(cert: &str, key: &str) -> u16 {
        server_with(cert, key, rustls::DEFAULT_VERSIONS)
    }

    /// The same, offering only `versions`.
    fn server_with(
        cert: &str,
        key: &str,
        versions: &[&'static rustls::SupportedProtocolVersion],
    ) -> u16 {
        let certs = certs_from_pem(&file(cert));
        let key = PrivateKeyDer::from_pem_slice(file(key).as_bytes()).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(versions)
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut c = rustls::ServerConnection::new(Arc::new(config)).unwrap();
            let mut st = rustls::Stream::new(&mut c, &mut s);
            if st.write_all(b"hello").is_ok() {
                let _ = st.flush();
            }
            st.conn.send_close_notify();
            let _ = st.conn.complete_io(st.sock);
        });
        port
    }

    fn connect(port: u16, host: &str, trust: Trust) -> Result<Tls, TlsError> {
        connect_with(port, host, trust, false)
    }

    fn connect_with(
        port: u16,
        host: &str,
        trust: Trust,
        tls13_only: bool,
    ) -> Result<Tls, TlsError> {
        let tcp = Tcp::connect("127.0.0.1", port, Duration::from_secs(5)).unwrap();
        Tls::handshake(
            tcp,
            host,
            &TlsOptions {
                trust,
                system_roots: false,
                timeout: Duration::from_secs(5),
                tls13_only,
            },
        )
    }

    #[test]
    fn a_configured_ca_is_trusted() {
        let port = server("server.pem", "server.key");
        let trust = Trust::from_pem([file("test-ca.pem").as_str()]);
        let mut t = connect(port, "localhost", trust).unwrap();
        let mut buf = [0u8; 5];
        t.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"hello");
        assert!(matches!(t.security(), Security::Tls { .. }));
    }

    #[test]
    fn an_unknown_ca_is_rejected_with_the_certificate() {
        let port = server("server.pem", "server.key");
        match connect(port, "localhost", Trust::default()) {
            Err(TlsError::Untrusted {
                certificate, info, ..
            }) => {
                assert_eq!(info.subject, "CN=localhost, O=Quartz Systems Test");
                assert_eq!(info.issuer, "CN=Quartz Test CA, O=Quartz Systems Test");
                let want = certs_from_pem(&file("server.pem"));
                assert_eq!(certificate, want[0].to_vec());
            }
            other => panic!("expected untrusted, got {other:?}"),
        }
    }

    #[test]
    fn a_self_signed_certificate_is_trusted_once_added() {
        let port = server("selfsigned.pem", "selfsigned.key");
        let cert = match connect(port, "localhost", Trust::default()) {
            Err(TlsError::Untrusted { certificate, .. }) => certificate,
            other => panic!("expected untrusted, got {other:?}"),
        };
        // "Trust for this system" stores the certificate as PEM.
        let pem = to_pem(&cert);
        let port = server("selfsigned.pem", "selfsigned.key");
        let mut t = connect(port, "localhost", Trust::from_pem([pem.as_str()])).unwrap();
        let mut buf = [0u8; 5];
        t.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"hello");
    }

    #[test]
    fn a_trusted_ca_does_not_cover_the_wrong_name() {
        let port = server("server.pem", "server.key");
        let trust = Trust::from_pem([file("test-ca.pem").as_str()]);
        assert!(matches!(
            connect(port, "other.example", trust),
            Err(TlsError::Untrusted { .. })
        ));
    }

    #[test]
    fn tls13_only_refuses_a_tls12_server() {
        let ca = file("test-ca.pem");
        let port = server_with("server.pem", "server.key", &[&rustls::version::TLS12]);
        assert!(matches!(
            connect_with(port, "localhost", Trust::from_pem([ca.as_str()]), true),
            Err(TlsError::Handshake(_))
        ));
        let port = server_with("server.pem", "server.key", &[&rustls::version::TLS12]);
        assert!(connect_with(port, "localhost", Trust::from_pem([ca.as_str()]), false).is_ok());
        let port = server("server.pem", "server.key");
        let t = connect_with(port, "localhost", Trust::from_pem([ca.as_str()]), true).unwrap();
        assert!(matches!(t.security(), Security::Tls { protocol, .. } if protocol == "TLSv1_3"));
    }

    #[test]
    fn pem_round_trips() {
        let certs = certs_from_pem(&file("test-ca.pem"));
        assert_eq!(certs.len(), 1);
        assert_eq!(certs_from_pem(&to_pem(&certs[0])), certs);
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b"Man"), "TWFu");
    }
}
