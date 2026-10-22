// SPDX-License-Identifier: GPL-3.0-or-later

//! Pinned mutual TLS 1.3 for the control link (PROTOCOL.md 3.1, 4).

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use a7n_proto::{ALPN_LINK, Pin};
use quinn::crypto::rustls::{NoInitialCipherSuite, QuicClientConfig, QuicServerConfig};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms, ring};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{CertificateError, DigitallySignedStruct, DistinguishedName, SignatureScheme};

use crate::identity::{self, Identity};

/// Server name sent in the TLS handshake. The verifier ignores it: peers are
/// identified by pin, not by name.
pub(crate) const TLS_SERVER_NAME: &str = "apeirogon";
/// Receive and send buffer for Motion datagrams (PROTOCOL.md 1); small, as a
/// datagram carries one `Motion` and stale ones are worthless.
const DATAGRAM_BUFFER_BYTES: usize = 64 * 1024;

/// Pins of all paired peers. Locked only for short synchronous lookups, never
/// across an await.
pub(crate) type TrustedPins = Arc<RwLock<HashSet<Pin>>>;

/// Why building a TLS configuration failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum TlsError {
    #[error("TLS configuration error: {0}")]
    Rustls(#[from] rustls::Error),
    #[error("QUIC cannot use the TLS configuration: {0}")]
    Quic(#[from] NoInitialCipherSuite),
}

/// Accepts a certificate only if its pin is trusted; ignores name, dates and
/// chain (PROTOCOL.md 3.1). The handshake signature is still verified.
#[derive(Debug)]
pub(crate) struct PinVerifier {
    pins: TrustedPins,
    algorithms: WebPkiSupportedAlgorithms,
}

impl PinVerifier {
    pub(crate) fn new(pins: TrustedPins) -> Self {
        Self {
            pins,
            algorithms: ring::default_provider().signature_verification_algorithms,
        }
    }

    fn check_pin(&self, end_entity: &CertificateDer<'_>) -> Result<(), rustls::Error> {
        let trusted = identity::pin_of(end_entity)
            .is_ok_and(|pin| self.pins.read().is_ok_and(|pins| pins.contains(&pin)));
        if trusted {
            return Ok(());
        }
        // rustls turns `NotValidForName` into the `bad_certificate` alert
        // (rustls 0.23.46 src/error.rs:625-632, `From<CertificateError> for
        // AlertDescription`; sent by `send_cert_verify_error_alert`). The
        // obvious `UnknownIssuer` would send `unknown_ca` instead, and
        // PROTOCOL.md 4 asks for `bad_certificate`.
        Err(rustls::Error::InvalidCertificate(
            CertificateError::NotValidForName,
        ))
    }

    fn verify_tls13(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }
}

/// TLS 1.2 is never negotiated; the configs allow TLS 1.3 only.
fn tls12_unsupported() -> rustls::Error {
    rustls::Error::General("TLS 1.2 is not supported".into())
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        self.check_pin(end_entity)?;
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(tls12_unsupported())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.verify_tls13(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

impl ClientCertVerifier for PinVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        self.check_pin(end_entity)?;
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(tls12_unsupported())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.verify_tls13(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(ring::default_provider())
}

fn certificate_chain(identity: &Identity) -> Vec<CertificateDer<'static>> {
    vec![CertificateDer::from(identity.cert_der.clone())]
}

fn private_key(identity: &Identity) -> PrivateKeyDer<'static> {
    PrivatePkcs8KeyDer::from(identity.key.serialize_der()).into()
}

/// Datagrams are on by default in quinn; the buffers are sized explicitly.
fn transport_config() -> Arc<quinn::TransportConfig> {
    let mut transport = quinn::TransportConfig::default();
    transport
        .datagram_receive_buffer_size(Some(DATAGRAM_BUFFER_BYTES))
        .datagram_send_buffer_size(DATAGRAM_BUFFER_BYTES);
    Arc::new(transport)
}

/// Server side: TLS 1.3, mandatory pinned client certificate, ALPN `a7n/1`,
/// no early data.
pub(crate) fn server_config(
    identity: &Identity,
    pins: TrustedPins,
) -> Result<quinn::ServerConfig, TlsError> {
    let mut tls = rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(Arc::new(PinVerifier::new(pins)))
        .with_single_cert(certificate_chain(identity), private_key(identity))?;
    tls.alpn_protocols = vec![ALPN_LINK.to_vec()];
    tls.max_early_data_size = 0;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(tls)?));
    config.transport_config(transport_config());
    Ok(config)
}

/// Client side: TLS 1.3, pinned server certificate, our certificate for
/// mutual auth, ALPN `a7n/1`, no early data.
pub(crate) fn client_config(
    identity: &Identity,
    pins: TrustedPins,
) -> Result<quinn::ClientConfig, TlsError> {
    let mut tls = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinVerifier::new(pins)))
        .with_client_auth_cert(certificate_chain(identity), private_key(identity))?;
    tls.alpn_protocols = vec![ALPN_LINK.to_vec()];
    tls.enable_early_data = false;
    let mut config = quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls)?));
    config.transport_config(transport_config());
    Ok(config)
}
