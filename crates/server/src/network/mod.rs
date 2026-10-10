use std::sync::Arc;

use quinn::Endpoint;
use tokio::io::AsyncWriteExt;
use tracing::{error, info};

use crate::core::CoreActor;
use crate::models::Connection;
use crate::network::connection::PeerConnection;

mod connection;
mod endpoint;
mod protocol;

pub(crate) struct NetworkActor {
    /// Send events to the core actor
    core: CoreActor,

    endpoint: quinn::Endpoint,
}

impl NetworkActor {
    /// Create a new object of the network actor.
    ///
    /// # Arguments
    ///
    /// * `core_tx` - The sender that allows sending events to the core actor.
    ///
    /// # Returns
    ///
    /// An object of type `NetworkActor`.
    pub(crate) fn new(core: CoreActor, endpoint: Endpoint) -> Self {
        Self { core, endpoint }
    }

    pub(crate) async fn start(&self) -> anyhow::Result<()> {
        // Loop of accepting new peers
        while let Some(incoming) = self.endpoint.accept().await {
            tokio::spawn(async move {
                match incoming.await {
                    Ok(connection) => {
                        info!(
                            "New connection attempt from: {}",
                            connection.remote_address()
                        );

                        let conn = PeerConnection::new();
                        if let Err(e) = conn.start(connection).await {
                            error!("Connection handler failed: {e}");
                        }
                    }
                    Err(e) => error!("Failed to establish QUIC connection: {e}"),
                }
            });
        }

        Ok(())
    }

    /// Establishes a connection between 2 peers connected to this relay.
    ///
    /// The connection is essentially a chain of bidirectional streams. Peer `a` and peer `b` each
    /// hold connections to the relay server, but not to each other. Therefore, each of those peers
    /// opens up a new bidirectional stream to the relay, and the relay just patches the two ends
    /// together, so that each peer has a bidirectional stream connection to each other peer.
    ///
    /// # Arguments
    ///
    /// * `a` - Peer A to be connected to peer B.
    /// * `b` - Peer B to be connected to peer A.
    ///
    /// # Panics
    ///
    /// * If opening up a bidirectional stream to either peers fails.
    /// * If either copy task experiences an error.
    async fn establish_connection(a: Arc<dyn Connection>, b: Arc<dyn Connection>) {
        // Open a <-> relay and b <-> relay streams
        println!(
            "Establishing connection between 2 peers: {} to {}",
            a.remote_address(),
            b.remote_address()
        );
        let (mut a_send, mut a_recv) = a
            .open_bidi_stream()
            .await
            .expect("Couldn't open new peer stream (relay <-> host)");
        let (mut b_send, mut b_recv) = b
            .open_bidi_stream()
            .await
            .expect("Couldn't open stream to host (relay <-> peer)");

        let _ = a_send.write_all(&[0, 0, 0, 0]).await;
        let _ = b_send.write_all(&[0, 0, 0, 0]).await;

        println!("Connection established successfully");

        // Join streams
        tokio::spawn(async move {
            if let Err(e) = tokio::io::copy(&mut b_recv, &mut a_send).await {
                eprintln!("Connection establishment copy (B->A) error: {e}");
            }
            let _ = a_send.shutdown().await;
        });
        tokio::spawn(async move {
            if let Err(e) = tokio::io::copy(&mut a_recv, &mut b_send).await {
                eprintln!("Connection establishment copy (A->B) error: {e}");
            }
            let _ = b_send.shutdown().await;
        });

        println!("Copy tasks started");
    }
}

// match ping {
//     ControlMessage::Register { key } => {
//         let session = Session::new(Arc::new(connection.clone()), key);
//         let session_name = session.name.clone();
//         server.register_session(session.clone());
//         let ans = ControlMessage::SessionCreated {
//             status: String::from("ok"),
//             name: session_name,
//         };
//         send.write_all(&serde_json::to_vec(&ans)?).await?;
//         send.finish()?;
//     }
//     ControlMessage::Join {
//         name: session_id,
//         key,
//     } => {
//         if let Some(mut session) = server.find_session(&session_id) {
//             let _ = session
//                 .join(Arc::new(connection.clone()), key, &mut send)
//                 .await;
//         }
//         tokio::time::sleep(std::time::Duration::from_secs(3600 * 24)).await;
//         send.finish()?;
//     }
//     _ => {}
// }
