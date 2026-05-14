use tokio::io::{AsyncRead, AsyncWrite};

#[async_trait::async_trait]
pub trait Connection: Send + Sync {
    async fn open_bidi_stream(
        &self,
    ) -> Result<
        (
            Box<dyn AsyncWrite + Unpin + Send>,
            Box<dyn AsyncRead + Unpin + Send>,
        ),
        String,
    >;

    fn remote_address(&self) -> std::net::SocketAddr;
}

#[async_trait::async_trait]
impl Connection for quinn::Connection {
    async fn open_bidi_stream(
        &self,
    ) -> Result<
        (
            Box<dyn AsyncWrite + Unpin + Send>,
            Box<dyn AsyncRead + Unpin + Send>,
        ),
        String,
    > {
        let (send, recv) = self.open_bi().await.map_err(|e| e.to_string())?;
        Ok((Box::new(send), Box::new(recv)))
    }

    fn remote_address(&self) -> std::net::SocketAddr {
        self.remote_address()
    }
}
