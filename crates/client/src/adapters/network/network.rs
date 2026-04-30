use anyhow::Result;
use quinn::{ClientConfig, Connection, SendStream, TransportConfig, VarInt};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc};

use crate::{core::Event, logger, lsp::Position};


pub async fn connect(
    relay_addr: SocketAddr,
    token: &str,
) -> Result<quinn::Connection, Box<dyn std::error::Error>> {
    logger::log(&format!("Connecting to relay at {}...", relay_addr));
    // Setup connection
    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
    let cfg = configure_client(token);
    endpoint.set_default_client_config(cfg);

    let conn = endpoint.connect(relay_addr, "relay")?.await?;
    logger::log("Connected to relay.");
    Ok(conn)
}

pub async fn run_peer(
    session_key: String,
    session_name: Option<String>,
    agent_id: String,
    core_tx: mpsc::Sender<Event>,
    mut net_rx: mpsc::Receiver<NetworkCommand>,
    conn: Connection,
) -> anyhow::Result<()> {
    let (mut send, mut recv) = conn.open_bi().await?;
    let is_host = session_name.is_none();

    // agent_id -> send to peer stream
    let peers: Arc<Mutex<HashMap<String, SendStream>>> = Arc::new(Mutex::new(HashMap::new()));
    let core_tx_ref = core_tx.clone();

    // Loop for accepting incoming peer stream requests in the relay's connection
    let peers_accept = Arc::clone(&peers);
    let conn_accept = conn.clone();
    let agent_id_accept = agent_id.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            logger::log("Waiting for peer to connect");
            let new_peer = conn_accept.accept_bi().await;
            logger::log("Getting new stream req");
            match new_peer {
                Ok((mut send, mut recv)) => {
                    let init_msg = ControlMessage::InitPeer {
                        agent_id: agent_id_accept.clone(),
                        is_host,
                    };
                    send_framed(&mut send, &init_msg)
                        .await
                        .expect("Couldn't send verify message");

                    let msg: ControlMessage = recv_framed(&mut recv)
                        .await
                        .expect("Unable to deserialize incoming message");

                    if let ControlMessage::InitPeer {
                        agent_id: remote_agent_id,
                        is_host: remote_is_host,
                    } = msg
                    {
                        logger::log(&format!(
                            ">> [Network] Connected to peer {} (host: {})",
                            remote_agent_id, remote_is_host
                        ));

                        let mut p = peers_accept.lock().await;
                        p.insert(remote_agent_id.clone(), send);

                        // If we are a peer and we just connected to the host, request sync
                        if !is_host && remote_is_host {
                            logger::log(">> [Network] Requesting initial sync from host...");
                            let sync_req = WireMessage::RequestFullSync;
                            if let Some(host_send) = p.get_mut(&remote_agent_id) {
                                send_framed(host_send, &sync_req)
                                    .await
                                    .expect("Failed to send sync request");
                            }
                        }

                        // Run receiving map for each peer in a separate thread
                        tokio::spawn(recv_loop(recv, core_tx_ref.clone(), is_host));
                    } else {
                        panic!("Invalid setup msg received, expected Init, got {:?}", msg);
                    }
                }
                Err(e) => {
                    logger::log(&format!("!! [Network] accept_bi failed: {}", e));
                    break;
                }
            }
        }
    });

    if let Some(name) = session_name {
        join_session(&mut send, &mut recv, name, session_key).await?;
    } else {
        init_session(&mut send, &mut recv, session_key).await?;
    }

    // Plain outbound traffic
    let peers_out = Arc::clone(&peers);
    tokio::spawn(async move {
        while let Some(cmd) = net_rx.recv().await {
            let wire_msg = match cmd {
                NetworkCommand::BroadcastCursor { uri, position } => {
                    WireMessage::Cursor { uri, position }
                }
                NetworkCommand::BroadcastPatch { uri, patch } => {
                    WireMessage::Patch { uri, data: patch }
                }
                NetworkCommand::SendFullSyncResponse { files } => {
                    WireMessage::FullSyncResponse { files }
                }
            };
            let mut p = peers_out.lock().await;
            broadcast(&mut p, wire_msg).await;
        }
    });

    // Cleanup
    // let _ = core_tx.send(Event::Shutdown).await;
    Ok(())
}

async fn init_session(
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    session_key: String,
) -> anyhow::Result<()> {
    logger::log("Registering new session on relay...");
    let msg = ControlMessage::Register { key: session_key };

    let response = init(send, recv, msg).await?;

    if let ControlMessage::SessionCreated { status, name } = response {
        if status.eq("ok") {
            logger::log(&format!("Created session - name: {}", name));
        } else {
            return Err(anyhow::Error::msg(
                "Unable to init session on relay server!",
            ));
        }
    } else {
        return Err(anyhow::Error::msg(
            "Invalid relay server response, check relay server logs for more information!",
        ));
    }

    Ok(())
}

async fn join_session(
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    session_name: String,
    session_key: String,
) -> anyhow::Result<()> {
    logger::log(&format!("Joining session {}...", session_name));
    let msg = ControlMessage::Join {
        name: session_name,
        key: session_key,
    };

    let response = init(send, recv, msg).await?;

    if let ControlMessage::SessionJoined { status } = response {
        if status.ne("ok") {
            return Err(anyhow::Error::msg(
                "Unable to init session on relay server!",
            ));
        }
        logger::log("Successfully joined session.");
    } else {
        return Err(anyhow::Error::msg(
            "Invalid relay server response, check relay server logs for more information!",
        ));
    }

    Ok(())
}

async fn init(
    send: &mut SendStream,
    recv: &mut quinn::RecvStream,
    msg: ControlMessage,
) -> anyhow::Result<ControlMessage> {
    send.write_all(&serde_json::to_vec(&msg)?).await?;
    send.finish()?;

    let mut buf = vec![0u8; 1024];
    let n = recv.read(&mut buf).await?.unwrap_or(0);

    Ok(serde_json::from_slice::<ControlMessage>(&buf[..n])?)
}

async fn recv_loop(mut recv: quinn::RecvStream, core_tx: mpsc::Sender<Event>, is_host: bool) {
    loop {
        match recv_framed(&mut recv).await {
            Ok(wire_msg) => {
                let core = core_tx.clone();
                let event: Event = match wire_msg {
                    WireMessage::Patch { uri, data } => {
                        logger::log(&format!(">> [Network] Received patch for {}", uri));
                        Event::RemotePatch { uri, patch: data }
                    }
                    WireMessage::Cursor { uri, position } => {
                        let (line, char) = position;
                        Event::RemoteCursorChange {
                            uri,
                            position: Position {
                                line,
                                character: char,
                            },
                        }
                    }
                    WireMessage::RequestFullSync => {
                        if is_host {
                            logger::log(">> [Network] Received sync request from peer.");
                            Event::PeerRequestedSync
                        } else {
                            Event::Ignoring
                        }
                    }
                    WireMessage::FullSyncResponse { files } => {
                        logger::log(&format!(
                            ">> [Network] Received full sync response with {} files.",
                            files.len()
                        ));
                        Event::RemoteFullSync { files }
                    }
                };

                // Local outgoing
                match core.send(event.clone()).await {
                    Ok(_) => logger::log("Sent patch to core!"),
                    Err(e) => logger::log(&format!("Couldn't send patch to remote: {}", e)),
                }
            }
            Err(e) => {
                crate::logger::log(&format!(
                    "!! [Network] Read error (connection closed): {}",
                    e
                ));
                break;
            }
        }
    }
}

async fn broadcast(peers: &mut HashMap<String, quinn::SendStream>, msg: WireMessage) {
    for (agent_id, send) in peers.iter_mut() {
        logger::log(&format!("Broadcasting to peer {}", agent_id));
        if let Err(e) = send_framed(send, &msg).await {
            logger::log(&format!(
                "!! [Network] Broadcast to {} failed: {}",
                agent_id, e
            ));
        }
    }
}

async fn send_framed<T>(send: &mut quinn::SendStream, msg: T) -> Result<()>
where
    T: Sized + Serialize,
{
    let bytes = serde_json::to_vec(&msg)?;
    let len = bytes.len() as u32;

    send.write_all(&len.to_be_bytes()).await?;
    send.write_all(&bytes).await?;
    Ok(())
}

async fn recv_framed<T>(recv: &mut quinn::RecvStream) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 100 * 1024 * 1024 {
        return Err(anyhow::anyhow!("Message too large (100MB limit)"));
    } else if len == 0 {
        return Box::pin(recv_framed(recv)).await;
    }

    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;

    let msg = serde_json::from_slice::<T>(&buf)?;
    Ok(msg)
}

fn make_transport_config() -> TransportConfig {
    let mut transport_config = TransportConfig::default();
    transport_config.max_concurrent_bidi_streams(VarInt::from_u32(100));
    transport_config.max_concurrent_uni_streams(VarInt::from_u32(100));
    transport_config.keep_alive_interval(Some(Duration::from_secs(2)));
    transport_config.max_idle_timeout(Some(VarInt::from_u32(30_000).into()));
    transport_config
}

fn configure_client(token: &str) -> ClientConfig {
    // Use own verifier
    let verifier = crate::crypto::TokenVerifier::new(token);

    let mut crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();

    // ALPN has to match
    crypto.alpn_protocols = vec![b"justsync".to_vec()];

    let mut config = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).unwrap(),
    ));
    config.transport_config(Arc::new(make_transport_config()));
    config
}
