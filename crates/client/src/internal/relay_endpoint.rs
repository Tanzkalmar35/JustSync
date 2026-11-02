use anyhow::Result;
use just_sync_protocol::invite::RelayEndpoint;
use std::net::SocketAddr;
use tokio::net::lookup_host;

/// Performs async DNS resolution to get the first available SocketAddr.
pub async fn resolve_endpoint(endpoint: RelayEndpoint) -> Result<SocketAddr> {
    let addr_str = endpoint.as_host_port();

    // lookup_host handles both raw IPs and DNS names automatically
    let mut addrs = lookup_host(&addr_str)
        .await
        .map_err(|e| anyhow::anyhow!("DNS lookup failed for {}: {}", addr_str, e))?;

    addrs
        .next()
        .ok_or_else(|| anyhow::anyhow!("DNS returned no records for {}", addr_str))
}
