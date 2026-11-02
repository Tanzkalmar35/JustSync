#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramingError {
    UnsupportedVersion { supported: u16 },
    TooLarge { max: u32, got: u32 },
    Malformed,
    Unknown,
}

impl From<postcard::Error> for FramingError {
    fn from(_: postcard::Error) -> Self {
        return Self::Malformed
    }
}

pub enum InviteError {
    AddressEmpty,
    MalformedIpv6Addr,
    MalformedAddr,
    MalformedPort,
    InvalidTrailingChars,
} 
