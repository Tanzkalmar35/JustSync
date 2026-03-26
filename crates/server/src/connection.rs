use quinn::Connection;
use tokio::io;

pub async fn connect_to_host(host: Connection, peer: Connection) -> () {
    // Open host <-> relay and peer <-> relay streams
    let (mut host_send, mut host_recv) = host
        .open_bi()
        .await
        .expect("Couldn't open new peer stream (relay <-> host)");
    let (mut peer_send, mut peer_recv) = peer
        .open_bi()
        .await
        .expect("Couldn't open stream to host (relay <-> peer)");

    // Specify the async copy task
    let peer_to_host = async { io::copy(&mut peer_recv, &mut host_send).await };
    let host_to_peer = async { io::copy(&mut host_recv, &mut peer_send).await };

    // Join streams
    let (to_host_result, to_peer_result) = tokio::join!(peer_to_host, host_to_peer);

    match to_host_result {
        Ok(bytes) => println!(
            "Peer -> Host stream closed natively. Copied {} bytes.",
            bytes
        ),
        Err(e) => eprintln!("Peer -> Host stream error: {}", e),
    }
    match to_peer_result {
        Ok(bytes) => println!(
            "Host -> Peer stream closed natively. Copied {} bytes.",
            bytes
        ),
        Err(e) => eprintln!("Host -> Peer stream error: {}", e),
    }
}
