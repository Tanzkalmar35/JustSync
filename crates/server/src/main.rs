use std::{net::SocketAddr, path::Path};

use quinn::{ServerConfig, Connection, Endpoint};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;

use crate::{server::Server, session::Session};

pub mod connection;
pub mod server;
pub mod session;

#[derive(Deserialize, Serialize, Debug)]
#[serde(tag = "action")]
enum ControlMessage {
    Register { key: String },
    Join { session_id: String, key: String },
}

#[derive(Serialize)]
struct SessionCreatedMessage {
    status: String,
    name: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = setup().await?;
    let server = Server::setup();

    // Handle incoming requests loop
    while let Some(incoming) = endpoint.accept().await {
        let server_ref = server.clone();

        tokio::spawn(async move {
            match incoming.await {
                Ok(connection) => {
                    println!("New raw connection from: {}", connection.remote_address());
                    if let Err(e) = handle_connection(connection, &server_ref).await {
                        eprintln!("Connection handler failed: {}", e);
                    }
                }
                Err(e) => eprintln!("Failed to establish QUIC connection: {}", e),
            }
        });
    }

    Ok(())
}

async fn setup() -> Result<Endpoint, Box<dyn std::error::Error>> {
    // Load Let's Encrypt Certs
    // Make sure these paths match what Certbot outputs on the server
    let cert_path = Path::new("/etc/letsencrypt/live/relay.yourdomain.com/fullchain.pem");
    let key_path = Path::new("/etc/letsencrypt/live/relay.yourdomain.com/privkey.pem");
    let server_config = load_certs(cert_path, key_path)?;

    // Bind the endpoint
    let listen_addr: SocketAddr = "0.0.0.0:5000".parse()?;
    let endpoint = Endpoint::server(server_config, listen_addr)?;
    println!("Production Relay running on {}", listen_addr);
    Ok(endpoint)
}

/// [TODO:description]
///
/// # Arguments
///
/// * `cert_path` - [TODO:description]
/// * `key_path` - [TODO:description]
///
/// # Errors
///
/// [TODO:describe error types and what triggers them]
///
/// # Examples
///
/// ```
/// [TODO:write some example code]
/// ```
pub fn load_certs(
    cert_path: &Path,
    key_path: &Path,
) -> Result<ServerConfig, Box<dyn std::error::Error>> {
    println!("Loading TLS certificates...");

    // 1. Open the certificate and key files
    let cert_file = File::open(cert_path)
        .map_err(|e| format!("Failed to open cert file at {:?}: {}", cert_path, e))?;
    let key_file = File::open(key_path)
        .map_err(|e| format!("Failed to open key file at {:?}: {}", key_path, e))?;

    let mut cert_reader = BufReader::new(cert_file);
    let mut key_reader = BufReader::new(key_file);

    // 2. Parse the certificate chain
    // rustls_pemfile::certs returns an iterator of Results, so we collect them into a Vec
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to parse certificates: {}", e))?;

    if certs.is_empty() {
        return Err("No certificates found in the PEM file".into());
    }

    // 3. Parse the private key
    // rustls_pemfile handles RSA, PKCS8, and SEC1 keys automatically
    let key = rustls_pemfile::private_key(&mut key_reader)?
        .ok_or("No private key found in the PEM file")?;

    // 4. Build the Quinn ServerConfig
    let mut server_config = ServerConfig::with_single_cert(certs, key)?;

    // OPTIONAL BUT HIGHLY RECOMMENDED:
    // Configure keep-alives so NAT routers don't drop idle connections
    let mut transport_config = quinn::TransportConfig::default();
    transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(60).try_into()?));
    transport_config.keep_alive_interval(Some(std::time::Duration::from_secs(15)));

    server_config.transport_config(std::sync::Arc::new(transport_config));

    println!("TLS certificates loaded successfully.");
    Ok(server_config)
}

async fn handle_connection(
    connection: Connection,
    server: &Server,
) -> Result<(), Box<dyn std::error::Error>> {
    // Wait for the client to open the first bidirectional stream (the Control Channel)
    let (mut send, mut recv) = connection.accept_bi().await?;

    // Read first message
    let mut buf = vec![0u8; 1024];
    let n = recv.read(&mut buf).await?.unwrap_or(0);

    let msg: ControlMessage = serde_json::from_slice(&buf[..n])?;

    match msg {
        ControlMessage::Register { key } => {
            let session = Session::new(connection.clone(), key);
            let session_name = session.name.clone();

            println!("Host registering session: {}", session_name);

            server.register_session(session.clone());

            let ans = SessionCreatedMessage {
                status: String::from("ok"),
                name: session_name.clone(),
            };
            send.write_all(&serde_json::to_vec(&ans)?).await?;
            send.finish()?;

            // Host now waits for peers. If this loop ends, the Host disconnected.
            let disconnect_reason = connection.closed().await;
            println!(
                "Host left ({:?}). Removing session: {}",
                disconnect_reason, session_name
            );

            server.deregister_session(session_name)?;
        }
        ControlMessage::Join { session_id, key } => {
            println!("Peer trying to join session: {}", session_id);

            // Look up the Host's connection in the map
            if let Some(mut session) = server.find_session(&session_id) {
                if let Err(e) = session.join(connection, key, &mut send).await
                    && e.eq("Error joining session - invalid key")
                {
                    send.write_all(b"{\"status\":\"error\", \"reason\":\"Invalid key\"}")
                        .await?;
                }
            } else {
                send.write_all(b"{\"status\":\"error\", \"reason\":\"session not found\"}")
                    .await?;
            }
            send.finish()?;
        }
    }

    Ok(())
}
