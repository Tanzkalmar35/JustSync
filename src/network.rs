use std::sync::Arc;

use quinn::{ClientConfig, Endpoint, ServerConfig};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct NetworkManager {
    pub endpoint: Endpoint,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum NetMessage {
    /// A generic handshake to verify protocol version (optional for now, but good practice)
    Handshake { version: String },

    // Initial 'cloning' payload
    ProjectState { files: Vec<(String, String)> },

    /// A binary blob representing a CRDT patch (from diamond-types)
    Sync { uri: String, data: Vec<u8> },

    // Future: Cursor { line: usize, col: usize }
}

impl NetworkManager {
    pub fn init_host(port: u16) -> Result<Self, Box<dyn std::error::Error>> {
        let (server_config, _cert) = configure_server()?;

        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
        let endpoint = Endpoint::server(server_config, addr)?;

        crate::logger::log(&format!(
            ">> [Network] Host listening on {}",
            endpoint.local_addr()?
        ));

        Ok(Self { endpoint })
    }

    pub fn init_client(bind_port: u16) -> Result<Self, Box<dyn std::error::Error>> {
        let client_config = configure_client();

        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], bind_port));
        let mut endpoint = Endpoint::client(addr)?;
        endpoint.set_default_client_config(client_config);

        Ok(Self { endpoint })
    }

    /// Connects to a Host at the specific IP:PORT
    pub async fn connect(
        &self,
        server_addr: std::net::SocketAddr,
    ) -> anyhow::Result<quinn::Connection> {
        // "localhost" is the server name we put in the cert.
        // If we used a real domain, this would matter.
        // Since we skip verification, it just needs to be non-empty.
        let connection = self.endpoint.connect(server_addr, "localhost")?.await?;

        crate::logger::log(&format!(
            ">> [Network] Successfully connected to {}",
            server_addr
        ));

        Ok(connection)
    }

    /// Host function: Wait for the next incoming Client
    pub async fn get_next_connection(&self) -> Option<quinn::Connection> {
        let incoming = self.endpoint.accept().await?;
        match incoming.await {
            Ok(conn) => {
                crate::logger::log(&format!(
                    ">> [Network] Connection accepted from {}",
                    conn.remote_address()
                ));
                Some(conn)
            }
            Err(e) => {
                crate::logger::log(&format!("!! [Network] Connection handshake failed: {}", e));
                None
            }
        }
    }
}

pub fn configure_server() -> Result<(ServerConfig, Vec<u8>), Box<dyn std::error::Error>> {
    // Generate the CertifiedKey
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;

    // Get the inner parts
    let cert_der = cert.cert;
    let key_pair = cert.signing_key;

    // Serialize the Key Pair to DER (Bytes)
    let key_der_bytes = key_pair.serialize_der();

    // Wrap it in the strict types rustls expects
    let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der_bytes));

    // Create the Config
    let config = ServerConfig::with_single_cert(vec![cert_der.clone().into()], private_key)?;

    // Return the config AND the raw cert bytes (so we can print/debug them if needed)
    // Note: cert_der is a specialized type, we convert it back to Vec<u8> for generic use if needed
    Ok((config, cert_der.der().to_vec()))
}

pub fn configure_client() -> ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();

    // Dangerous: Diabled certificate verification
    // We have to do this because we are using self-signed certs generated on the fly
    let mut crypto = crypto;
    crypto
        .dangerous()
        .set_certificate_verifier(Arc::new(SkipServerVerification));

    ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap(),
    ))
}

pub async fn send_message(stream: &mut quinn::SendStream, msg: &NetMessage) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(msg)?;

    // Write header and body to the stream
    stream.write_u32_le(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;

    // Should be auto-flushed, but just to make sure the message is sent directly
    stream.flush().await?;

    Ok(())
}

pub async fn recv_message(stream: &mut quinn::RecvStream) -> anyhow::Result<NetMessage> {
    // Read the length (4 bytes) directly as a u32
    let len = stream.read_u32_le().await?;

    // Create the buffer for the body
    let mut buffer = vec![0u8; len as usize];

    // Read the body
    stream.read_exact(&mut buffer).await?;

    // Deserialize
    let msg = serde_json::from_slice(&buffer)?;
    Ok(msg)
}

// The "Yes Man" Verifier
#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // BLIND TRUST
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
        ]
    }
}
