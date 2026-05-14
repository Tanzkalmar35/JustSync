use quinn::{Connection, Endpoint, ServerConfig, VarInt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;

pub mod connection;
pub mod models;
pub mod server;
pub mod session;

use crate::server::Server;
use crate::session::Session;

#[derive(Deserialize, Serialize, Debug)]
pub enum ControlMessage {
    Register { key: String },
    SessionCreated { status: String, name: String },
    Join { name: String, key: String },
    SessionJoined { status: String },
}

pub async fn run_relay(listen_addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_der = cert.cert.der().clone();
    let priv_key_bytes = cert.signing_key.serialize_der();
    let priv_key = rustls_pki_types::PrivatePkcs8KeyDer::from(priv_key_bytes);

    let cert_chain = vec![cert_der];
    let key = rustls_pki_types::PrivateKeyDer::Pkcs8(priv_key);

    let mut crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)?;
    crypto.alpn_protocols = vec![b"justsync".to_vec()];

    let mut server_config = ServerConfig::with_crypto(std::sync::Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(crypto)?,
    ));

    let mut transport_config = quinn::TransportConfig::default();
    transport_config.max_concurrent_bidi_streams(VarInt::from_u32(100));
    transport_config.max_concurrent_uni_streams(VarInt::from_u32(100));
    transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(60).try_into()?));
    transport_config.keep_alive_interval(Some(std::time::Duration::from_secs(15)));

    server_config.transport_config(std::sync::Arc::new(transport_config));

    let endpoint = Endpoint::server(server_config, listen_addr)?;
    let server = Server::setup();

    while let Some(incoming) = endpoint.accept().await {
        let server_ref = server.clone();
        tokio::spawn(async move {
            if let Ok(connection) = incoming.await {
                let _ = handle_connection(connection, &server_ref).await;
            }
        });
    }

    Ok(())
}

async fn handle_connection(
    connection: Connection,
    server: &Server,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut send, mut recv) = connection.accept_bi().await?;
    let mut buf = vec![0u8; 1024];
    let n = recv.read(&mut buf).await?.unwrap_or(0);
    let msg: ControlMessage = serde_json::from_slice(&buf[..n])?;

    match msg {
        ControlMessage::Register { key } => {
            let session = Session::new(Arc::new(connection.clone()), key);
            let session_name = session.name.clone();
            server.register_session(session.clone());
            let ans = ControlMessage::SessionCreated {
                status: String::from("ok"),
                name: session_name,
            };
            send.write_all(&serde_json::to_vec(&ans)?).await?;
            send.finish()?;
        }
        ControlMessage::Join {
            name: session_id,
            key,
        } => {
            if let Some(mut session) = server.find_session(&session_id) {
                let _ = session.join(Arc::new(connection.clone()), key, &mut send).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(3600 * 24)).await;
            send.finish()?;
        }
        _ => {}
    }

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        if connection.close_reason().is_some() {
            break;
        }
    }
    Ok(())
}
