use std::sync::{Arc, Mutex};

use rand::RngExt;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::{ControlMessage, connection::hotwire, models::Connection};

#[derive(Clone)]
pub struct Session {
    pub name: String,
    key: String,
    pub host: Arc<dyn Connection>,
    pub peers: Arc<Mutex<Vec<Arc<dyn Connection>>>>,
}

impl Session {
    pub fn new(host: Arc<dyn Connection>, key: String) -> Self {
        Self {
            name: Self::generate_name(),
            key,
            host,
            peers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Allows peers to join the session
    ///
    /// # Arguments
    ///
    /// * `peer` - The peer to join
    /// * `key` - The provided session key
    /// * `send` - The sender to the control channel
    ///
    /// # Errors
    ///
    /// * If the provided session key does not match the actual
    /// * If `send` can't send a shutdown command
    ///
    /// # Panics
    ///
    /// * If no lock on
    /// * If `send` can't report status
    pub async fn join<W>(
        &mut self,
        peer: Arc<dyn Connection>,
        key: String,
        send: &mut W,
    ) -> Result<(), String>
    where
        W: AsyncWrite + Unpin + Send,
    {
        if !self.key.eq(&key) {
            return Err(String::from("Error joining session - invalid key"));
        }

        tokio::spawn(hotwire(self.host.clone(), peer.clone()));
        self.peers.lock().unwrap().iter().for_each(|p| {
            tokio::spawn(hotwire(p.clone(), peer.clone()));
        });

        let msg = ControlMessage::SessionJoined {
            status: String::from("ok"),
        };
        send.write_all(&serde_json::to_vec(&msg).unwrap())
            .await
            .expect("Couldn't report status");
        send.shutdown().await.map_err(|e| e.to_string())?;

        self.peers
            .lock()
            .expect("Couldn't lock peers...")
            .push(peer.clone());
        Ok(())
    }

    pub fn regenerate_name(&mut self) {
        self.name = Self::generate_name();
    }

    fn generate_name() -> String {
        let names = petname::petname(2, "-").expect("petname session name generation failed!");
        let mut rng = rand::rng();
        let number: u16 = rng.random_range(100..1000);

        format!("{names}-{number}")
    }
}

impl PartialEq for Session {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite};

    #[derive(Clone)]
    struct MockConnection {
        pub open_count: Arc<AtomicUsize>,
    }

    impl MockConnection {
        fn new() -> Self {
            Self {
                open_count: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    #[async_trait::async_trait]
    impl Connection for MockConnection {
        async fn open_bidi_stream(
            &self,
        ) -> Result<
            (
                Box<dyn AsyncWrite + Unpin + Send>,
                Box<dyn AsyncRead + Unpin + Send>,
            ),
            String,
        > {
            self.open_count.fetch_add(1, Ordering::SeqCst);
            struct DummyStream;
            impl AsyncRead for DummyStream {
                fn poll_read(
                    self: Pin<&mut Self>,
                    _: &mut Context<'_>,
                    _: &mut tokio::io::ReadBuf<'_>,
                ) -> Poll<std::io::Result<()>> {
                    Poll::Ready(Ok(()))
                }
            }
            impl AsyncWrite for DummyStream {
                fn poll_write(
                    self: Pin<&mut Self>,
                    _: &mut Context<'_>,
                    buf: &[u8],
                ) -> Poll<std::io::Result<usize>> {
                    Poll::Ready(Ok(buf.len()))
                }
                fn poll_flush(
                    self: Pin<&mut Self>,
                    _: &mut Context<'_>,
                ) -> Poll<std::io::Result<()>> {
                    Poll::Ready(Ok(()))
                }
                fn poll_shutdown(
                    self: Pin<&mut Self>,
                    _: &mut Context<'_>,
                ) -> Poll<std::io::Result<()>> {
                    Poll::Ready(Ok(()))
                }
            }
            Ok((Box::new(DummyStream), Box::new(DummyStream)))
        }
        fn remote_address(&self) -> std::net::SocketAddr {
            "127.0.0.1:0".parse().unwrap()
        }
    }

    #[tokio::test]
    async fn test_join_invalid_key() {
        let host = Arc::new(MockConnection::new());
        let mut session = Session::new(host, "correct-key".to_string());
        let peer = Arc::new(MockConnection::new());
        let mut sink = Vec::new();

        let result = session.join(peer, "wrong-key".to_string(), &mut sink).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Error joining session - invalid key");
    }

    #[tokio::test]
    async fn test_join_success() {
        let host = Arc::new(MockConnection::new());
        let mut session = Session::new(host, "key".to_string());
        let peer = Arc::new(MockConnection::new());
        let mut sink = Vec::new();

        let result = session.join(peer, "key".to_string(), &mut sink).await;
        assert!(result.is_ok());

        // Verify "ok" message was written to the stream
        let response: ControlMessage = serde_json::from_slice(&sink).unwrap();
        if let ControlMessage::SessionJoined { status } = response {
            assert_eq!(status, "ok");
        } else {
            panic!("Unexpected message type");
        }

        // Verify peer list was updated
        assert_eq!(session.peers.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_relay_logic_orchestration() {
        // This test verifies that the server attempts to open streams (hotwire)
        // between the correct parties.

        let host_conn = Arc::new(MockConnection::new());
        let mut session = Session::new(host_conn.clone(), "key".to_string());

        // Peer 1 Joins
        let p1_conn = Arc::new(MockConnection::new());
        let mut sink = Vec::new();
        session
            .join(p1_conn.clone(), "key".to_string(), &mut sink)
            .await
            .unwrap();

        // After P1 joins, we expect 1 hotwire task started (Host <-> P1).
        // Each hotwire task calls open_bidi_stream on both parties.
        // Wait a tiny bit for the spawned tasks to run
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(host_conn.open_count.load(Ordering::SeqCst), 1);
        assert_eq!(p1_conn.open_count.load(Ordering::SeqCst), 1);

        // Peer 2 Joins
        let p2_conn = Arc::new(MockConnection::new());
        session
            .join(p2_conn.clone(), "key".to_string(), &mut sink)
            .await
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // P2 joins -> triggers Host <-> P2 AND P1 <-> P2
        // Total Host opens: 2 (one for P1, one for P2)
        // Total P1 opens: 2 (one for Host, one for P2)
        // Total P2 opens: 2 (one for Host, one for P1)
        assert_eq!(host_conn.open_count.load(Ordering::SeqCst), 2);
        assert_eq!(p1_conn.open_count.load(Ordering::SeqCst), 2);
        assert_eq!(p2_conn.open_count.load(Ordering::SeqCst), 2);
    }
}
