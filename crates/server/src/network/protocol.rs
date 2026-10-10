use anyhow::anyhow;
use just_sync_protocol::client_relay_handshake::{CreateSessionMsg, PingRelayIntend, PingRelayMsg};
use quinn::{RecvStream, SendStream};

/// Receives a ping message from a peer, signaling an upcoming register event.
///
/// # Arguments
///
/// * `recv` - The receiving stream for the connection between this relay and the new peer.
///
/// # Returns
///
/// The ping message's intend, or an error if something along the way fails.
pub(super) async fn recv_ping(recv: &mut RecvStream) -> anyhow::Result<PingRelayIntend> {
    let mut buf = vec![0u8; 1024];
    let n = recv.read(&mut buf).await?.unwrap_or(0);
    let msg = postcard::from_bytes(&buf[..n])?;

    return if let PingRelayMsg::Ping { intend } = msg {
        Ok(intend)
    } else {
        Err(anyhow!("Invalid ping attempt from peer"))
    };
}

/// Sends a pong message to a peer. Pong is the answer to a peer sending Ping.
///
/// # Arguments
///
/// * `send` - The stream to send messages to the peer.
pub(super) async fn send_pong(send: &mut SendStream) -> anyhow::Result<()> {
    let msg = PingRelayMsg::Pong {};

    send.write_all(&postcard::to_stdvec(&msg)?).await?;
    send.finish()?;
    Ok(())
}

/// Receives a `CreateSessionMsg::Register` from the given stream.
///
/// # Arguments
///
/// * `recv` - The stream to receive the message from.
///
/// # Returns
///
/// The protocol version provided by the message's body, or an error.
pub(super) async fn recv_create_session_register(recv: &mut RecvStream) -> anyhow::Result<u16> {
    let mut buf = vec![0u8; 1024];
    let n = recv.read(&mut buf).await?.unwrap_or(0);
    let msg = postcard::from_bytes(&buf[..n])?;

    return if let CreateSessionMsg::Register { p_version } = msg {
        Ok(p_version)
    } else {
        Err(anyhow!(
            "Invalid message received: expected: CreateSessionMsg::Register"
        ))
    };
}
