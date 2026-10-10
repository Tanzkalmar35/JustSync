use anyhow::anyhow;
use just_sync_protocol::{PROTOCOL_VERSION, client_relay_handshake::PingRelayIntend};
use quinn::{RecvStream, SendStream};

use crate::network::protocol::{recv_create_session_register, recv_ping, send_pong};

pub(super) struct PeerConnection {}

impl PeerConnection {
    pub(super) fn new() -> Self {
        Self {}
    }

    pub(super) async fn start(
        &self,
        connection: quinn::Connection,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (mut send, mut recv) = connection.accept_bi().await?;

        let intend = recv_ping(&mut recv).await?;
        send_pong(&mut send).await?;

        match intend {
            PingRelayIntend::CreateSession => self.create_session(&mut send, &mut recv).await?,
            PingRelayIntend::JoinSession => self.join_session(&mut send, &mut recv).await?,
        }

        // Keep connection running until the peer closes it
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            if connection.close_reason().is_some() {
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
        let p_version = recv_create_session_register(recv).await?;
        if p_version != PROTOCOL_VERSION {
            return Err(anyhow!("Protocol mismatch during session registration"))
        }

        // Generate challenge nonce & send to peer
        
        // Receive createsession command with the signed nonce
        
        // create session & respond to peer

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
