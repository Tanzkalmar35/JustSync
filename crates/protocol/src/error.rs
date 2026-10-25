pub enum ErrorCode {
    UnknownSession,
    BadAdmission,
    Expired,
    UnsupportedVersion { supported: u16 },
    TooLarge { max: u32, got: u32 },
    Malformed,
    AuthFailed,
    BadInvite,
    Replay,
    RateLimited,
    HandshakeFailed,
    BadDelegation,
    Unknown,
}

impl From<postcard::Error> for ErrorCode {
    fn from(_: postcard::Error) -> Self {
        return Self::Malformed
    }
}
