use anyhow::Result;
use serde::{Deserialize, Serialize};

pub(crate) trait AsyncSender {
    async fn send<T: serde::Serialize>(&mut self, message: &T) -> anyhow::Result<()>;
}

pub(crate) trait AsyncReceiver {
    async fn recv<T: serde::de::DeserializeOwned>(&mut self) -> Result<Option<T>>;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) enum SetupMessage {
    /// Peer -> Host
    Id { agent_id: String },

    /// Host -> Peer
    FullSync { files: Vec<(String, Vec<u8>)> },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) enum WireEvent {
    Patch {
        uri: String,
        data: Vec<u8>,
    },

    Cursor {
        uri: String,
        position: (usize, usize),
    },
}
