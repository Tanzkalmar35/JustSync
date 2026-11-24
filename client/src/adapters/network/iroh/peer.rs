use anyhow::{Context, bail};
use iroh::endpoint::{RecvStream, SendStream};

use crate::adapters::network::protocol::{AsyncReceiver, AsyncSender};

#[derive(Debug)]
pub(in crate::adapters::network) struct IrohPeer {
    pub(in crate::adapters::network) sender: SendStream,
    pub(in crate::adapters::network) receiver: RecvStream,
}

impl AsyncSender for IrohPeer {
    /// Send a message out over the wire.
    ///
    /// # Arguments
    ///
    /// * `send` - The network's send stream with a remote peer on the other end.
    /// * `message` - The message to send to the remote peer. Message type must be serializable.
    ///
    /// # Returns
    ///
    /// Nothing, if successful, an `anyhow::Error` otherwise.
    async fn send<T: serde::Serialize>(&mut self, message: &T) -> anyhow::Result<()> {
        let payload = postcard::to_allocvec(message)?;

        let length = u32::try_from(payload.len())?;

        self.sender.write_all(&length.to_be_bytes()).await?;
        self.sender.write_all(&payload).await?;

        Ok(())
    }
}

impl AsyncReceiver for IrohPeer {
    /// Waits for a new message to arrive in the given receiver stream, reads it in a predefined format
    /// and attempts deserialization into `T`.
    ///
    /// # Arguments
    ///
    /// * `recv` - The receiver stream to wait for a new message for.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(msg: T))` if successful
    /// * `Err(e)` if the msg has no length prefix, or if reading the new message or deserializing it
    /// fails.
    /// * `Ok(None)` if reading the new message's length prefix fails.
    async fn recv<T: serde::de::DeserializeOwned>(
        &mut self,
    ) -> anyhow::Result<Option<T>> {
        // 4 byte length prefix
        let mut header = [0u8; 4];

        // Read the message length.
        match self.receiver.read_exact(&mut header).await {
            Ok(()) => {}
            Err(_) => {
                // EOF -> Temporarily fine. Might improve edge case handling later on.
                return Ok(None);
            }
        }

        let length = u32::from_be_bytes(header) as usize;
        if length == 0 {
            bail!("Invalid message length: {length}");
        }

        let mut buffer = vec![0u8; length];

        self.receiver
            .read_exact(&mut buffer)
            .await
            .context("Failed to read message payload")?;

        let message: T = postcard::from_bytes(&buffer).context("Failed to deserialize message")?;

        Ok(Some(message))
    }
}
