use anyhow::Result;
use chacha20poly1305::{
    AeadCore, ChaCha20Poly1305, Key, KeyInit,
    aead::{Aead, OsRng},
};
use quinn::{Connection, RecvStream, SendStream};
use serde::Serialize;
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::{cmp::Ordering, collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error, info};

use crate::internal::{
    self,
    core::Event,
    network::{
        ControlMessage, NetworkAdapter, NetworkCommand, SessionCfg, SessionRole, WireMessage,
        configure_client, into_external, into_internal,
    },
};

struct PeerContext {
    sender: quinn::SendStream,
    secret: ChaCha20Poly1305,
}

pub struct QuicNetworkAdapter {
    session: SessionCfg,
    peers: Arc<Mutex<HashMap<String, PeerContext>>>, // agent_id -> peer
    core_send: mpsc::Sender<Event>,
    core_recv: Mutex<mpsc::Receiver<NetworkCommand>>,
}

impl QuicNetworkAdapter {
    /// Checks if the network adapter is connected to the relay server as hosting peer.
    fn is_host(&self) -> bool {
        matches!(self.session.role, SessionRole::Host {})
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

        if let SessionRole::Peer { session_name } = &self.session.role {
            self.join_session(&mut send, &mut recv, session_name.clone())
                .await?;
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
        let init_msg = ControlMessage::InitPeer {
            agent_id: self.session.agent_id.clone(),
            is_host: self.is_host(),
        };
        self.send_framed(&mut send, &init_msg, None)
            .await
            .expect("Couldn't send verify message");

        let msg: ControlMessage = self
            .recv_framed(&mut recv, None)
            .await
            .expect("Unable to deserialize incoming message");

        if let ControlMessage::InitPeer {
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
                    self.send_framed(&mut host_ctx.sender, &sync_req, Some(&host_ctx.secret))
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

                let msg = ControlMessage::Spake2MsgA { data: msg_a };

                self.send_framed(&mut send, msg, None)
                    .await
                    .map_err(|e| e.to_string())?;

                if let ControlMessage::Spake2MsgB { data } = self
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
                if let ControlMessage::Spake2MsgA { data } = self
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

                let msg = ControlMessage::Spake2MsgB { data: msg_b };

                self.send_framed(&mut send, msg, None)
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
            match self.recv_framed(&mut recv, Some(cipher)).await {
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

    /// Initializes a new session on hte remote relay server.
    ///
    /// # Arguments
    ///
    /// * `send` - The localhost -> relay server stream.
    /// * `recv` - The relay server -> localhost stream.
    ///
    /// # Errors
    ///
    /// * If sending the initialization message fails.
    async fn init_session(
        &self,
        send: &mut quinn::SendStream,
        recv: &mut quinn::RecvStream,
    ) -> anyhow::Result<()> {
        debug!("[Net] Registering new session on relay");
        let msg = ControlMessage::Register {
            key: self.session.key.clone(),
        };

        let response = self.init(send, recv, msg).await?;

        if let ControlMessage::SessionCreated { status, name } = response {
            if status.eq("ok") {
                info!(
                    "[Net] Registered new session on relay server: name: {}",
                    name
                );
                let _ = self.core_send.send(Event::SessionRegistered { name }).await;
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
        session_name: String,
    ) -> anyhow::Result<()> {
        debug!("[Net] Attempting to join session {}", session_name);
        let msg = ControlMessage::Join {
            name: session_name,
            key: self.session.key.clone(),
        };

        let response = self.init(send, recv, msg).await?;

        if let ControlMessage::SessionJoined { status } = response {
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

    /// Initializes this peer on the relay server.
    ///
    /// # Arguments
    ///
    /// * `send` - The localhost -> relay server stream.
    /// * `recv` - The relay server -> localhost stream.
    /// * `msg` - The message to init with. In practise should either be `ControlMessage::Join` or
    ///   `ControlMessage::Init`
    ///
    /// # Errors
    ///
    /// * If serializing the given message fails.
    /// * If writing the given message to the outgoing stream fails.
    /// * If closing the init stream fails.
    /// * If reading a message from the incoming stream fails.
    /// * If the incoming message is not a valid `ControlMessage`.
    ///
    /// # Returns
    ///
    /// The relay server response of type `ControlMessage`.
    async fn init(
        &self,
        send: &mut SendStream,
        recv: &mut RecvStream,
        msg: ControlMessage,
    ) -> anyhow::Result<ControlMessage> {
        send.write_all(&serde_json::to_vec(&msg)?).await?;
        send.finish()?;

        let mut buf = vec![0u8; 1024];
        let n = recv.read(&mut buf).await?.unwrap_or(0);

        Ok(serde_json::from_slice::<ControlMessage>(&buf[..n])?)
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
                .send_framed(&mut ctx.sender, &msg, Some(&ctx.secret))
                .await
            {
                error!("[Net] Broadcast to {} failed: {}", agent_id, e);
            }
        }
    }

    /// Sends a given message in valid format, end to end encrypted.
    ///
    /// # Arguments
    ///
    /// * `send` - The localhost -> remote peer stream.
    /// * `msg` - The message to send to the remote peer.
    /// * `cipher` - The cipher used for E2EE.
    ///
    /// # Errors
    ///
    /// * If serializing the given message fails.
    /// * If encryption of the message fails.
    /// * If writing the message and the header to the output stream fails.
    async fn send_framed<T>(
        &self,
        send: &mut quinn::SendStream,
        msg: T,
        cipher: Option<&ChaCha20Poly1305>,
    ) -> Result<()>
    where
        T: Sized + Serialize,
    {
        let mut bytes = serde_json::to_vec(&msg)?;

        // Encrypt message if cipher is provided, don't if not
        if let Some(c) = cipher {
            let nonce = ChaCha20Poly1305::generate_nonce(OsRng);
            match c.encrypt(&nonce, bytes.as_ref()) {
                Ok(blob) => {
                    // Prepend the 12-byte nonce to the ciphertext
                    let mut payload = nonce.to_vec();
                    payload.extend_from_slice(&blob);
                    bytes = payload;
                }
                Err(e) => {
                    error!(
                        "[Net] An error occured while encrypting outgoing message: {:?}",
                        e
                    );
                    return Err(anyhow::anyhow!("Encryption failed"));
                }
            }
        }

        let len = u32::try_from(bytes.len())?;

        send.write_all(&len.to_be_bytes()).await?;
        send.write_all(&bytes).await?;
        Ok(())
    }

    /// Recieves, decrypts and forwards a formatted message from a peer to the core.
    ///
    /// # Arguments
    ///
    /// * `recv` - The remote peer -> localhost stream.
    /// * `cipher` - The cipher used for decryption of the incoming message.
    ///
    /// # Errors
    ///
    /// * If reading the message from the `RecvStream` fails
    /// * If the incoming message is >100MB.
    /// * If the cipher is provided, if the incoming message does not contain the nonce.
    /// * If the cipher is provided, if decrypting the incoming message fails.
    ///
    /// # Returns
    ///
    /// The incoming message, validated, decrypted, ready to use.
    async fn recv_framed<T>(
        &self,
        recv: &mut quinn::RecvStream,
        cipher: Option<&ChaCha20Poly1305>,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;

        // Validate msg size
        if len > 100 * 1024 * 1024 {
            return Err(anyhow::anyhow!("Message too large (100MB limit)"));
        } else if len == 0 {
            return Box::pin(self.recv_framed(recv, cipher)).await;
        }

        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf).await?;

        // If a cipher is provided, we expect encrypted traffic. Otherwise not.
        if let Some(c) = cipher {
            // ChaCha20Poly1305 nonce is exactly 12 bytes
            if buf.len() < 12 {
                return Err(anyhow::anyhow!(
                    "Encrypted payload too small to contain nonce"
                ));
            }

            // Split the buffer into nonce and ciphertext
            let nonce = chacha20poly1305::Nonce::clone_from_slice(&buf[..12]);
            match c.decrypt(&nonce, &buf[12..]) {
                Ok(text) => Ok(serde_json::from_slice::<T>(&text)?),
                Err(e) => {
                    error!(
                        "[Net] An error occured decrypting incoming message: {:?}",
                        e
                    );
                    Err(anyhow::anyhow!("Decryption failed"))
                }
            }
        } else {
            // Unencrypted traffic (e.g., initial SPAKE2 handshake)
            Ok(serde_json::from_slice::<T>(&buf)?)
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
    async fn connect_and_run(
        session: internal::network::SessionCfg,
        core_tx: mpsc::Sender<Event>,
        net_rx: mpsc::Receiver<crate::internal::network::NetworkCommand>,
    ) {
        let adapter = Self {
            session: session.clone(),
            peers: Arc::new(Mutex::new(HashMap::new())),
            core_send: core_tx,
            core_recv: Mutex::new(net_rx),
        };

        let conn = match adapter.connect(session.relay_addr).await {
            Ok(conn) => conn,
            Err(e) => panic!("{}", e),
        };

        Arc::new(adapter)
            .run_peer(conn)
            .await
            .expect("Failed to run peer network adapter");
    }
}
