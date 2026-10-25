use crate::PROTOCOL_VERSION;
use crate::error::ErrorCode;
use serde::{Serialize, de::DeserializeOwned};

pub const MAX_CTRL: usize = 2 * 1024;
pub const MAX_HS: usize = 4 * 1024;
pub const MAX_APP: usize = 1 << 20;

const PREFIX: usize = 4; // len
const SUBHEADER: usize = 3; // v + kind

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Relay = 0,
    Handshake = 1,
    App = 2,
}

impl Kind {
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Relay,
            1 => Self::Handshake,
            2 => Self::App,
            _ => return None,
        })
    }
}

pub const fn max_len(kind: Kind) -> usize {
    match kind {
        Kind::Relay => MAX_CTRL,
        Kind::Handshake => MAX_HS,
        Kind::App => MAX_APP,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    pub v: u16,
    pub kind: Kind,
    pub payload: &'a [u8],
}

/// Encodes a given message for the given kind of channel.
///
/// # Arguments
///
/// * `v` - The protocol version.
/// * `kind` - The kind of message to send, see `protocol::framing::Kind`.
/// * `msg` - The message payload to send. Must be of valid size for the given kind of channel.
///
/// # Returns
///
/// A result of type byte vector, formatted as follows if succeeded. A `protocol::error::ErrorCode` if not: 
///
/// | Offset | Size  | Field             |
/// |--------|-------|-------------------|
/// | 0      | 4     | 3 + payload.len() |
/// | 4      | 2     | v                 |
/// | 6      | 1     | kind              |
/// | 7      | len-3 | payload           |
pub fn encode(v: u16, kind: Kind, msg: &impl Serialize) -> Result<Vec<u8>, ErrorCode> {
    let payload = postcard::to_allocvec(msg)?;
    let max = max_len(kind);

    // Validate payload size for the given kind of channel
    if payload.len() > max {
        return Err(ErrorCode::TooLarge {
            max: max as u32,
            got: payload.len() as u32,
        });
    }

    let len = payload.len() + SUBHEADER;
    let mut out = Vec::with_capacity(PREFIX + len);

    out.extend_from_slice(&(len as u32).to_be_bytes());
    out.extend_from_slice(&v.to_be_bytes());

    out.push(kind as u8);
    out.extend_from_slice(&payload);

    Ok(out)
}

pub fn decode<T: DeserializeOwned>(
    buf: &[u8],
    expected: Kind,
) -> Result<Option<(T, usize)>, ErrorCode> {
    match decode_frame(buf)? {
        None => Ok(None),
        Some((f, n)) => {
            if f.kind != expected {
                return Err(ErrorCode::Malformed);
            }
            Ok(Some((postcard::from_bytes(f.payload)?, n)))
        }
    }
}

/// Decodes a given frame of byte data into an inspectable frame.
///
/// # Arguments
///
/// * `buf` - The buffer containing the data to decode.
///
/// # Returns
///
/// description
fn decode_frame(buf: &[u8]) -> Result<Option<(Frame<'_>, usize)>, ErrorCode> {
    // Make sure the buffer is big enough to actually hold the length prefix
    if buf.len() < PREFIX {
        return Ok(None);
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;

    if len < SUBHEADER {
        return Err(ErrorCode::Malformed);
    }
    if len > MAX_APP + SUBHEADER {
        // universal ceiling, before kind is known
        return Err(ErrorCode::TooLarge {
            max: (MAX_APP + SUBHEADER) as u32,
            got: len as u32,
        });
    }
    if buf.len() < PREFIX + SUBHEADER {
        return Ok(None);
    } // need v + kind to judge further

    let v = u16::from_be_bytes([buf[4], buf[5]]);
    let Some(kind) = Kind::from_u8(buf[6]) else {
        return Err(ErrorCode::UnsupportedVersion {
            supported: PROTOCOL_VERSION,
        });
    };
    if v != PROTOCOL_VERSION {
        return Err(ErrorCode::UnsupportedVersion {
            supported: PROTOCOL_VERSION,
        });
    }
    if len > max_len(kind) + SUBHEADER {
        // reject a 1 MiB claim at byte 7
        return Err(ErrorCode::TooLarge {
            max: max_len(kind) as u32,
            got: len as u32,
        });
    }
    let end = PREFIX + len;
    if buf.len() < end {
        return Ok(None);
    }
    Ok(Some((
        Frame {
            v,
            kind,
            payload: &buf[PREFIX + SUBHEADER..end],
        },
        end,
    )))
}

