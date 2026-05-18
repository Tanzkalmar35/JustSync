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

use crate::{
    internal::{
        self,
        core::Event,
        network::{
            ControlMessage, NetworkAdapter, NetworkCommand, SessionCfg, SessionRole, WireMessage,
            configure_client, into_external, into_internal,
        },
    },
    logger,
};

struct PeerContext {
    sender: quinn::SendStream,
    secret: ChaCha20Poly1305,
}

pub struct QuicNetworkAdapter {
    session: SessionCfg,
    peers: Arc<Mutex<HashMap<String, PeerContext>>>,
    core_send: mpsc::Sender<Event>,
    core_recv: Mutex<mpsc::Receiver<NetworkCommand>>,
}

impl QuicNetworkAdapter {
    fn is_host(&self) -> bool {
        matches!(self.session.role, SessionRole::Host {})
    }

    async fn connect(
        &self,
        relay_addr: SocketAddr,
    ) -> Result<quinn::Connection, Box<dyn std::error::Error>> {
        logger::log(&format!("Connecting to relay at {relay_addr}!"));
        // Setup connection
        let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
        let cfg = configure_client();
        endpoint.set_default_client_config(cfg);

        let conn = endpoint.connect(relay_addr, "relay")?.await?;
        logger::log("Connected to relay.");
        Ok(conn)
    }

    async fn run_peer(self: Arc<Self>, conn: Connection) -> anyhow::Result<()> {
        let (mut send, mut recv) = conn.open_bi().await?;

        let self_accept = Arc::clone(&self);

        // Loop for accepting incoming peer stream requests in the relay's connection
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let new_peer = conn.accept_bi().await;
                match new_peer {
                    Ok((send, recv)) => {
                        let self_accept = Arc::clone(&self_accept);
                        if let Err(e) = self_accept.accept_peer(send, recv).await {
                            logger::log(&format!("!! [Network] accept_peer failed: {e}"));
                        }
                    }
                    Err(e) => {
                        logger::log(&format!("!! [Network] accept_bi failed: {e}"));
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

        // Plain outbound traffic
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
            logger::log(&format!(
                ">> [Network] Connected to peer {remote_agent_id} (host: {remote_is_host})",
            ));

            let peer = match self
                .setup_peer_e2ee(&remote_agent_id, send, &mut recv)
                .await
            {
                Ok(ctx) => ctx,
                Err(e) => {
                    logger::log(&format!("Failed to set up E2EE: {}", e));
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
                logger::log("Requesting initial sync from host...");
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
                self_recv.recv_loop(recv, &peer_secret).await;
            });
        } else {
            panic!("Invalid setup msg received, expected Init, got {msg:?}");
        }

        Ok(())
    }

    /// Initializes E2EE between two peers.
    ///
    /// # Arguments
    ///
    /// * `remote_agent_id` - The agent_id of the remote peer
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
                let (state, msg_a) = Spake2::<Ed25519Group>::start_a(
                    &Password::new(self.session.key.clone()),
                    &Identity::new(self.session.agent_id.as_bytes()),
                    &Identity::new(remote_agent_id.as_bytes()),
                );

                let msg = WireMessage::Spake2MsgA { data: msg_a };

                self.send_framed(&mut send, msg, None)
                    .await
                    .map_err(|e| e.to_string())?;

                if let WireMessage::Spake2MsgB { data } =
                    self.recv_framed(recv, None).await.map_err(|e| e.to_string())?
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
                let mut msg_a: Vec<u8> = vec![];
                if let WireMessage::Spake2MsgA { data } =
                    self.recv_framed(recv, None).await.map_err(|e| e.to_string())?
                {
                    msg_a = data;
                }

                let (state, msg_b) = Spake2::<Ed25519Group>::start_b(
                    &Password::new(self.session.key.clone()),
                    &Identity::new(remote_agent_id.as_bytes()),
                    &Identity::new(self.session.agent_id.as_bytes()),
                );

                let msg = WireMessage::Spake2MsgA { data: msg_b };

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

    async fn recv_loop(self: Arc<Self>, mut recv: quinn::RecvStream, cipher: &ChaCha20Poly1305) {
        loop {
            match self.recv_framed(&mut recv, Some(cipher)).await {
                Ok(wire_msg) => {
                    let event = into_internal(wire_msg, self.is_host());
                    match self.core_send.send(event.clone()).await {
                        Ok(()) => logger::log("Sent patch to core!"),
                        Err(e) => logger::log(&format!("Couldn't send patch to remote: {e}")),
                    }
                }
                Err(e) => {
                    crate::logger::log(&format!(
                        "!! [Network] Read error (connection closed): {e}"
                    ));
                    break;
                }
            }
        }
    }

    async fn init_session(
        &self,
        send: &mut quinn::SendStream,
        recv: &mut quinn::RecvStream,
    ) -> anyhow::Result<()> {
        logger::log("Registering new session on relay...");
        let msg = ControlMessage::Register {
            key: self.session.key.clone(),
        };

        let response = self.init(send, recv, msg).await?;

        if let ControlMessage::SessionCreated { status, name } = response {
            if status.eq("ok") {
                logger::log(&format!("Created session - name: {name}"));
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

    async fn join_session(
        &self,
        send: &mut quinn::SendStream,
        recv: &mut quinn::RecvStream,
        session_name: String,
    ) -> anyhow::Result<()> {
        logger::log(&format!("Joining session {session_name}!"));
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
            logger::log("Successfully joined session.");
        } else {
            return Err(anyhow::Error::msg(
                "Invalid relay server response, check relay server logs for more information!",
            ));
        }

        Ok(())
    }

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

    async fn broadcast(&self, msg: WireMessage) {
        for (agent_id, ctx) in self.peers.lock().await.iter_mut() {
            logger::log(&format!("Broadcasting to peer {agent_id}"));
            if let Err(e) = self
                .send_framed(&mut ctx.sender, &msg, Some(&ctx.secret))
                .await
            {
                logger::log(&format!("!! [Network] Broadcast to {agent_id} failed: {e}"));
            }
        }
    }

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

        if let Some(c) = cipher {
            let nonce = ChaCha20Poly1305::generate_nonce(OsRng);
            match &c.encrypt(&nonce, bytes.as_ref()) {
                Ok(blob) => bytes = blob.to_vec(),
                Err(e) => {
                    logger::log(&format!("An error occured while encrypting msg: {e:?}"));
                    // TODO: Return err
                }
            }
        }

        let len = u32::try_from(bytes.len())?;

        send.write_all(&len.to_be_bytes()).await?;
        send.write_all(&bytes).await?;
        Ok(())
    }

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

        if len > 100 * 1024 * 1024 {
            return Err(anyhow::anyhow!("Message too large (100MB limit)"));
        } else if len == 0 {
            return Box::pin(self.recv_framed(recv, cipher)).await;
        }

        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf).await?;
        let mut msg = serde_json::from_slice::<T>(&buf)?;

        if let Some(c) = cipher {
            let nonce = ChaCha20Poly1305::generate_nonce(OsRng);
            match &c.decrypt(&nonce, buf.as_ref()) {
                Ok(text) => msg = serde_json::from_slice::<T>(&text)?,
                Err(e) => {
                    logger::log(&format!(
                        "An error occured while decrypting incoming msg: {e:?}"
                    ));
                    // TODO: Return err
                }
            }
        }

        Ok(msg)
    }
}

#[async_trait::async_trait]
impl NetworkAdapter for QuicNetworkAdapter {
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
