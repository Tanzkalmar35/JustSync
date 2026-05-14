use std::sync::Arc;

use tokio::io::AsyncWriteExt;

use crate::models::Connection;

pub async fn hotwire(a: Arc<dyn Connection>, b: Arc<dyn Connection>) -> () {
    // Open a <-> relay and b <-> relay streams
    println!("Hotwiring {} to {}", a.remote_address(), b.remote_address());
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

    println!("Hotwired both");

    // Join streams
    tokio::spawn(async move {
        if let Err(e) = tokio::io::copy(&mut b_recv, &mut a_send).await {
            eprintln!("Hotwire copy (B->A) error: {}", e);
        }
        let _ = a_send.shutdown().await;
    });
    tokio::spawn(async move {
        if let Err(e) = tokio::io::copy(&mut a_recv, &mut b_send).await {
            eprintln!("Hotwire copy (A->B) error: {}", e);
        }
        let _ = b_send.shutdown().await;
    });

    println!("Copy tasks started");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

    struct MockConnection {
        // We use a channel or duplex to simulate the network
        pub stream_tx: tokio::sync::Mutex<Option<(Box<dyn tokio::io::AsyncWrite + Unpin + Send>, Box<dyn tokio::io::AsyncRead + Unpin + Send>)>>,
    }

    #[async_trait::async_trait]
    impl Connection for MockConnection {
        async fn open_bidi_stream(&self) -> Result<(Box<dyn tokio::io::AsyncWrite + Unpin + Send>, Box<dyn tokio::io::AsyncRead + Unpin + Send>), String> {
            let mut guard = self.stream_tx.lock().await;
            guard.take().ok_or_else(|| "Stream already opened".to_string())
        }
        fn remote_address(&self) -> std::net::SocketAddr {
            "127.0.0.1:0".parse().unwrap()
        }
    }

    #[tokio::test]
    async fn test_hotwire_data_flow() {
        // 1. Setup Duplex Streams for Peer A and Peer B
        // (one end goes to the relay, the other end is our "test handle")
        let (a_relay, mut a_test) = duplex(1024);
        let (b_relay, mut b_test) = duplex(1024);

        let (a_relay_read, a_relay_write) = tokio::io::split(a_relay);
        let (b_relay_read, b_relay_write) = tokio::io::split(b_relay);

        let conn_a = Arc::new(MockConnection {
            stream_tx: tokio::sync::Mutex::new(Some((Box::new(a_relay_write), Box::new(a_relay_read)))),
        });
        let conn_b = Arc::new(MockConnection {
            stream_tx: tokio::sync::Mutex::new(Some((Box::new(b_relay_write), Box::new(b_relay_read)))),
        });

        // 2. Start Hotwire
        hotwire(conn_a, conn_b).await;

        // 3. Verify Header: Both sides should receive [0,0,0,0]
        let mut header_a = [0u8; 4];
        let mut header_b = [0u8; 4];
        a_test.read_exact(&mut header_a).await.unwrap();
        b_test.read_exact(&mut header_b).await.unwrap();
        assert_eq!(header_a, [0, 0, 0, 0]);
        assert_eq!(header_b, [0, 0, 0, 0]);

        // 4. Verify Data Flow: A -> B
        a_test.write_all(b"Hello from A").await.unwrap();
        let mut buf = [0u8; 12];
        b_test.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"Hello from A");

        // 5. Verify Data Flow: B -> A
        b_test.write_all(b"Hello from B").await.unwrap();
        let mut buf = [0u8; 12];
        a_test.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"Hello from B");
    }
}
