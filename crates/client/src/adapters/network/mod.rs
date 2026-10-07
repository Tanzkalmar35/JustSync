use anyhow::Result;
use chacha20poly1305::{
    AeadCore, ChaCha20Poly1305, Key, KeyInit,
    aead::{Aead, OsRng},
};
use hex::{ToHex, encode};
use just_sync_protocol::{
    PROTOCOL_VERSION,
    client_relay_handshake::{Capabilities, CreateSessionMsg},
};
use quinn::{Connection, RecvStream, SendStream};
use ring::signature::KeyPair;
use serde::Serialize;
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::{cmp::Ordering, collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error, info};

use crate::{
    adapters::network::protocol_impl::ProtocolImpl,
    internal::{
        self,
        core::Event,
        network::{
            NetworkAdapter, NetworkCommand, SessionCfg, configure_client, into_external,
            into_internal,
        },
    },
};

mod protocol_impl;

struct PeerContext {
    sender: quinn::SendStream,
    secret: ChaCha20Poly1305,
}

pub struct QuicNetworkAdapter {
    /// Information about the current session.
    session: SessionCfg,
    /// Implementation of the protocol - bare-bones protocol helper.
    protocol_impl: protocol_impl::ProtocolImpl,

    /// Connected peers.
    peers: Arc<Mutex<HashMap<String, PeerContext>>>, // agent_id -> peer

    /// Send events coming from remote to the core.
    core_send: mpsc::Sender<Event>,
    /// Receive events to send out to remote from core.
    core_recv: Mutex<mpsc::Receiver<NetworkCommand>>,
}

impl QuicNetworkAdapter {
    fn new(
        session: SessionCfg,
        core_send: mpsc::Sender<Event>,
        core_recv: Mutex<mpsc::Receiver<NetworkCommand>>,
    ) -> Self {
        Self {
            session,
            protocol_impl: ProtocolImpl {},
            peers: Arc::new(Mutex::new(HashMap::new())),
            core_send,
            core_recv,
        }
    }

    /// Checks if the network adapter is connected to the relay server as hosting peer.
    fn is_host(&self) -> bool {
        return self.session.invitation.is_none();
    }

    /// Establishes a connection between localhost and a relay server running at the given url.
    ///
    /// # Arguments
    ///
    /// * `relay_addr` - The url pointing towards the remote relay server.
    ///
    /// # Errors
    ///
    /// * If the endpoint could not be initialized.
    /// * If the connection can't be established.
    ///
    /// # Returns
    ///
    /// The connection on success.
    async fn connect(
        &self,
        relay_addr: SocketAddr,
    ) -> Result<quinn::Connection, Box<dyn std::error::Error>> {
        info!("[Net] Connecting to relay at {}", relay_addr);

        let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
        let cfg = configure_client();
        endpoint.set_default_client_config(cfg);

        let conn = endpoint.connect(relay_addr, "relay")?.await?;
        info!("[Net] Connected to relay server.");
        Ok(conn)
    }

    /// Runs the event loop for the peer.
    ///
    /// This function
    ///
    /// * starts a new thread for accepting new peer connections.
    /// * joins or initializes a session, depending on the session role.
    /// * starts a new thread which listens for broadcast commands from core, and handles
    ///   conversion and broadcasting for these.
    ///
    /// # Arguments
    ///
    /// * `conn` - The established connection to the relay server.
    ///
    /// # Errors
    ///
    /// * If no bidirectional stream can be opened to the relay server.
    /// * If joining or creating a session on the relay server fails.
    async fn run_peer(self: Arc<Self>, conn: Connection) -> anyhow::Result<()> {
        let (mut send, mut recv) = conn.open_bi().await?;

        let self_accept = Arc::clone(&self);

        // Spawning connection acceptance & setup thread
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let new_peer = conn.accept_bi().await;
                match new_peer {
                    Ok((send, recv)) => {
                        let self_accept = Arc::clone(&self_accept);
                        if let Err(e) = self_accept.accept_peer(send, recv).await {
                            error!("[Net] An error occured while accepting new peer: {}", e);
                        }
                    }
                    Err(e) => {
                        error!(
                            "[Net] Establishing a new incoming stream from relay failed: {}",
                            e
                        );
                        break;
                    }
                }
            }
        });

        if let Some(invitation) = &self.session.invitation {
            self.join_session(&mut send, &mut recv, invitation).await?;
        } else {
            self.init_session(&mut send, &mut recv).await?;
        }

        // Broadcast messages from core to all connected peers
        tokio::spawn(async move {
            let mut core_recv = self.core_recv.lock().await;
            while let Some(cmd) = core_recv.recv().await {
                let msg = into_external(cmd);
                self.broadcast(msg).await;
            }
        });

        // Cleanup
        // let _ = core_tx.send(Event::Shutdown).await;
        Ok(())
    }

    /// Manages setup of new peers.
    ///
    /// The setup order goes as follows:
    ///
    /// 1. Both peers send a identification message.
    /// 2. Both peers wait for the incoming identification message.
    /// 3. E2EE setup runs.
    /// 4. If one of the peers is the hosting peer, the other peer requests a full project sync.
    /// 5. If 4, then the host sends the whole project state over the wire.
    /// 6. Both peers set up a listener for messages from the other peer.
    ///
    /// # Arguments
    ///
    /// * `send` - The localhost -> remote peer stream.
    /// * `recv` - The remote peer -> localhost stream.
    ///
    /// # Errors
    ///
    /// * If sending a message to the new peer fails.
    /// * If receiving a message from a new peer fails.
    /// * If the setup order is not followed by the remote peer.
    /// * If setting up E2EE with the new peer fails.
    async fn accept_peer(
        self: Arc<Self>,
        mut send: quinn::SendStream,
        mut recv: quinn::RecvStream,
    ) -> anyhow::Result<()> {
        let init_msg = client_relay_handshake::ControlMessage::InitPeer {
            agent_id: self.session.agent_id.clone(),
            is_host: self.is_host(),
        };
        self.protocol_impl
            .send_framed(&mut send, &init_msg, None)
            .await
            .expect("Couldn't send verify message");

        let msg: client_relay_handshake::ControlMessage = self
            .protocol_impl
            .recv_framed(&mut recv, None)
            .await
            .expect("Unable to deserialize incoming message");

        if let client_relay_handshake::ControlMessage::InitPeer {
            agent_id: remote_agent_id,
            is_host: remote_is_host,
        } = msg
        {
            info!("[Net] Connected to peer {}", remote_agent_id);

            let peer = match self
                .setup_peer_e2ee(&remote_agent_id, send, &mut recv)
                .await
            {
                Ok(ctx) => ctx,
                Err(e) => {
                    error!("[Net] Failed to initialize E2EE with new peer: {}", e);
                    panic!("{e}");
                }
            };

            let peer_secret = peer.secret.clone();

            self.peers
                .lock()
                .await
                .insert(remote_agent_id.clone(), peer);

            // If we are a peer and we just connected to the host, request sync
            if !self.is_host() && remote_is_host {
                info!("[Net] Requesting initial sync from host");
                let sync_req = WireMessage::RequestFullSync;
                if let Some(host_ctx) = self.peers.lock().await.get_mut(&remote_agent_id) {
                    self.protocol_impl
                        .send_framed(&mut host_ctx.sender, &sync_req, Some(&host_ctx.secret))
                        .await
                        .expect("Failed to send sync request");
                }
            }

            let self_recv = Arc::clone(&self);

            // Run receiving map for each peer in a separate thread
            tokio::spawn(async move {
                self_recv
                    .recv_loop(recv, &peer_secret, &remote_agent_id)
                    .await;
            });
        } else {
            panic!("Invalid setup msg received, expected Init, got {msg:?}");
        }

        Ok(())
    }

    /// Initializes E2EE between two peers.
    ///
    /// The order of setup is determined by the agent_id's of both peers, so it's a deterministic
    /// order that can be computed by both peers with the same result in a stable way, since the
    /// likelyhood of two UUIDv4's matching is ~1 in 2.71 x 10^18, so very unlikely.
    ///
    /// # Arguments
    ///
    /// * `remote_agent_id` - The agent id of the remote peer
    /// * `send` - Outgoing channel to the peer
    /// * `recv` - Incoming channel from the peer
    ///
    /// # Panics
    ///
    /// * If the agent id's of the 2 peers are exactly equal, which is very highly unlikely, and if
    ///   that happens, then this function failing is not the only problem we have
    ///
    /// # Errors
    ///
    /// * If Sending to the peer fails
    /// * If recveiving from the peer fails
    /// * If the process fails unexpectedly
    async fn setup_peer_e2ee(
        &self,
        remote_agent_id: &str,
        mut send: quinn::SendStream,
        recv: &mut quinn::RecvStream,
    ) -> Result<PeerContext, String> {
        match self.session.agent_id.cmp(&remote_agent_id.to_string()) {
            Ordering::Less => {
                // Initiate setup
                let (state, msg_a) = Spake2::<Ed25519Group>::start_a(
                    &Password::new(self.session.key.clone()),
                    &Identity::new(self.session.agent_id.as_bytes()),
                    &Identity::new(remote_agent_id.as_bytes()),
                );

                let msg = client_relay_handshake::ControlMessage::Spake2MsgA { data: msg_a };

                self.protocol_impl
                    .send_framed(&mut send, msg, None)
                    .await
                    .map_err(|e| e.to_string())?;

                if let client_relay_handshake::ControlMessage::Spake2MsgB { data } = self
                    .protocol_impl
                    .recv_framed(recv, None)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    let secret = state.finish(&data).map_err(|e| e.to_string())?;
                    let key = Key::from_slice(&secret);
                    let cipher = ChaCha20Poly1305::new(key);

                    return Ok(PeerContext {
                        sender: send,
                        secret: cipher,
                    });
                }
            }
            Ordering::Greater => {
                // Wait for setup initiation
                let mut msg_a: Vec<u8> = vec![];
                if let client_relay_handshake::ControlMessage::Spake2MsgA { data } = self
                    .protocol_impl
                    .recv_framed(recv, None)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    msg_a = data;
                }

                let (state, msg_b) = Spake2::<Ed25519Group>::start_b(
                    &Password::new(self.session.key.clone()),
                    &Identity::new(remote_agent_id.as_bytes()),
                    &Identity::new(self.session.agent_id.as_bytes()),
                );

                let msg = client_relay_handshake::ControlMessage::Spake2MsgB { data: msg_b };

                self.protocol_impl
                    .send_framed(&mut send, msg, None)
                    .await
                    .map_err(|e| e.to_string())?;

                let secret = state.finish(&msg_a).map_err(|e| e.to_string())?;
                let key = Key::from_slice(&secret);
                let cipher = ChaCha20Poly1305::new(key);

                return Ok(PeerContext {
                    sender: send,
                    secret: cipher,
                });
            }
            Ordering::Equal => {
                panic!("Woah, we got matching agent_id's (uuids) - That's a first for me...")
            }
        }

        Err(String::from("E2EE setup process failed!"))
    }

    /// Listens for incoming messages from the given `RecvStream` and forwards them to the core.
    ///
    /// # Arguments
    ///
    /// * `cipher` - The cipher used for E2EE handling.
    /// * `agent_id` - The agent_id of the sending side.
    async fn recv_loop(
        self: Arc<Self>,
        mut recv: quinn::RecvStream,
        cipher: &ChaCha20Poly1305,
        agent_id: &str,
    ) {
        loop {
            match self
                .protocol_impl
                .recv_framed(&mut recv, Some(cipher))
                .await
            {
                Ok(wire_msg) => {
                    let event = into_internal(wire_msg, agent_id, self.is_host());
                    match self.core_send.send(event.clone()).await {
                        Ok(()) => debug!("[Net] Populated patch to editor"),
                        Err(e) => error!(
                            "[Net] An error occured populating incoming event to core: {}",
                            e
                        ),
                    }
                }
                Err(e) => {
                    error!("[Net] An error occured reading incoming message: {}", e);
                    break;
                }
            }
        }
    }

    /// Initializes a new session on the remote relay server.
    ///
    /// # Arguments
    ///
    /// * `send` - The localhost -> relay server stream.
    /// * `recv` - The relay server -> localhost stream.
    ///
    /// # Errors
    ///
    /// * If sending the initialization message fails.
    ///
    /// # Returns
    ///
    /// * The session id + capabilities on success.
    async fn init_session(
        &self,
        send: &mut quinn::SendStream,
        recv: &mut quinn::RecvStream,
    ) -> anyhow::Result<(String, Capabilities)> {
        debug!("[Net] Registering as new peer on relay");
        let response = self.protocol_impl.send_register_cmd(send, recv).await?;

        // Expect challenge -> read nonce to sign
        let nonce = if let CreateSessionMsg::Challenge(nonce) = response {
            nonce
        } else {
            return Err(anyhow::Error::msg(
                "Invalid relay server response, check relay server logs for more information!",
            ));
        };

        // Sign nonce using keypair and send create session signal
        let sig = self.session.keypair.sign(nonce);
        let msg = CreateSessionMsg::CreateSession {
            identity_pk: self.session.keypair.public_key().encode_hex(),
            sig_id: sig.encode_hex(),
        };

        let create_session_response = self.protocol_impl.send_create_session_cmd(
            send,
            recv,
            self.session.keypair.public_key().encode_hex(),
            sig.encode_hex(),
        );

        // Return session id and capabilities on success - expecting successful session creation
        if let CreateSessionMsg::SessionCreated {
            session_id,
            server_time,
            capabilities,
        } = create_session_response
        {
            info!("New session created at: {server_time}");
            s_id = str::from_utf8(&session_id)
                .expect("Could not decode session id")
                .to_string();
            return Ok((s_id, capabilities));
        } else {
            return Err(anyhow::Error::msg(
                "Invalid relay server response, check relay server logs for more information!",
            ));
        }
    }

    /// Joins an existing session on the relay server.
    ///
    /// # Arguments
    ///
    /// * `send` - The localhost -> relay server stream.
    /// * `recv` - The relay server -> localhost stream.
    /// * `session_name` - The name of the existing session to join.
    ///
    /// # Errors
    ///
    /// * If sending the init message fails.
    /// * If the join request returns non-ok status (although SessionJoined response).
    /// * If the response to the join request was not expected.
    async fn join_session(
        &self,
        send: &mut quinn::SendStream,
        recv: &mut quinn::RecvStream,
        invitation: &str,
    ) -> anyhow::Result<()> {
        debug!("[Net] Attempting to join session {}", session_name);
        let msg = relay::ControlMessage::Join {
            name: session_name,
            key: self.session.key.clone(),
        };

        let response = self.register_on_relay(send, recv).await?;

        if let relay::ControlMessage::SessionJoined { status } = response {
            if status.ne("ok") {
                return Err(anyhow::Error::msg(
                    "Unable to init session on relay server!",
                ));
            }
            info!("[Net] Successfully joined session");
        } else {
            return Err(anyhow::Error::msg(
                "Invalid relay server response, check relay server logs for more information!",
            ));
        }

        Ok(())
    }

    /// Broadcasts a given message to all connected peers.
    ///
    /// # Arguments
    ///
    /// * `msg` - The message to broadcast.
    async fn broadcast(&self, msg: WireMessage) {
        for (agent_id, ctx) in self.peers.lock().await.iter_mut() {
            debug!("[Net] Broadcasting patch to {}", agent_id);
            if let Err(e) = self
                .protocol_impl
                .send_framed(&mut ctx.sender, &msg, Some(&ctx.secret))
                .await
            {
                error!("[Net] Broadcast to {} failed: {}", agent_id, e);
            }
        }
    }
}

#[async_trait::async_trait]
impl NetworkAdapter for QuicNetworkAdapter {
    /// Connects localhost to a session on a relay server, and runs the main network loop:
    ///
    /// * Broadcasting messages from core.
    /// * Forwarding messages to core.
    ///
    /// # Arguments
    ///
    /// * `core_tx` - Net -> Core stream.
    /// * `net_rx` - Core -> Net stream.
    ///
    /// # Errors
    ///
    /// * If no connection to the relay serer could be obtained.
    /// * If the peer event loop fails.
    async fn connect_and_run(
        session: internal::network::SessionCfg,
        core_tx: mpsc::Sender<Event>,
        net_rx: mpsc::Receiver<crate::internal::network::NetworkCommand>,
    ) -> anyhow::Result<()> {
        let adapter = Self::new(session.clone(), core_send, core_recv);

        let socket_addr = session.relay_addr.resolve().await?;
        info!(
            "Connecting to relay at {} (IP: {})",
            session.relay_addr.host, socket_addr
        );

        let conn = adapter
            .connect(socket_addr)
            .await
            .map_err(|e| anyhow::anyhow!("Unable to obtain connection to relay server: {}", e))?;

        Arc::new(adapter)
            .run_peer(conn)
            .await
            .map_err(|e| anyhow::anyhow!("Unable to run peer event loop: {}", e))?;

        Ok(())
    }
}
