use std::marker::PhantomData;

use crate::PROTOCOL_VERSION;
use crate::error::ErrorCode;
use serde::{Serialize, de::DeserializeOwned};

const MAX_CTRL: usize = 2 * 1024;
const MAX_HS: usize = 4 * 1024;
const MAX_APP: usize = 1 << 20;

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
    fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Relay,
            1 => Self::Handshake,
            2 => Self::App,
            _ => return None,
        })
    }
}

const fn max_len(kind: Kind) -> usize {
    match kind {
        Kind::Relay => MAX_CTRL,
        Kind::Handshake => MAX_HS,
        Kind::App => MAX_APP,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame<'a> {
    v: u16,
    kind: Kind,
    payload: &'a [u8],
}

pub struct Framing<'a> {
    phantom: &'a PhantomData<String>,
}

impl<'a> Framing<'a> {
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

    /// Attempts to decode data from a given buffer into the specified type, while also validating the
    /// decoded message's kind.
    ///
    /// # Arguments
    ///
    /// * `buf` - The byte buffer to read the data to decode from.
    /// * `expected` - The kind of message that is expected to receive.
    ///
    /// # Returns
    ///
    /// Err(ErrorCode) if decoding fails for whatever reason, e.g. malformed msg, protocol mismatch, ..
    /// Ok(None) if the buffer does not contain enough data to fully decode into the desired type.
    /// Ok(Some((message, end))) if decoding is successful. The end is the offset in the buffer, where
    /// the actual message data ends.
    pub fn decode<T: DeserializeOwned>(
        buf: &[u8],
        expected: Kind,
    ) -> Result<Option<(T, usize)>, ErrorCode> {
        let framing = Self {
            phantom: &PhantomData,
        };
        match framing.decode_frame(buf)? {
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
    /// * An error code in any error scenario, such as version mismatches, protocol mismatches etc.
    /// * Ok(None) if the message itself is valid, but contains no real payload content.
    /// * Ok((Frame, end)) if successful. The frame holds the data, the end holds the actual received
    ///   length.
    fn decode_frame(&self, buf: &'a [u8]) -> Result<Option<(Frame<'a>, usize)>, ErrorCode> {
        // Decode the length prefix (bytes idx 0-3)
        let prefix_window = buf[0..3]
            .try_into()
            .expect("Expected there to be an impl for [u8] -> [u8; 4]");
        let len = match self.decode_len_prefix(prefix_window)? {
            Some(l) => l,
            None => return Ok(None),
        };

        // Decode the subheader (v + kind, bytes idx 4-6)
        let subheader_window = buf[4..6]
            .try_into()
            .expect("Expected there to be an impl for [u8] -> [u8; 4]");
        let (v, kind) = match self.decode_subheader(subheader_window)? {
            Some((v, k)) => (v, k),
            None => return Ok(None),
        };

        // Validate format
        if len > max_len(kind) + SUBHEADER {
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

    /// Decodes a length prefix from a given byte buffer.
    ///
    /// # Arguments
    ///
    /// * `buf` - The byte buffer to decode from. Must be exactly 4 bytes long.
    ///
    /// # Returns
    ///
    /// * Err(ErrorCode) if the decoding failed, e.g. a malformed length prefix was found.
    /// * Ok(None) if the buffer does not contain enough data to hold a valid length prefix.
    /// * Ok(Some(len)) if the decoding succeeds.
    fn decode_len_prefix(&self, buf: [u8; 4]) -> Result<Option<usize>, ErrorCode> {
        if buf.len() < PREFIX {
            return Ok(None);
        }

        let len = u32::from_be_bytes(buf) as usize;
        if len < SUBHEADER {
            return Err(ErrorCode::Malformed);
        }

        if len > MAX_APP + SUBHEADER {
            return Err(ErrorCode::TooLarge {
                max: (MAX_APP + SUBHEADER) as u32,
                got: len as u32,
            });
        }

        Ok(Some(len))
    }

    /// Attempts to decode a 3 byte long byte buffer into the message `v` and `kind`.
    ///
    /// # Arguments
    ///
    /// * `buf` - The byte buffer to decode from. Needs to be exactly 3 bytes long.
    ///
    /// # Returns
    ///
    /// * Err(Errorcode) if something fails along the way, e.g. protocol version mismatch.
    /// * Ok(None) if The buffer is too small to contain the desired data.
    /// * Ok(Some((v, kind))), the message's v and kind, if successfully decoded.
    fn decode_subheader(&self, buf: [u8; 3]) -> Result<Option<(u16, Kind)>, ErrorCode> {
        if buf.len() < PREFIX + SUBHEADER {
            return Ok(None);
        }
        let v = u16::from_be_bytes([buf[4], buf[5]]);
        let Some(kind) = Kind::from_u8(buf[6]) else {
            return Err(ErrorCode::UnsupportedVersion {
                supported: PROTOCOL_VERSION,
            });
        };

        // Validate protocol version matches
        if v != PROTOCOL_VERSION {
            return Err(ErrorCode::UnsupportedVersion {
                supported: PROTOCOL_VERSION,
            });
        }

        Ok(Some((v, kind)))
    }
}
