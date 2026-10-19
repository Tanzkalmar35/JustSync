use clap::Parser;
use just_sync_protocol::alpn;
use quinn::{Connection, Endpoint, ServerConfig};
use ring::rand::SystemRandom;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tracing::{error, info};

use crate::{core::CoreActor, network::NetworkActor};

mod core;
mod network;
mod models;
mod connection;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct ServerArgs {
    /// Port to listen on
    #[arg(short, long, default_value_t = 5000)]
    port: u16,

    /// Run in development mode (generates self-signed certs)
    #[arg(long, default_value_t = false)]
    dev: bool,

    /// Path to the TLS certificate fullchain.pem (Required if not in --dev mode)
    #[arg(long)]
    cert: Option<String>,

    /// Path to the TLS private key privkey.pem (Required if not in --dev mode)
    #[arg(long)]
    key: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Install default crypto provider for rustls
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Initialize logging
    tracing_subscriber::fmt::init();

    let args = ServerArgs::parse();

    let core = CoreActor::new(SystemRandom::new());
    let netw = NetworkActor::new(core, &args)?;
    netw.start().await?;

    Ok(())
}

// async fn handle_connection(
//     connection: Connection,
//     server: &Server,
// ) -> Result<(), Box<dyn std::error::Error>> {
//     // Wait for the client to open the first bidirectional stream (the Control Channel)
//     let (mut send, mut recv) = connection.accept_bi().await?;
//
//     // Read first message
//     let mut buf = vec![0u8; 1024];
//     let n = recv.read(&mut buf).await?.unwrap_or(0);
//
//     let msg: ControlMessage = serde_json::from_slice(&buf[..n])?;
//
//     match msg {
//         ControlMessage::Register { key } => {
//             let session = Session::new(Arc::new(connection.clone()), key);
//             let session_name = session.name.clone();
//
//             info!("Host registering session: {session_name}");
//
//             server.register_session(session.clone());
//
//             let ans = ControlMessage::SessionCreated {
//                 status: String::from("ok"),
//                 name: session_name.clone(),
//             };
//             send.write_all(&serde_json::to_vec(&ans)?).await?;
//             send.finish()?;
//         }
//         ControlMessage::Join {
//             name: session_id,
//             key,
//         } => {
//             info!("Peer trying to join session: {session_id}");
//
//             // Look up the Host's connection in the map
//             if let Some(mut session) = server.find_session(&session_id) {
//                 if let Err(e) = session
//                     .join(Arc::new(connection.clone()), key.clone(), &mut send)
//                     .await
//                     && e.eq("Error joining session - invalid key")
//                 {
//                     // send.write_all(b"{\"status\":\"error\", \"reason\":\"Invalid key\"}")
//                     //     .await?;
//                     info!("Invalid key: {}", key.clone());
//                 }
//             } else {
//                 // send.write_all(b"{\"status\":\"error\", \"reason\":\"session not found\"}")
//                 //     .await?;
//                 info!("Session to join not found: {}", session_id.clone());
//             }
//             tokio::time::sleep(std::time::Duration::from_secs(3600 * 24)).await;
//             send.shutdown().await.map_err(|e| e.to_string())?;
//         }
//         _ => {
//             error!("Invalid controlmessage received");
//         }
//     }
//
//     loop {
//         tokio::time::sleep(std::time::Duration::from_secs(60)).await;
//         if connection.close_reason().is_some() {
//             break;
//         }
//     }
//     Ok(())
// }
