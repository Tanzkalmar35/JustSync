use crate::ServerArgs;

pub(crate) struct QuicEndpoint {}

impl QuicEndpoint {
    fn setup(args: &ServerArgs) -> Result<Endpoint, Box<dyn std::error::Error>> {
        let server_config = if args.dev {
            info!("DEV_MODE: Generating self-signed certificate for localhost...");
            generate_self_signed_config()?
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
            load_certs(cert_path, key_path)?
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
}
