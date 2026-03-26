use std::{net::SocketAddr, path::Path};

use quinn::{Connection, Endpoint};
use serde::{Deserialize, Serialize};

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
    session_name: String
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
    let server_config = load_production_certs(cert_path, key_path)?;

    // Bind the endpoint
    let listen_addr: SocketAddr = "0.0.0.0:5000".parse()?;
    let endpoint = Endpoint::server(server_config, listen_addr)?;
    println!("Production Relay running on {}", listen_addr);
    Ok(endpoint)
}

async fn handle_connection(
    connection: Connection,
    server: &Server,
) -> Result<(), Box<dyn std::error::Error>> {
    // Wait for the client to open the first bidirectional stream (the Control Channel)
    let (mut send, mut recv) = connection.accept_bi().await?;

    // Read the first message to see what they want
    let mut buf = vec![0u8; 1024];
    let n = recv.read(&mut buf).await?.unwrap_or(0);

    // Parse the JSON message
    let msg: ControlMessage = serde_json::from_slice(&buf[..n])?;

    match msg {
        ControlMessage::Register { key } => {
            let session = Session::new(connection.clone(), key);
            let session_name = session.name.clone();

            println!("Host registering session: {}", session_name);
            server.register_session(session);

            // Send an "OK" back to the Host
            let ans = SessionCreatedMessage{
                status: String::from("ok"),
                session_name: session_name.clone(),
            };
            send.write_all(&serde_json::to_vec(&ans)?).await?;
            send.finish()?;

            // Host now waits for peers. If this loop ends, the Host disconnected.
            // When they disconnect, we MUST clean up the map!
            let disconnect_reason = connection.closed().await;
            println!(
                "Host left ({:?}). Removing session: {}",
                disconnect_reason, session_name
            );

            server.deregister_session(session_name);
        }
        ControlMessage::Join { session_id, key } => {
            println!("Peer trying to join session: {}", session_id);

            // Look up the Host's connection in the map
            if let Some(session) = server.find_session(&session_id) {
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
