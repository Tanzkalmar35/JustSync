use std::{sync::Arc};

use anyhow::anyhow; use just_sync_protocol::{PROTOCOL_VERSION, client_relay_handshake::PingRelayIntend};
use quinn::{RecvStream, SendStream};
use tokio::sync::{Mutex};

use crate::{core::CoreActor, network::protocol::{recv_create_session_cmd, recv_create_session_register, recv_ping, send_challenge, send_pong, send_session_created}};

pub(super) struct PeerConnection {
    core: Arc<Mutex<CoreActor>>,
    conn: quinn::Connection,
}

impl PeerConnection {
    pub(super) fn new(core: Arc<Mutex<CoreActor>>, conn: quinn::Connection) -> Self {
        Self { core, conn }
    }

    pub(super) async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        let (mut send, mut recv) = self.conn.accept_bi().await?;

        let intend = recv_ping(&mut recv).await?;
        send_pong(&mut send).await?;

        match intend {
            PingRelayIntend::CreateSession => self.create_session(&mut send, &mut recv).await?,
            PingRelayIntend::JoinSession => self.join_session(&mut send, &mut recv).await?,
        }

        // Keep connection running until the peer closes it
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            if self.conn.close_reason().is_some() {
                break;
            }
        }

        Ok(())
    }

    async fn create_session(
        &self,
        send: &mut SendStream,
        recv: &mut RecvStream,
    ) -> anyhow::Result<()> {
        // Receive register command and verify protocol matches
        let p_version = recv_create_session_register(recv).await?;
        if p_version != PROTOCOL_VERSION {
            return Err(anyhow!("Protocol mismatch during session registration"));
        }

        // Generate a nonce and send it to the peer to sign
        let nonce = self.core.lock().await.generate_nonce()?;
        send_challenge(send, nonce).await?;

        // Receive createsession command with the signed nonce
        let (identity, sig) = recv_create_session_cmd(recv).await?;
        // TODO: Validate, do whatever

        // Create session & respond to peer
        let (session_id, capabilities) = self.core.lock().await.create_session()?;
        send_session_created(send, session_id, capabilities).await?;

        Ok(())
    }

    async fn join_session(
        &self,
        send: &mut SendStream,
        recv: &mut RecvStream,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
