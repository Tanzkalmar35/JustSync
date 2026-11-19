use anyhow::Result;
use anyhow::{Context, bail};
use iroh::endpoint::RecvStream;
use iroh::endpoint::SendStream;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) enum SetupMessage {
    /// Peer -> Host
    Id { agent_id: String },

    /// Peer -> Host
    RequestFullSync,

    /// Host -> Peer
    FullSyncResponse { files: Vec<(String, Vec<u8>)> },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) enum WireEvent {
    Patch {
        uri: String,
        data: Vec<u8>,
    },

    Cursor {
        uri: String,
        position: (usize, usize),
    },
}

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
pub(crate) async fn send_message<T: serde::Serialize>(send: &mut SendStream, message: &T) -> Result<()> {
    let payload = postcard::to_allocvec(message)?;

    let length = u32::try_from(payload.len())?;

    send.write_all(&length.to_be_bytes()).await?;
    send.write_all(&payload).await?;

    Ok(())
}

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
pub(crate) async fn receive_message<T: serde::de::DeserializeOwned>(
    recv: &mut RecvStream,
) -> Result<Option<T>> {
    // 4 byte length prefix
    let mut header = [0u8; 4];

    // Read the message length.
    match recv.read_exact(&mut header).await {
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

    recv.read_exact(&mut buffer)
        .await
        .context("Failed to read message payload")?;

    let message: T = postcard::from_bytes(&buffer).context("Failed to deserialize message")?;

    Ok(Some(message))
}
