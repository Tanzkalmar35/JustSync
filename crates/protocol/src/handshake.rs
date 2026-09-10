use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ControlMessage {
    Register { key: String },
    SessionCreated { status: String, name: String },
    Join { name: String, key: String },
    SessionJoined { status: String },

    InitPeer { agent_id: String, is_host: bool },
    Spake2MsgA { data: Vec<u8> },
    Spake2MsgB { data: Vec<u8> },
}
