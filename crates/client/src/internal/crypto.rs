use ring::{digest::{SHA256, digest}, rand::SystemRandom, signature::Ed25519KeyPair};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};

/// Generate a private/public keypair using ed25519.
///
/// # Returns
///
/// * Ok(keypair) if successful
/// * Err, if pkcs generation or keypair generation fails.
#[must_use]
pub fn generate_keypair() -> Result<Ed25519KeyPair> {
    let rng = SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng)?;
    let keypair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())?;

    Ok(keypair)
}

/// Calculates the SHA256 hash of the given key.
///
/// # Arguments
///
/// * `key` - The key to calculate the hash of.
///
/// # Returns
///
/// The calculated SHA256 hash, hex encoded.
#[must_use]
pub fn hash(key: &str) -> String {
    let hash = digest(&SHA256, key.as_bytes());
    hex::encode(hash.as_ref())
}

/// As this architecture works on zero trust using E2EE and SPAKE2, there's no need to verify any
/// certs. Therefore we just accept all traffic without checking, because it's all just encrypted
/// gibberish anyways (apart from setup traffic).

#[derive(Debug)]
pub struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
