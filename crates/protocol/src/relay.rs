use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub enum ControlMessage {
    Register { key: String },

    SessionCreated { status: String, name: String },

    Join { name: String, key: String },

    SessionJoined { status: String },
}
