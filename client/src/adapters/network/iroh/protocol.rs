use std::sync::Mutex;

use iroh::endpoint::{RecvStream, SendStream};

#[derive(Debug)]
pub(super) struct IrohPeerManager {
    peers: Mutex<Vec<(SendStream, RecvStream)>>,
}

impl iroh::protocol::ProtocolHandler for IrohPeerManager {
    /// Accepts a new connection.
    ///
    /// # Returns
    ///
    /// Nothing, if successful, an `iroh::protocol::AcceptError` if not.
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        let endpoint_id = connection.remote_id();
        println!("accepted connection from {endpoint_id}");

        let (send, recv) = connection.accept_bi().await?;
        self.peers.lock().unwrap().push((send, recv));

        Ok(())
    }
}

impl IrohPeerManager {
    fn active_peers(&self) -> Vec<(SendStream, RecvStream)> {
        self.peers.lock().unwrap()
    }
}
