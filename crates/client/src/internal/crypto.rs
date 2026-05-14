use ring::digest::{SHA256, digest};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error, SignatureScheme};
use std::sync::Arc;

// pub fn generate_cert_and_token() -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>, String) {
//     // Creating the certificate
//     let cert = generate_simple_self_signed(vec!["localhost".into()]).unwrap();
//
//     // Calculate tokens as SHA256 hash of certificate
//     let cert_der = cert.cert.der().clone();
//
//     // Extract private key
//     let priv_key_bytes = cert.signing_key.serialize_der();
//     let priv_key = PrivatePkcs8KeyDer::from(priv_key_bytes);
//
//     // Calculate tokens
//     let hash = digest(&SHA256, cert_der.as_ref());
//     let token = hex::encode(hash.as_ref());
//
//     let cert_chain = vec![cert_der];
//     (cert_chain, PrivateKeyDer::Pkcs8(priv_key), token)
// }

/// Own special verifier for the peer
#[derive(Debug)]
pub struct TokenVerifier {
    expected_hash: Vec<u8>,
}

impl TokenVerifier {
    /// Initializes a new token verifier
    ///
    /// # Arguments
    ///
    /// * `token_hex` - A hex encoded token to use for verification
    ///
    /// # Panics
    ///
    /// * If the given `token_hex` isn't correctly hex-encoded. (So the decoding fails)
    pub fn new(token_hex: &str) -> Arc<Self> {
        let bytes = hex::decode(token_hex).expect("Invalid token format, expected hash");
        Arc::new(Self {
            expected_hash: bytes,
        })
    }
}

impl ServerCertVerifier for TokenVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        // If no token is provided, skip verification (useful for local testing)
        if self.expected_hash.is_empty() {
            return Ok(ServerCertVerified::assertion());
        }

        // Calculate received hash
        let cert_hash = digest(&SHA256, end_entity.as_ref());

        // Compare with user's token
        if cert_hash.as_ref() == self.expected_hash {
            Ok(ServerCertVerified::assertion())
        } else {
            // Hash is not matching - alert
            Err(Error::General("SECURITY ALERT: Token not matching!".into()))
        }
    }

    // Following methods are just boilerplate, to work around the signature check
    // Note: We don't have to signature check, as we trust the hash
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        // Wir akzeptieren alle gängigen Schemata
        vec![
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ED25519,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_verifier_success() {
        // 1. Generate a real cert to test against
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert_der = cert.cert.der().clone();
        
        // 2. Calculate the token (hex hash)
        let hash = digest(&SHA256, cert_der.as_ref());
        let token = hex::encode(hash.as_ref());

        // 3. Create verifier
        let verifier = TokenVerifier::new(&token);

        // 4. Verify
        let der = CertificateDer::from(cert_der.as_ref());
        let result = verifier.verify_server_cert(
            &der,
            &[],
            &ServerName::try_from("localhost").unwrap(),
            &[],
            UnixTime::now(),
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_token_verifier_failure() {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert_der = cert.cert.der().clone();
        
        // Create verifier with a WRONG token
        let verifier = TokenVerifier::new("0000000000000000000000000000000000000000000000000000000000000000");

        let der = CertificateDer::from(cert_der.as_ref());
        let result = verifier.verify_server_cert(
            &der,
            &[],
            &ServerName::try_from("localhost").unwrap(),
            &[],
            UnixTime::now(),
        );

        assert!(result.is_err());
        if let Err(Error::General(msg)) = result {
            assert!(msg.contains("SECURITY ALERT"));
        } else {
            panic!("Expected security alert error");
        }
    }

    #[test]
    fn test_token_verifier_skip() {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert_der = cert.cert.der().clone();
        
        // Empty token should skip verification
        let verifier = TokenVerifier::new("");

        let der = CertificateDer::from(cert_der.as_ref());
        let result = verifier.verify_server_cert(
            &der,
            &[],
            &ServerName::try_from("localhost").unwrap(),
            &[],
            UnixTime::now(),
        );

        assert!(result.is_ok());
    }
}
