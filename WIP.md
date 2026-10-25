Read the protocol crate, the TODO note, and the two design notes. Here's how I'd build those two functions.

## The core decision: split framing from payload parsing

The current stub `decode<T>(buf) -> Option<(T, usize)>` conflates two jobs: locating a frame on a byte stream, and interpreting its payload. I'd make framing its own layer that never knows `T`:

```rust
pub struct Frame<'a> { pub v: u16, pub kind: Kind, pub payload: &'a [u8] }

pub fn encode(v: u16, kind: Kind, msg: &impl Serialize) -> Result<Vec<u8>, ErrorCode>;
pub fn decode_frame(buf: &[u8]) -> Result<Option<(Frame<'_>, usize)>, ErrorCode>;
pub fn decode<T: DeserializeOwned>(buf: &[u8], expected: Kind)
-> Result<Option<(T, usize)>, ErrorCode>; // thin wrapper: decode_frame + postcard
```

Reasons:

- The relay's hot-wire path only needs `decode_frame` — it must be able to validate size/kind and forward opaque bytes without ever binding `T` (that's the "dumb relay" invariant in the notes).
- Returning only `T` throws away `kind` and `v`, so a caller can accidentally decode a `Kind::Relay` frame as a handshake message. A cross-protocol confusion bug is exactly the kind of thing the note's "separate interfaces" rule is meant to prevent. Either return the `Frame` or require the caller to pass the expected kind.
- `T` must be `DeserializeOwned`, not `Deserialize<'de>`: the tuple return has no lifetime to tie to `buf`, and any borrowed payload would dangle the moment the stream buffer advances/compacts. If app frames ever need zero-copy, add a separate borrow-based path next to `decode_frame`.

## Wire layout

`u32 BE len | u16 BE v | u8 kind | postcard(payload)`, with `len = 3 + payload.len()` (everything after the length prefix). That definition matters: it means a future `v2` can add header fields and the frame still self-describes, and the relay can forward by reading 4 bytes and blindly copying `len` bytes.

Header fields big-endian even though postcard is little-endian/varint — network byte order for the part humans debug with `xxd`. Pin it with one golden-vector test so nobody "cleans it up" later.

`Kind` needs `#[repr(u8)]`, `Copy/Clone/Debug/PartialEq`, and a hand-written `TryFrom<u8>`. Don't let the wire discriminant come from serde's derive (postcard encodes variant *index*), or reordering the enum silently changes the protocol.


The wire format is just the byte contract — the thing both binaries must agree on, independent of Rust types. Your sketch is already in `protocol.rs` (it doesn't compile yet: stubs + missing `DeserializeOwned` import), so here's exactly what those bytes should be and which file each rule belongs in.

## The layout, byte by byte

```
offset size field
0 4 len = 3 + payload.len() (u32, big-endian)
4 2 v (u16, big-endian)
6 1 kind (u8: 0 relay, 1 handshake, 2 app)
7 len-3 payload (postcard, little-endian varints inside)
```

Concrete example — `v = 1`, `kind = Relay`, payload `de ad be ef`:

```
00 00 00 07 | 00 01 | 00 | de ad be ef
└─ len=7 ──┘ └─v=1─┘ kind └ payload ┘
```

11 bytes total; `4 + len` is always the frame size, `len - 3` the payload size.

## What I meant by the four points

1. **`len` counts the subheader, not the payload.** If it counted only the payload you'd need `total = 7 + len` and a `len` of 0 would be legal, so `decode` would return an empty frame and the loop spins. With this definition one comparison gives the frame boundary, `len < 3` is unambiguously `Malformed`, and the relay can forward frames by reading 4 bytes and copying `len` bytes without knowing the header shape at all.

2. **Header big-endian, postcard little-endian — deliberately split.** The 7-byte header is a hand-written contract you'll read in `xxd` and in packet-capture tests; network order is the right default there (and it matches the `u32 BE len` the old peer framing already uses). Postcard is only the payload codec. Don't be tempted to make the header postcard too: then the header's own length prefix would be varint and an attacker-controlled varint would decide how many header bytes you need.

3. **`v` in every frame despite negotiation.** Two bytes. Without it, a client that negotiated `v=1` against a relay that speaks `v=2` only finds out when some payload fails to deserialize deep inside `envelope::open`, and you get `Malformed` instead of `UnsupportedVersion`. With it, the first frame is rejected at byte 6. The relay also validates version without tracking per-connection negotiation state.

4. **`kind` pinned by explicit code, never by serde.** `#[repr(u8)]` + `out.push(kind as u8)` on write, an exhaustive `match` on read. serde/postcard encode variant *index*, so if someone later reorders the enum, a derive-based kind silently changes the wire. Unknown `kind` → `UnsupportedVersion` (a v2 peer), not `Malformed` (a corruption) — although note the error variant carries `supported: Vec<u16>`, which is versions, not kinds; I'd add `UnsupportedKind { kind: u8 }` at the *end* of the enum instead.

## Where each piece goes

| Where | What |
|---|---|
| `crates/protocol/src/framing.rs` (rename your `protocol.rs`) | `MAX_CTRL/MAX_HS/MAX_APP`, `Kind`, `Kind::from_u8`, `max_len`, `Frame`, `encode`, `decode_frame`, `decode`, `mod tests` |
| `crates/protocol/src/lib.rs` | `pub mod framing;`, `pub use framing::{…}`; `MAX_FRAME` (currently defined but unused) is superseded by `MAX_CTRL` |
| `crates/protocol/src/error.rs` | derives + `thiserror::Error`; `TooLarge { max: u32, got: u32 }`, `UnsupportedVersion { supported: Vec<u16> }` |
| `crates/protocol/Cargo.toml` | `postcard = { version = "1.1.3", features = ["alloc"] }` (needed for `to_allocvec`; don't rely on the client enabling it), `[dev-dependencies] proptest` |
| `crates/protocol/tests/vectors.rs` | golden byte vectors |

Nothing consumes it yet — server/client still import the deleted `client_relay_handshake`, so the workspace is red on purpose until steps 7–8. `cargo check -p just_sync_protocol` / `cargo test -p just_sync_protocol` is the gate.

## The skeleton

```rust
// framing.rs
use serde::{Serialize, de::DeserializeOwned};
use crate::error::ErrorCode;
use crate::PROTOCOL_VERSION;

pub const MAX_CTRL: usize = 2 * 1024;
pub const MAX_HS: usize = 4 * 1024;
pub const MAX_APP: usize = 1 << 20;

const PREFIX: usize = 4; // len
const SUBHEADER: usize = 3; // v + kind

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind { Relay = 0, Handshake = 1, App = 2 }

impl Kind {
pub fn from_u8(b: u8) -> Option<Self> {
Some(match b { 0 => Self::Relay, 1 => Self::Handshake, 2 => Self::App, _ => return None })
}
}

pub const fn max_len(kind: Kind) -> usize {
match kind { Kind::Relay => MAX_CTRL, Kind::Handshake => MAX_HS, Kind::App => MAX_APP }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame<'a> { pub v: u16, pub kind: Kind, pub payload: &'a [u8] }

pub fn encode(v: u16, kind: Kind, msg: &impl Serialize) -> Result<Vec<u8>, ErrorCode> {
let payload = postcard::to_allocvec(msg)?; // owns the bytes, may overshoot once
let max = max_len(kind);
if payload.len() > max {
return Err(ErrorCode::TooLarge { max: max as u32, got: payload.len() as u32 });
}
let len = payload.len() + SUBHEADER;
let mut out = Vec::with_capacity(PREFIX + len);
out.extend_from_slice(&(len as u32).to_be_bytes());
out.extend_from_slice(&v.to_be_bytes());
out.push(kind as u8);
out.extend_from_slice(&payload);
Ok(out)
}

pub fn decode_frame(buf: &[u8]) -> Result<Option<(Frame<'_>, usize)>, ErrorCode> {
if buf.len() < PREFIX { return Ok(None); }
let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;

if len < SUBHEADER { return Err(ErrorCode::Malformed); }
if len > MAX_APP + SUBHEADER { // universal ceiling, before kind is known
return Err(ErrorCode::TooLarge { max: (MAX_APP + SUBHEADER) as u32, got: len as u32 });
}
if buf.len() < PREFIX + SUBHEADER { return Ok(None); } // need v + kind to judge further

let v = u16::from_be_bytes([buf[4], buf[5]]);
let Some(kind) = Kind::from_u8(buf[6]) else {
return Err(ErrorCode::UnsupportedVersion { supported: vec![PROTOCOL_VERSION] });
};
if v != PROTOCOL_VERSION {
return Err(ErrorCode::UnsupportedVersion { supported: vec![PROTOCOL_VERSION] });
}
if len > max_len(kind) + SUBHEADER { // reject a 1 MiB claim at byte 7
return Err(ErrorCode::TooLarge { max: max_len(kind) as u32, got: len as u32 });
}
let end = PREFIX + len;
if buf.len() < end { return Ok(None); } // incomplete, not invalid
Ok(Some((Frame { v, kind, payload: &buf[PREFIX + SUBHEADER..end] }, end)))
}

pub fn decode<T: DeserializeOwned>(buf: &[u8], expected: Kind)
-> Result<Option<(T, usize)>, ErrorCode>
{
match decode_frame(buf)? {
None => Ok(None),
Some((f, n)) => {
if f.kind != expected { return Err(ErrorCode::Malformed); }
Ok(Some((postcard::from_bytes(f.payload)?, n)))
}
}
}
```

`decode_frame` hard-codes `PROTOCOL_VERSION` in step 1, matching the TODO note. When `ServerHello` negotiation lands (step 4), swap that `if` for the negotiated value passed in — the layout doesn't change.

Two things worth doing while you're in `error.rs`: `TooLarge { max: usize }` from the elaboration note is not wire-portable (32-bit vs 64-bit serialization differs), so `u32`; and add `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]` — the enum crosses the wire inside `RelayToClient::Err`, and postcard encodes variant index, so treat the variant order as frozen and append only.

**🛠 Suggest Followups**

```json
{
"followups": [
{
"prompt": "Implement framing.rs with the layout and check order above and rename protocol.rs, then make cargo test -p just_sync_protocol pass",
"label": "Apply framing"
},
{
"prompt": "Make ErrorCode wire-safe: derives, u32 sizes, Vec<u16> supported versions, append-only variant order",
"label": "Fix ErrorCode"
},
{
"prompt": "Add framing tests: golden vector, truncation at every offset, oversized claims, unknown version and kind, proptest no-panic",
"label": "Wire tests"
}
]
}
```

```
message: Followups suggested!
```

## encode

1. Serialize into a bounded scratch buffer, not an unbounded one: `postcard::to_slice(msg, &mut scratch[..max_len(kind)])` (or `to_allocvec` then length-check — but then a 40 MiB `Patch` is fully materialized before you reject it). `SerializeBufferFull` maps nicely onto `TooLarge { max, got }`.
2. Check the serialized length against `max_len(kind)` — 2 KiB relay, 4 KiB handshake, 1 MiB app — and fail before allocating the output.
3. Build the output with exact capacity, `to_be_bytes` for the length, `PROTOCOL_VERSION`/negotiated `v`, `kind as u8`, then the payload.

Two manifest notes: the protocol crate should declare `postcard = { features = ["alloc"] }` itself instead of relying on workspace feature unification from the client/server (`to_allocvec` isn't in postcard's defaults, so a standalone `cargo publish`/CI build of `just_sync_protocol` would break). And `encode` should take `v` explicitly now, as the elaboration note already has it — the chosen version comes from `ServerHello`, so hiding `PROTOCOL_VERSION` in the function makes step 7 a churn. For step 1 passing the constant through is fine.

## decode — the important part is the check order

The invariant I'd write at the top of the module:

> `None` means "the first bytes don't determine a verdict yet, feed me more." `Err` means "the verdict is already determined and this frame can never be valid." Never return `None` for bytes that are actually malformed, or a read loop will spin forever accumulating garbage.

Order, using only the first 7 bytes before the body arrives:

1. `buf.len() < 4` → `Ok(None)`.
2. `len = u32::from_be_bytes(..)`; `len < 3` → `Malformed` (this kills the old `len == 0` recursion bug). `len > max_len(App)` (the universal ceiling) → `TooLarge` immediately — do **not** wait for 4 GiB of body.
3. `buf.len() < 7` → `Ok(None)`.
4. Read `v` and `kind`. Unknown `v` → `UnsupportedVersion`. Unknown `kind` → `UnsupportedVersion`. Both are decidable from the header alone, so fail now rather than buffering a body you'd discard.
5. Check `len` against `max_len(kind)` → `TooLarge`. Again before the body: a 1 MiB claim on a handshake stream should be rejected at byte 7.
6. `buf.len() < 4 + len` → `Ok(None)` (use `checked_add`; on 32-bit `4 + u32` can overflow if the ceiling check were ever loosened).
7. Slice `&buf[7..4+len]`, `postcard::from_bytes::<T>(payload)`, map `postcard::Error` → `Malformed` via the existing `From`. If the frame's declared kind is wrong for the call → `Malformed`/`UnsupportedVersion` (pick one and document it).
8. Return `(msg, 4 + len)` — consumed bytes, so the caller advances by exactly that.

Strictness question: `from_bytes` accepts trailing bytes inside the payload. I'd reject them at alpha (`take_from_bytes` + `rest.is_empty()`). Postcard isn't self-describing, so "append a field" only works if the appended field is `Option`; tolerating trailing bytes is exactly the decision you'd revisit when you design `AppFrame` evolution. Canonical encoding now is worth more than speculative forward-compat.

Allocation: with a borrow-based `Frame`, decode allocates nothing beyond what `T` owns. Never `Vec::with_capacity(len)` before the completeness check — and note that the read loop's accumulation buffer is the real bound to enforce, capped at `4 + max_len(App)`, or a peer can make you grow forever one frame claim at a time.

## Housekeeping I'd fold into the same step

- Both notes call this file `framing.rs`; `lib.rs` still says `pub mod protocol;`. Rename while it's one file.
- `ErrorCode::TooLarge { max: i32, got: i32 }` and `UnsupportedVersion { supported: i32 }` will be serialized over the wire. Use `u32` for sizes (portable, no negative-cast) and `u16`/`Vec<u16>` for versions. Add the derives (`Serialize, Deserialize, Clone, Copy, Debug, PartialEq, thiserror::Error`) and **append-only ordering** — postcard encodes variant index, so reordering variants later breaks the wire.
- `encode`/`decode` are currently private `fn`s; they need `pub`.

Tests I'd write with it: one golden byte vector; round-trip per kind; truncate a valid frame at every offset → `None`; `len` in `{0,1,2}` → `Malformed`; claims just over each kind's cap fed as only 7 bytes → `TooLarge`; unknown `v` and unknown `kind` → `UnsupportedVersion`; two frames back-to-back → first consumes exactly its length; proptest random bytes never panic and never return a consumed count > `buf.len()`.
