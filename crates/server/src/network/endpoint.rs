use std::{fs::File, io::BufReader, net::SocketAddr, path::Path};

use just_sync_protocol::alpn;
use quinn::{Endpoint, ServerConfig, VarInt};
use tracing::info;

use crate::ServerArgs;

pub(crate) struct QuicEndpoint {}

impl QuicEndpoint {
    pub(crate) fn setup(args: &ServerArgs) -> Result<Endpoint, Box<dyn std::error::Error>> {
        let server_config = if args.dev {
            info!("DEV_MODE: Generating self-signed certificate for localhost...");
            Self::generate_self_signed_config()?
        } else {
            let cert_path_str = args
                .cert
                .as_ref()
                .ok_or("Missing --cert path. Use --dev for self-signed certs.")?;
            let key_path_str = args
                .key
                .as_ref()
                .ok_or("Missing --key path. Use --dev for self-signed certs.")?;

            let cert_path = Path::new(cert_path_str);
            let key_path = Path::new(key_path_str);
            Self::load_certs(cert_path, key_path)?
        };

        // Bind the endpoint
        let listen_addr: SocketAddr = format!("0.0.0.0:{}", args.port).parse()?;
        let endpoint = Endpoint::server(server_config, listen_addr)?;
        info!("Relay running on {listen_addr}");
        Ok(endpoint)
    }

    fn generate_self_signed_config() -> Result<ServerConfig, Box<dyn std::error::Error>> {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
        let cert_der = cert.cert.der().clone();
        let priv_key_bytes = cert.signing_key.serialize_der();
        let priv_key = rustls_pki_types::PrivatePkcs8KeyDer::from(priv_key_bytes);

        let cert_chain = vec![cert_der];
        let key = rustls_pki_types::PrivateKeyDer::Pkcs8(priv_key);

        let mut crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain.clone(), key)?;
        crypto.alpn_protocols = alpn();

        let mut server_config = ServerConfig::with_crypto(std::sync::Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(crypto)?,
        ));

        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_concurrent_bidi_streams(VarInt::from_u32(100));
        transport_config.max_concurrent_uni_streams(VarInt::from_u32(100));
        transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(60).try_into()?));
        transport_config.keep_alive_interval(Some(std::time::Duration::from_secs(15)));

        server_config.transport_config(std::sync::Arc::new(transport_config));

        // Print the token (hash of the certificate) so the client can use it for verification
        let hash = ring::digest::digest(&ring::digest::SHA256, cert_chain[0].as_ref());
        let token = hex::encode(hash.as_ref());
        info!("--- DEV_MODE TOKEN: {token} ---");
        info!("Use this token in your client configuration to verify the self-signed certificate.");

        Ok(server_config)
    }

    /// Loads TLS certificates from file
    ///
    /// # Arguments
    ///
    /// * `cert_path` - The path to the certificate file
    /// * `key_path` - The path to the key file
    ///
    /// # Errors
    ///
    /// * If `cert_path` points to no or an invalid file
    /// * If `key_path` points to no or an invalid file
    fn load_certs(
        cert_path: &Path,
        key_path: &Path,
    ) -> Result<ServerConfig, Box<dyn std::error::Error>> {
        info!("Loading TLS certificates...");

        // Open the certificate and key files
        let cert_file = File::open(cert_path)
            .map_err(|e| format!("Failed to open cert file at {}: {e}", cert_path.display()))?;
        let key_file = File::open(key_path)
            .map_err(|e| format!("Failed to open key file at {}: {e}", key_path.display()))?;

        let mut cert_reader = BufReader::new(cert_file);
        let mut key_reader = BufReader::new(key_file);

        // Parse the certificate chain
        // rustls_pemfile::certs returns an iterator of Results, so we collect them into a Vec
        let certs = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to parse certificates: {e}"))?;

        if certs.is_empty() {
            return Err("No certificates found in the PEM file".into());
        }

        // Parse the private key
        // rustls_pemfile handles RSA, PKCS8, and SEC1 keys automatically
        let key = rustls_pemfile::private_key(&mut key_reader)?
            .ok_or("No private key found in the PEM file")?;

        // Build the Quinn ServerConfig
        let mut crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;
        crypto.alpn_protocols = alpn();

        let mut server_config = ServerConfig::with_crypto(std::sync::Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(crypto)?,
        ));

        // Configure keep-alives so NAT routers don't drop idle connections
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(60).try_into()?));
        transport_config.keep_alive_interval(Some(std::time::Duration::from_secs(15)));

        server_config.transport_config(std::sync::Arc::new(transport_config));

        info!("TLS certificates loaded successfully.");
        Ok(server_config)
    }
}
