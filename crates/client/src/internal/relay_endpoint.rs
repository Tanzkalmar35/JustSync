use anyhow::{Result, anyhow};
use rustls::pki_types::ServerName;
use std::net::{IpAddr, SocketAddr};
use tokio::net::lookup_host;

/// A validated endpoint ready for DNS resolution and QUIC connection.
#[derive(Debug, Clone)]
pub struct RelayEndpoint {
    pub host: String,
    pub port: u16,
}

impl RelayEndpoint {
    /// Parses and strictly validates a host string (offline).
    pub fn parse(input: &str, default_port: u16) -> Result<Self> {
        let input = input.trim();
        if input.is_empty() {
            return Err(anyhow!("Address cannot be empty"));
        }

        let (host_part, port) = Self::split_host_port(input, default_port)?;

        // 1. Is it a valid IP address? (Handles both IPv4 and IPv6)
        if host_part.parse::<IpAddr>().is_ok() {
            return Ok(Self {
                host: host_part.to_string(),
                port,
            });
        }

        // 2. Is it a valid domain name suitable for TLS/QUIC?
        if ServerName::try_from(host_part).is_ok() {
            return Ok(Self {
                host: host_part.to_string(),
                port,
            });
        }

        Err(anyhow!("Invalid IP or domain name: '{}'", host_part))
    }

    /// Safely splits the host and port, respecting IPv6 brackets (e.g., `[::1]:5000`)
    fn split_host_port(input: &str, default_port: u16) -> Result<(&str, u16)> {
        // Handle IPv6 with brackets
        if input.starts_with('[') {
            let end_bracket = input
                .find(']')
                .ok_or_else(|| anyhow!("Missing closing bracket for IPv6 address: '{}'", input))?;

            let host = &input[1..end_bracket];
            let remainder = &input[end_bracket + 1..];

            if let Some(port_str) = remainder.strip_prefix(':') {
                let port = port_str
                    .parse::<u16>()
                    .map_err(|_| anyhow!("Invalid port number: '{}'", port_str))?;
                return Ok((host, port));
            } else if remainder.is_empty() {
                return Ok((host, default_port));
            } else {
                return Err(anyhow!(
                    "Trailing characters after IPv6 address: '{}'",
                    input
                ));
            }
        }

        // Handle IPv4 or Domain with optional port
        if let Some((host, port_str)) = input.rsplit_once(':') {
            let port = port_str
                .parse::<u16>()
                .map_err(|_| anyhow!("Invalid port number: '{}'", port_str))?;
            Ok((host, port))
        } else {
            Ok((input, default_port))
        }
    }

    /// Performs async DNS resolution to get the first available SocketAddr.
    pub async fn resolve(&self) -> Result<SocketAddr> {
        let addr_str = self.as_host_port();

        // lookup_host handles both raw IPs and DNS names automatically
        let mut addrs = lookup_host(&addr_str)
            .await
            .map_err(|e| anyhow::anyhow!("DNS lookup failed for {}: {}", addr_str, e))?;

        addrs
            .next()
            .ok_or_else(|| anyhow::anyhow!("DNS returned no records for {}", addr_str))
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
}
