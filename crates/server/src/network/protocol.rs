use anyhow::anyhow;
use just_sync_protocol::client_relay_handshake::{
    Capabilities, CreateSessionMsg, PingRelayIntend, PingRelayMsg,
};
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
    let msg: CreateSessionMsg = postcard::from_bytes(&buf[..n])?;

    return if let CreateSessionMsg::Register { p_version } = msg {
        Ok(p_version)
    } else {
        Err(anyhow!(
            "Invalid message received: expected CreateSessionMsg::Register"
        ))
    };
}

/// Sends a `CreateSessionMsg::Challenge` to a peer via the provided quinn::SendStream.
///
/// # Arguments
///
/// * `send` - The stream to send the message to the peer through.
/// * `nonce` - The generated nonce used for challenging the peer.
pub(super) async fn send_challenge(send: &mut SendStream, nonce: [u8; 32]) -> anyhow::Result<()> {
    let msg = CreateSessionMsg::Challenge { nonce };

    send.write_all(&postcard::to_stdvec(&msg)?).await?;
    send.finish()?;

    Ok(())
}

/// Waits for the provided stream to receive a `CreateSessionMsg::CreateSession`, containing the
/// challenge signature and the public key of the peer's keypair.
///
/// # Arguments
///
/// * `recv` - The stream to receive messages from the peer from.
///
/// # Returns
///
/// A tuple of (identity, signature), with the identity being the public key of the peer's keypair
/// used for the signature, and the signature being the peer's signature of the previously sent
/// challenge nonce. If it fails an error.
pub(super) async fn recv_create_session_cmd(
    recv: &mut RecvStream,
) -> anyhow::Result<([u8; 32], [u8; 32])> {
    let mut buf = vec![0u8; 1024];
    let n = recv.read(&mut buf).await?.unwrap_or(0);
    let msg: CreateSessionMsg = postcard::from_bytes(&buf[..n])?;

    return if let CreateSessionMsg::CreateSession {
        identity_pk,
        sig_id,
    } = msg
    {
        Ok((identity_pk, sig_id))
    } else {
        Err(anyhow!(
            "Invalid message received: expected CreateSessionMsg::CreateSesion"
        ))
    };
}

/// Sends a `CreateSessionMsg::SessionCreated` to the peer on the other end of the provided stream.
///
/// # Arguments
///
/// * `send` - The stream to send data to the peer through.
/// * `session_id` - The id of the generated session, the peer might i.e. use for invite generation.
/// * `capabilities` - The session's capabilities.
pub(super) async fn send_session_created(
    send: &mut SendStream,
    session_id: [u8; 16],
    capabilities: Capabilities,
) -> anyhow::Result<()> {
    let msg = CreateSessionMsg::SessionCreated {
        session_id,
        capabilities,
    };

    send.write_all(&postcard::to_stdvec(&msg)?).await?;
    send.finish()?;

    Ok(())
}
