//! Length-prefixed framing for the JustSync wire protocol.
//!
//! Every frame on the wire looks like this:
//!
//! | Offset | Size    | Field                                  |
//! |--------|---------|----------------------------------------|
//! | 0      | 4       | `payload_len + SUBHEADER` (`u32`, be)  |
//! | 4      | 2       | protocol version (`u16`, be)           |
//! | 6      | 1       | frame kind (`Kind`)                    |
//! | 7      | len - 3 | payload (postcard)                     |
//!
//! The payload length is the first thing on the wire so a reader can tell how
//! many more bytes it has to wait for. The kind follows in the fixed-size
//! header, which lets a reader reject an oversized or unexpected frame as soon
//! as the header is complete, without ever buffering the payload.

use crate::PROTOCOL_VERSION;
use crate::error::FramingError;
use serde::{Serialize, de::DeserializeOwned};

/// Maximum payload size of a relay control frame.
const MAX_CTRL: usize = 2 * 1024;
/// Maximum payload size of a handshake frame.
const MAX_HS: usize = 4 * 1024;
/// Maximum payload size of an application frame.
const MAX_APP: usize = 1 << 20;

/// Size of the length prefix.
const PREFIX: usize = 4;
/// Size of the version + kind subheader.
const SUBHEADER: usize = 3;
/// Size of the complete header (length prefix + subheader).
const HEADER: usize = PREFIX + SUBHEADER;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Relay = 0,
    Handshake = 1,
    App = 2,
}

impl Kind {
    /// The largest payload this kind of frame may carry.
    const fn max_payload(self) -> usize {
        match self {
            Self::Relay => MAX_CTRL,
            Self::Handshake => MAX_HS,
            Self::App => MAX_APP,
        }
    }

    fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Relay,
            1 => Self::Handshake,
            2 => Self::App,
            _ => return None,
        })
    }
}

/// A decoded frame, borrowing its payload from the input buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame<'a> {
    kind: Kind,
    payload: &'a [u8],
}

/// Encodes a given message for the given kind of channel.
///
/// # Arguments
///
/// * `v` - The protocol version.
/// * `kind` - The kind of message to send, see [`Kind`].
/// * `msg` - The message payload to send. Must be of valid size for the given kind of channel.
///
/// # Returns
///
/// A result of type byte vector, formatted as described in the [module docs](self),
/// or an [`ErrorCode`] if serializing fails or the payload is too large for `kind`.
pub fn encode(v: u16, kind: Kind, msg: &impl Serialize) -> Result<Vec<u8>, FramingError> {
    // Start with the header reserved so the payload can be serialized straight
    // into the output buffer rather than into a temporary allocation.
    let mut out = postcard::to_extend(msg, vec![0u8; HEADER])?;

    let payload_len = out.len() - HEADER;
    let max = kind.max_payload();
    if payload_len > max {
        return Err(FramingError::TooLarge {
            max: max as u32,
            got: payload_len as u32,
        });
    }

    // `payload_len <= MAX_APP`, so this cannot overflow a `u32`.
    let len = (payload_len + SUBHEADER) as u32;
    out[0..PREFIX].copy_from_slice(&len.to_be_bytes());
    out[PREFIX..PREFIX + 2].copy_from_slice(&v.to_be_bytes());
    out[PREFIX + 2] = kind as u8;

    Ok(out)
}

/// Attempts to decode data from a given buffer into the specified type, while also validating the
/// decoded message's kind.
///
/// # Arguments
///
/// * `buf` - The byte buffer to read the data to decode from. Any trailing bytes beyond the first
///   complete frame are ignored.
/// * `expected` - The kind of message that is expected to receive.
///
/// # Returns
///
/// * `Err(ErrorCode)` if decoding fails, e.g. malformed message, version mismatch, frame too large
///   or an unexpected kind.
/// * `Ok(None)` if the buffer does not contain a complete frame yet.
/// * `Ok(Some((message, end)))` if decoding succeeds. `end` is the offset in the buffer at which the
///   frame ends, so a caller can feed the remainder to the next call.
pub fn decode<T: DeserializeOwned>(
    buf: &[u8],
    expected: Kind,
) -> Result<Option<(T, usize)>, FramingError> {
    let Some((frame, end)) = decode_frame(buf)? else {
        return Ok(None);
    };

    if frame.kind != expected {
        return Err(FramingError::Malformed);
    }

    Ok(Some((postcard::from_bytes(frame.payload)?, end)))
}

/// Decodes a given frame of byte data into an inspectable frame.
///
/// # Arguments
///
/// * `buf` - The buffer containing the data to decode.
///
/// # Returns
///
/// * `Err(ErrorCode)` in any error scenario, such as version mismatches, oversized frames, an
///   unknown kind or a malformed length prefix.
/// * `Ok(None)` if the buffer does not yet hold a complete frame.
/// * `Ok(Some((frame, end)))` if successful. The frame borrows the payload, `end` is the offset at
///   which the frame ends.
fn decode_frame(buf: &[u8]) -> Result<Option<(Frame<'_>, usize)>, FramingError> {
    if buf.len() < HEADER {
        return Ok(None);
    }

    // Header first: reject bogus lengths, versions and kinds before waiting for
    // a payload that may never be worth buffering.
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len < SUBHEADER {
        return Err(FramingError::Malformed);
    }

    let subheader: [u8; SUBHEADER] = buf[PREFIX..HEADER]
        .try_into()
        .expect("[PREFIX..HEADER] is exactly SUBHEADER bytes long");
    let kind = decode_subheader(&subheader)?;

    let payload_len = len - SUBHEADER;
    let max = kind.max_payload();
    if payload_len > max {
        return Err(FramingError::TooLarge {
            max: max as u32,
            got: payload_len as u32,
        });
    }

    let end = PREFIX + len;
    if buf.len() < end {
        return Ok(None);
    }

    Ok(Some((
        Frame {
            kind,
            payload: &buf[HEADER..end],
        },
        end,
    )))
}

/// Decodes the fixed-size `v` + `kind` subheader, validating the protocol version.
///
/// # Arguments
///
/// * `buf` - The subheader bytes, exactly [`SUBHEADER`] long.
///
/// # Returns
///
/// * `Err(ErrorCode::UnsupportedVersion)` if the peer speaks another protocol version.
/// * `Err(ErrorCode::Malformed)` if the frame kind is unknown.
/// * `Ok(kind)` if the subheader is valid.
fn decode_subheader(buf: &[u8; SUBHEADER]) -> Result<Kind, FramingError> {
    let v = u16::from_be_bytes([buf[0], buf[1]]);
    if v != PROTOCOL_VERSION {
        return Err(FramingError::UnsupportedVersion {
            supported: PROTOCOL_VERSION,
        });
    }

    Kind::from_u8(buf[2]).ok_or(FramingError::Malformed)
}

/// #ai-generated - DeepSeek V4.1 Flash, 01.11.26
#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Msg {
        id: u32,
        body: String,
    }

    fn msg() -> Msg {
        Msg {
            id: 7,
            body: "hello framing".to_string(),
        }
    }

    /// Builds a frame from raw parts, bypassing `encode`, so that malformed
    /// headers can be tested too.
    fn raw_frame(v: u16, kind_byte: u8, payload: &[u8]) -> Vec<u8> {
        let len = payload.len() + SUBHEADER;
        let mut out = Vec::with_capacity(PREFIX + len);
        out.extend_from_slice(&(len as u32).to_be_bytes());
        out.extend_from_slice(&v.to_be_bytes());
        out.push(kind_byte);
        out.extend_from_slice(payload);
        out
    }

    /// Just the header, as a decoder would see it before the payload arrives.
    fn header_only(len: usize, v: u16, kind_byte: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER);
        out.extend_from_slice(&(len as u32).to_be_bytes());
        out.extend_from_slice(&v.to_be_bytes());
        out.push(kind_byte);
        out
    }

    #[test]
    fn encode_matches_documented_layout() {
        let m = msg();
        let payload = postcard::to_extend(&m, Vec::<u8>::new()).unwrap();
        let bytes = encode(PROTOCOL_VERSION, Kind::App, &m).unwrap();

        assert_eq!(bytes.len(), HEADER + payload.len());
        assert_eq!(
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize,
            payload.len() + SUBHEADER
        );
        assert_eq!(
            u16::from_be_bytes([bytes[4], bytes[5]]),
            PROTOCOL_VERSION,
            "version field must be big endian"
        );
        assert_eq!(bytes[6], Kind::App as u8);
        assert_eq!(&bytes[HEADER..], &payload[..]);
    }

    #[test]
    fn encode_escapes_protocol_version_verbatim() {
        // The version is a parameter (not read from the constant) so tests can
        // produce frames from other protocol versions.
        let bytes = encode(PROTOCOL_VERSION + 1, Kind::Relay, &msg()).unwrap();
        assert_eq!(u16::from_be_bytes([bytes[4], bytes[5]]), PROTOCOL_VERSION + 1);
    }

    #[test]
    fn roundtrip_for_every_kind() {
        for kind in [Kind::Relay, Kind::Handshake, Kind::App] {
            let bytes = encode(PROTOCOL_VERSION, kind, &msg()).unwrap();
            let decoded = decode::<Msg>(&bytes, kind).unwrap();

            assert_eq!(decoded, Some((msg(), bytes.len())), "kind {kind:?}");
        }
    }

    #[test]
    fn roundtrip_empty_payload() {
        let bytes = encode(PROTOCOL_VERSION, Kind::Relay, &()).unwrap();

        assert_eq!(bytes.len(), HEADER);
        assert_eq!(
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize,
            SUBHEADER,
            "an empty payload is the smallest legal frame"
        );
        assert_eq!(decode::<()>(&bytes, Kind::Relay), Ok(Some(((), HEADER))));
    }

    #[test]
    fn decode_returns_none_for_every_truncated_prefix() {
        let bytes = encode(PROTOCOL_VERSION, Kind::App, &msg()).unwrap();

        for n in 0..bytes.len() {
            assert_eq!(
                decode::<Msg>(&bytes[..n], Kind::App),
                Ok(None),
                "truncated at {n} bytes"
            );
        }
    }

    #[test]
    fn decode_streams_back_to_back_frames() {
        let first = encode(PROTOCOL_VERSION, Kind::App, &msg()).unwrap();
        let second = encode(PROTOCOL_VERSION, Kind::App, &msg()).unwrap();

        let mut buf = first.clone();
        buf.extend_from_slice(&second);

        let (m1, end) = decode::<Msg>(&buf, Kind::App).unwrap().unwrap();
        assert_eq!(m1, msg());
        assert_eq!(end, first.len());

        let (m2, end2) = decode::<Msg>(&buf[end..], Kind::App).unwrap().unwrap();
        assert_eq!(m2, msg());
        assert_eq!(end2, second.len(), "trailing frame ends at its own length");
    }

    #[test]
    fn decode_rejects_unexpected_kind() {
        let bytes = encode(PROTOCOL_VERSION, Kind::Relay, &msg()).unwrap();

        assert_eq!(
            decode::<Msg>(&bytes, Kind::App),
            Err(FramingError::Malformed)
        );
    }

    #[test]
    fn decode_rejects_version_mismatch() {
        let bytes = encode(PROTOCOL_VERSION + 1, Kind::App, &msg()).unwrap();

        assert_eq!(
            decode::<Msg>(&bytes, Kind::App),
            Err(FramingError::UnsupportedVersion {
                supported: PROTOCOL_VERSION
            })
        );
    }

    #[test]
    fn decode_rejects_unknown_kind_from_header_alone() {
        let bytes = header_only(SUBHEADER, PROTOCOL_VERSION, 3);

        assert_eq!(
            decode::<Msg>(&bytes, Kind::App),
            Err(FramingError::Malformed),
            "an unknown kind must be rejected before the payload is buffered"
        );
    }

    #[test]
    fn decode_rejects_version_mismatch_from_header_alone() {
        let bytes = header_only(SUBHEADER, PROTOCOL_VERSION + 1, Kind::App as u8);

        assert_eq!(
            decode::<Msg>(&bytes, Kind::App),
            Err(FramingError::UnsupportedVersion {
                supported: PROTOCOL_VERSION
            }),
            "the version must be validated before waiting for a payload"
        );
    }

    #[test]
    fn decode_rejects_length_smaller_than_subheader() {
        for len in 0..SUBHEADER {
            let bytes = header_only(len, PROTOCOL_VERSION, Kind::Relay as u8);

            assert_eq!(
                decode::<Msg>(&bytes, Kind::Relay),
                Err(FramingError::Malformed),
                "length {len} cannot even hold the subheader"
            );
        }
    }

    #[test]
    fn decode_rejects_oversized_claimed_length() {
        let claimed = MAX_APP + SUBHEADER + 1;
        let bytes = header_only(claimed, PROTOCOL_VERSION, Kind::App as u8);

        assert_eq!(
            decode::<Msg>(&bytes, Kind::App),
            Err(FramingError::TooLarge {
                max: MAX_APP as u32,
                got: (MAX_APP + 1) as u32
            }),
            "an oversized length must be rejected from the header, without allocating"
        );
    }

    #[test]
    fn decode_accepts_length_at_the_app_limit() {
        let claimed = MAX_APP + SUBHEADER;
        let bytes = header_only(claimed, PROTOCOL_VERSION, Kind::App as u8);

        assert_eq!(
            decode::<Vec<u8>>(&bytes, Kind::App),
            Ok(None),
            "the largest legal length is accepted, it just needs more data"
        );
    }

    #[test]
    fn decode_enforces_per_kind_payload_limit() {
        // Same payload, same frame length: only the kind decides whether it fits.
        let payload = vec![0u8; MAX_CTRL + 1];

        let too_big = raw_frame(PROTOCOL_VERSION, Kind::Relay as u8, &payload);
        assert_eq!(
            decode::<Vec<u8>>(&too_big, Kind::Relay),
            Err(FramingError::TooLarge {
                max: MAX_CTRL as u32,
                got: (MAX_CTRL + 1) as u32
            })
        );

        let fits = raw_frame(PROTOCOL_VERSION, Kind::App as u8, &payload);
        let (frame, end) = decode_frame(&fits).unwrap().unwrap();
        assert_eq!(frame.kind, Kind::App);
        assert_eq!(frame.payload, &payload[..]);
        assert_eq!(end, fits.len());
    }

    #[test]
    fn decode_accepts_payload_at_kind_limit() {
        let payload = vec![0u8; MAX_CTRL];
        let bytes = raw_frame(PROTOCOL_VERSION, Kind::Relay as u8, &payload);

        let (frame, end) = decode_frame(&bytes).unwrap().unwrap();
        assert_eq!(frame.kind, Kind::Relay);
        assert_eq!(frame.payload.len(), MAX_CTRL);
        assert_eq!(end, bytes.len());
    }

    #[test]
    fn encode_rejects_payload_above_kind_limit() {
        let payload = vec![0u8; MAX_CTRL + 1];
        let serialized_len = postcard::to_extend(&payload, Vec::<u8>::new())
            .unwrap()
            .len();

        assert_eq!(
            encode(PROTOCOL_VERSION, Kind::Relay, &payload),
            Err(FramingError::TooLarge {
                max: MAX_CTRL as u32,
                got: serialized_len as u32
            })
        );
        assert!(encode(PROTOCOL_VERSION, Kind::App, &payload).is_ok());
    }

    #[test]
    fn decode_rejects_payload_that_is_invalid_for_type() {
        // One byte can never be a valid `Msg`, so postcard reports a truncation.
        let bytes = raw_frame(PROTOCOL_VERSION, Kind::App as u8, &[0x01]);

        assert_eq!(
            decode::<Msg>(&bytes, Kind::App),
            Err(FramingError::Malformed)
        );
    }

    #[test]
    fn decode_never_panics_on_arbitrary_input() {
        // Deterministic xorshift: every length from 0..=64 with random bytes.
        // The decoder must always answer with a value, never a panic.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        for len in 0..=64 {
            let buf: Vec<u8> = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    state as u8
                })
                .collect();

            for kind in [Kind::Relay, Kind::Handshake, Kind::App] {
                let _ = decode::<Msg>(&buf, kind);
            }
        }
    }
}
