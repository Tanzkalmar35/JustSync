use std::net::IpAddr;

use crate::{PROTOCOL_VERSION, error::InviteError};

/// RFC 1035 §2.3.4.
const MAX_HOST_LEN: usize = 253;
/// RFC 1035 §2.3.1.
const MAX_LABEL_LEN: usize = 63;

/// A validated endpoint ready for DNS resolution and QUIC connection.
#[derive(Debug, Clone)]
pub struct RelayEndpoint {
    pub host: String,
    pub port: u16,
}

impl RelayEndpoint {
    /// Parses and strictly validates a host string (offline).
    pub fn parse(input: &str, default_port: u16) -> Result<Self, InviteError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(InviteError::AddressEmpty);
        }

        let (host_part, port) = Self::destruct_addr(input, default_port)?;

        // Syntactic validation
        if host_part.parse::<IpAddr>().is_ok() {
            return Ok(Self {
                host: host_part.to_string(),
                port,
            });
        }

        // Compatibility check
        if Self::is_valid_host(host_part) {
            return Ok(Self {
                host: host_part.to_string(),
                port,
            });
        }

        Err(InviteError::MalformedAddr)
    }

    /// Returns the formatted string required by `tokio::net::lookup_host`
    pub fn as_host_port(&self) -> String {
        // If it's an IPv6 address (contains a colon), re-wrap it in brackets
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// Safely splits the host and port, respecting IPv6 brackets (e.g., `[::1]:5000`)
    fn destruct_addr(input: &str, default_port: u16) -> Result<(&str, u16), InviteError> {
        // Handle IPv6 with brackets
        if input.starts_with('[') {
            let end_bracket = input
                .find(']')
                .ok_or_else(|| InviteError::MalformedIpv6Addr)?;

            let host = &input[1..end_bracket];
            let remainder = &input[end_bracket + 1..];

            return if let Some(port_str) = remainder.strip_prefix(':') {
                let port = port_str
                    .parse::<u16>()
                    .map_err(|_| InviteError::MalformedPort)?;
                Ok((host, port))
            } else if remainder.is_empty() {
                Ok((host, default_port))
            } else {
                Err(InviteError::InvalidTrailingChars)
            };
        }

        // Handle IPv4 or Domain with optional port
        if let Some((host, port_str)) = input.rsplit_once(':') {
            let port = port_str
                .parse::<u16>()
                .map_err(|_| InviteError::MalformedPort)?;
            Ok((host, port))
        } else {
            Ok((input, default_port))
        }
    }

    /// Validates a given hostname to make sure it's a valid relay server destination.
    /// 
    /// # Arguments
    ///
    /// * `host` - The host name to validate.
    ///
    /// # Returns
    ///
    /// * Ok(()) if the hostname is valid,
    /// * Err(InviteError) if not.
    fn validate_hostname(host: &str) -> Result<(), InviteError> {
        // Root label: the trailing dot is implicit, "example.com." == "example.com".
        let host = host.strip_suffix('.').unwrap_or(host);

        // Basic format validation
        if !host.is_ascii() || host.is_empty() || host.len() > MAX_HOST_LEN {
            return Err(InviteError::MalformedAddr);
        }

        for label in host.split('.') {
            let bytes = label.as_bytes();
            if bytes.is_empty() || bytes.len() > MAX_LABEL_LEN {
                return Err(InviteError::MalformedAddr);
            }
            // No leading/trailing hyphen
            if !bytes[0].is_ascii_alphanumeric() || !bytes[bytes.len() - 1].is_ascii_alphanumeric()
            {
                return Err(InviteError::MalformedAddr);
            }
            if !bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
            {
                return Err(InviteError::MalformedAddr);
            }
        }

        // An all-numeric last label is an IPv4 address that failed to parse
        if host
            .rsplit('.')
            .next()
            .is_some_and(|tld| tld.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(InviteError::MalformedAddr);
        }

        Ok(())
    }
}

pub struct InviteV1 {
    v: [u8; 16],
    relay: RelayEndpoint,
    relay_spki_fp: Option<[u8; 32]>,
    session_id: [u8; 16],
    admission: [u8; 32],
    invite_secret: [u8; 32],
    authority_pk: [u8; 32],
    expires_at: u64,
}

impl InviteV1 {
    pub fn generate(
        relay: RelayEndpoint,
        fp: [u8; 32],
        authority_pk: [u8; 32],
        ttl: [u8; 32],
    ) -> String {
        let prefix = format!("justsync_v{}", PROTOCOL_VERSION.to_string());

        return String::new();
    }
}
