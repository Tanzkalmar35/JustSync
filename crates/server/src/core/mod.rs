use just_sync_protocol::client_relay_handshake::Capabilities;

pub(crate) struct CoreActor {}

impl CoreActor {
    pub(crate) fn generate_nonce(&self) -> anyhow::Result<[u8; 32]> {
        Ok(())
    }

    pub(crate) fn create_session(&self) -> anyhow::Result<([u8; 16], Capabilities)> {
        Ok(())
    }
}
