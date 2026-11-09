//! TLS connection to Cast devices.
//!
//! Cast devices use self-signed TLS certificates on port 8009.
//! Certificate verification is **disabled by default** because Cast
//! devices do not use CA-signed certificates. This means the connection
//! is encrypted but not authenticated — a LAN attacker could MITM.
//!
//! This is the same trade-off made by pychromecast, go-chromecast,
//! rust_cast, and node-castv2. The Cast protocol has a separate device
//! authentication channel (`urn:x-cast:com.google.cast.tp.deviceauth`)
//! which is not yet implemented in oxicast.
//!
//! Set `verify_tls(true)` on the builder if your device has a CA-signed
//! certificate (uncommon).

use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::error::{Error, Result};

/// Establish a TLS connection to a Cast device.
///
/// Returns the connected TLS stream which can be split into read/write halves.
///
/// `timeout` bounds the whole attempt (TCP connect and TLS handshake). Without
/// it, an unreachable device or one that accepts the TCP connection but never
/// completes the handshake would block the caller indefinitely.
pub async fn connect(
    host: &str,
    port: u16,
    verify_tls: bool,
    timeout: Duration,
) -> Result<TlsStream<TcpStream>> {
    match tokio::time::timeout(timeout, connect_inner(host, port, verify_tls)).await {
        Ok(result) => result,
        Err(_) => {
            tracing::debug!("timed out connecting to cast device at {host}:{port}");
            Err(Error::Connect(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("timed out after {timeout:?}"),
            )))
        }
    }
}

async fn connect_inner(host: &str, port: u16, verify_tls: bool) -> Result<TlsStream<TcpStream>> {
    let target = Target::parse(host)?;
    tracing::debug!("connecting to cast device at {host}:{port}");

    let tcp = match target.socket_addr(port) {
        Some(addr) => TcpStream::connect(addr).await,
        None => TcpStream::connect((host, port)).await,
    }
    .map_err(Error::Connect)?;

    // Always use ring explicitly instead of the process-wide default provider:
    // if the application's dependency graph also enables aws-lc-rs, rustls
    // cannot pick a default and `ClientConfig::builder()` would panic.
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Tls(format!("{e}")))?;
    let config = if verify_tls {
        let mut root_store = rustls::RootCertStore::empty();
        let certs_result = rustls_native_certs::load_native_certs();
        for cert in certs_result.certs {
            let _ = root_store.add(cert);
        }
        builder.with_root_certificates(root_store).with_no_client_auth()
    } else {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoCertVerification))
            .with_no_client_auth()
    };

    let connector = TlsConnector::from(Arc::new(config));
    let tls_stream = connector
        .connect(target.server_name()?, tcp)
        .await
        .map_err(|e| Error::Tls(format!("{e}")))?;

    tracing::debug!("TLS connection established to {host}:{port}");
    Ok(tls_stream)
}

/// A parsed connection target: an IP literal (IPv4, IPv6 with optional
/// brackets and numeric `%scope`) or a hostname resolved by the OS.
#[derive(Debug, PartialEq)]
enum Target<'a> {
    V4(std::net::Ipv4Addr),
    V6 { ip: std::net::Ipv6Addr, scope_id: u32 },
    Host(&'a str),
}

impl<'a> Target<'a> {
    fn parse(host: &'a str) -> Result<Self> {
        let unbracketed = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
        let (addr, scope) = match unbracketed.split_once('%') {
            Some((addr, scope)) => (addr, Some(scope)),
            None => (unbracketed, None),
        };
        match addr.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V4(ip)) if scope.is_none() => Ok(Self::V4(ip)),
            Ok(std::net::IpAddr::V6(ip)) => {
                let scope_id = match scope {
                    None => 0,
                    Some(s) => s.parse().map_err(|_| {
                        invalid_input(format!(
                            "unsupported IPv6 scope `{s}` in `{host}` (use the numeric interface index)"
                        ))
                    })?,
                };
                Ok(Self::V6 { ip, scope_id })
            }
            Ok(std::net::IpAddr::V4(_)) => {
                Err(invalid_input(format!("IPv4 address `{host}` cannot have a scope")))
            }
            Err(_) => Ok(Self::Host(host)),
        }
    }

    fn socket_addr(&self, port: u16) -> Option<std::net::SocketAddr> {
        match *self {
            Self::V4(ip) => Some(std::net::SocketAddr::V4(std::net::SocketAddrV4::new(ip, port))),
            Self::V6 { ip, scope_id } => {
                Some(std::net::SocketAddr::V6(std::net::SocketAddrV6::new(ip, port, 0, scope_id)))
            }
            Self::Host(_) => None,
        }
    }

    /// TLS server name: the bare IP (no brackets or scope) or the hostname.
    fn server_name(&self) -> Result<ServerName<'static>> {
        Ok(match *self {
            Self::V4(ip) => ServerName::IpAddress(rustls::pki_types::IpAddr::V4(ip.into())),
            Self::V6 { ip, .. } => ServerName::IpAddress(rustls::pki_types::IpAddr::V6(ip.into())),
            Self::Host(host) => ServerName::try_from(host.to_string())
                .map_err(|e| Error::Tls(format!("invalid host `{host}`: {e}")))?,
        })
    }
}

fn invalid_input(msg: String) -> Error {
    Error::Connect(std::io::Error::new(std::io::ErrorKind::InvalidInput, msg))
}

/// TLS certificate verifier that accepts all certificates (insecure).
///
/// Required for Cast devices which use self-signed certificates.
/// The connection is still encrypted — only authentication is skipped.
#[derive(Debug)]
struct NoCertVerification;

impl rustls::client::danger::ServerCertVerifier for NoCertVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_connect_times_out_when_tls_handshake_stalls() {
        // Accepts the TCP connection but never responds to the TLS handshake.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });

        let _ = rustls::crypto::ring::default_provider().install_default();
        let start = std::time::Instant::now();
        let result = connect("127.0.0.1", port, false, Duration::from_millis(200)).await;

        match result {
            Err(Error::Connect(e)) => assert_eq!(e.kind(), std::io::ErrorKind::TimedOut),
            other => panic!("expected connect timeout, got {:?}", other.map(|_| ())),
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        server.abort();
    }
}

#[cfg(test)]
mod target_tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

    #[test]
    fn parses_ipv4() {
        let t = Target::parse("192.168.1.5").unwrap();
        assert_eq!(t, Target::V4(Ipv4Addr::new(192, 168, 1, 5)));
        assert_eq!(t.socket_addr(8009).unwrap(), "192.168.1.5:8009".parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn parses_ipv6_with_and_without_brackets() {
        for host in ["fe80::1", "[fe80::1]"] {
            let t = Target::parse(host).unwrap();
            assert_eq!(t, Target::V6 { ip: "fe80::1".parse::<Ipv6Addr>().unwrap(), scope_id: 0 });
            assert_eq!(
                t.socket_addr(8009).unwrap(),
                "[fe80::1]:8009".parse::<SocketAddr>().unwrap()
            );
        }
    }

    #[test]
    fn parses_ipv6_numeric_scope_and_strips_it_from_server_name() {
        let t = Target::parse("[fe80::1%3]").unwrap();
        let SocketAddr::V6(addr) = t.socket_addr(8009).unwrap() else { panic!("not v6") };
        assert_eq!(addr.scope_id(), 3);
        assert_eq!(addr.port(), 8009);
        let ServerName::IpAddress(rustls::pki_types::IpAddr::V6(_)) = t.server_name().unwrap()
        else {
            panic!("expected IPv6 server name")
        };
    }

    #[test]
    fn rejects_named_scope_and_scoped_ipv4() {
        assert!(Target::parse("fe80::1%en0").is_err());
        assert!(Target::parse("10.0.0.1%2").is_err());
    }

    #[test]
    fn hostnames_are_resolved_by_the_os() {
        let t = Target::parse("living-room.local").unwrap();
        assert_eq!(t, Target::Host("living-room.local"));
        assert!(t.socket_addr(8009).is_none());
        assert!(t.server_name().is_ok());
    }
}
