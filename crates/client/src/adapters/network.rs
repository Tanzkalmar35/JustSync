use anyhow::Result;
use quinn::{Connection, RecvStream, SendStream};
use serde::Serialize;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
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

pub struct QuicNetworkAdapter {
    session: SessionCfg, peers: Arc<Mutex<HashMap<String, SendStream>>>,
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
                            logger::log(&format!("!! [Network] accept_peer failed: {}", e));
                        }
                    }
                    Err(e) => {
                        logger::log(&format!("!! [Network] accept_bi failed: {}", e));
                        break;
                    }
                }
            }
        });

        if let SessionRole::Peer { session_name } = &self.session.role {
            self.join_session(&mut send, &mut recv, session_name.to_string())
                .await?;
        } else {
            self.init_session(&mut send, &mut recv).await?;
        }

        // Plain outbound traffic
        tokio::spawn(async move {
            let mut core_recv = self.core_recv.lock().await;
            while let Some(cmd) = core_recv.recv().await {
                let msg = into_external(cmd);
                let mut p = self.peers.lock().await;
                self.broadcast(&mut p, msg).await;
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
        self.send_framed(&mut send, &init_msg)
            .await
            .expect("Couldn't send verify message");

        let msg: ControlMessage = self.recv_framed(&mut recv)
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

            self.peers
                .lock()
                .await
                .insert(remote_agent_id.clone(), send);

            // If we are a peer and we just connected to the host, request sync
            if !self.is_host() && remote_is_host {
                logger::log(">> [Network] Requesting initial sync from host...");
                let sync_req = WireMessage::RequestFullSync;
                if let Some(host_send) = self.peers.lock().await.get_mut(&remote_agent_id) {
                    self.send_framed(host_send, &sync_req)
                        .await
                        .expect("Failed to send sync request");
                }
            }

            let self_recv = Arc::clone(&self);

            // Run receiving map for each peer in a separate thread
            tokio::spawn(async move {
                self_recv.recv_loop(recv)
            });
        } else {
            panic!("Invalid setup msg received, expected Init, got {:?}", msg);
        }

        Ok(())
    }

    async fn recv_loop(self: Arc<Self>, mut recv: quinn::RecvStream) {
        loop {
            match self.recv_framed(&mut recv).await {
                Ok(wire_msg) => {
                    let event = into_internal(wire_msg, self.is_host());
                    match self.core_send.send(event.clone()).await {
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
        &self,
        send: &mut quinn::SendStream,
        recv: &mut quinn::RecvStream,
        session_name: String,
    ) -> anyhow::Result<()> {
        logger::log(&format!("Joining session {}...", session_name));
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

    async fn broadcast(&self, peers: &mut HashMap<String, quinn::SendStream>, msg: WireMessage) {
        for (agent_id, send) in peers.iter_mut() {
            logger::log(&format!("Broadcasting to peer {}", agent_id));
            if let Err(e) = self.send_framed(send, &msg).await {
                logger::log(&format!(
                    "!! [Network] Broadcast to {} failed: {}",
                    agent_id, e
                ));
            }
        }
    }

    async fn send_framed<T>(&self, send: &mut quinn::SendStream, msg: T) -> Result<()>
    where
        T: Sized + Serialize,
    {
        let bytes = serde_json::to_vec(&msg)?;
        let len = bytes.len() as u32;

        send.write_all(&len.to_be_bytes()).await?;
        send.write_all(&bytes).await?;
        Ok(())
    }

    async fn recv_framed<T>(&self, recv: &mut quinn::RecvStream) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;

        if len > 100 * 1024 * 1024 {
            return Err(anyhow::anyhow!("Message too large (100MB limit)"));
        } else if len == 0 {
            return Box::pin(self.recv_framed(recv)).await;
        }

        let mut buf = vec![0u8; len];
        recv.read_exact(&mut buf).await?;

        let msg = serde_json::from_slice::<T>(&buf)?;
        Ok(msg)
    }
}

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

        let conn = match adapter.connect(session.relay_addr, &session.key).await {
            Ok(conn) => conn,
            Err(e) => panic!("{}", e),
        };

        Arc::new(adapter).run_peer(conn).await.expect("Failed to run peer network adapter");
    }
}
