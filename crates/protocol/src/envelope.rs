pub struct Envelope {
    kind: u16,
    request_id: usize,
    payload: [u8],
}
