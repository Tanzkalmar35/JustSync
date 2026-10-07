use just_sync_server::Event;
use quinn::{RecvStream, SendStream};

use crate::core::CoreActor;

pub(crate) struct NetworkActor {
    /// Receive events from the core actor.
    // recv: RecvStream<Event>,

    /// Send events to the core actor
    core_tx: SendStream<Event>,
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
    pub fn new(core_tx: SendStream<Event>) -> Self {
        Self { core_tx }
    }

    pub fn start(&self) -> Result<()> {
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
