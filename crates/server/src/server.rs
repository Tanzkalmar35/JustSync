use dashmap::DashMap;
use std::sync::Arc;

use crate::session::Session;

#[derive(Clone)]
pub struct Server {
    // Session name -> Session
    sessions: Arc<DashMap<String, Session>>,
}

impl Server {
    #[must_use]
    pub fn setup() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
        }
    }

    pub fn register_session(&self, mut session: Session) {
        while self.sessions.contains_key(&session.name) {
            session.regenerate_name();
        }

        self.sessions.insert(session.name.clone(), session);
    }

    /// Deregisters a session from the relay server
    ///
    /// # Arguments
    ///
    /// * `s` - The session to close
    ///
    /// # Errors
    ///
    /// * If the session to deregister isn't even registered
    pub fn deregister_session(&self, s: &str) -> Result<(), String> {
        if !self.sessions.contains_key(s) {
            return Err(String::from(
                "Error deregistering session - No session to deregister found!",
            ));
        }

        self.sessions.remove(s);
        Ok(())
    }

    #[must_use]
    pub fn find_session(&self, name: &str) -> Option<Session> {
        self.sessions.get(name).map(|s| s.value().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Connection;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite};

    // A minimal Mock Connection that implements our new trait
    #[derive(Clone)]
    struct MockConnection;

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
            // We don't even need real streams for registry tests
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
                    _: &[u8],
                ) -> Poll<std::io::Result<usize>> {
                    Poll::Ready(Ok(0))
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
            "127.0.0.1:1234".parse().unwrap()
        }
    }

    #[test]
    fn test_registration_and_lookup() {
        let server = Server::setup();
        let conn = Arc::new(MockConnection);
        let session = Session::new(conn, "secret-key".to_string());
        let name = session.name.clone();

        server.register_session(session);

        let found = server.find_session(&name);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, name);
    }

    #[test]
    fn test_collision_handling() {
        let server = Server::setup();
        let conn = Arc::new(MockConnection);

        // 1. Register first session
        let s1 = Session::new(conn.clone(), "key1".to_string());
        let name1 = s1.name.clone();
        server.register_session(s1);

        // 2. Create second session and FORCIBLY give it the same name
        let mut s2 = Session::new(conn.clone(), "key2".to_string());
        s2.name = name1.clone();

        // 3. Register it - Server should force a rename
        server.register_session(s2);

        // 4. Verify we now have two different sessions
        assert_eq!(server.sessions.len(), 2);

        let found1 = server.find_session(&name1).unwrap();
        assert_eq!(found1.name, name1);
    }

    #[test]
    fn test_deregister() {
        let server = Server::setup();
        let conn = Arc::new(MockConnection);
        let s = Session::new(conn, "key".to_string());
        let name = s.name.clone();

        server.register_session(s);
        assert!(server.find_session(&name).is_some());

        let res = server.deregister_session(name.as_str());
        assert!(res.is_ok());
        assert!(server.find_session(&name).is_none());
    }

    #[tokio::test]
    async fn test_concurrent_registrations() {
        let server = Arc::new(Server::setup());
        let mut handles = vec![];

        for _ in 0..50 {
            let s_ref = server.clone();
            handles.push(tokio::spawn(async move {
                let conn = Arc::new(MockConnection);
                let session = Session::new(conn, "key".to_string());
                s_ref.register_session(session);
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(server.sessions.len(), 50);
    }
}
