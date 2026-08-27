use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::mpsc;

use just_sync_client::adapters::network::QuicNetworkAdapter;
use just_sync_client::internal::core::{Core, Event};
use just_sync_client::internal::fs::FsOps;
use just_sync_client::internal::handler::EditorCommand;
use just_sync_client::internal::lsp::TextDocumentContentChangeEvent;
use just_sync_client::internal::network::{NetworkAdapter, SessionCfg, SessionRole};

#[derive(Clone)]
struct MockFs;
impl FsOps for MockFs {
    fn scan_project_directory(&self, _path: &str) -> Vec<(String, String)> {
        vec![]
    }

    fn write_project_files(&self, _files: Vec<(String, String)>) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn test_full_system_sync() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Start Relay Server in background
    let relay_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let (addr_tx, addr_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        // We need a slightly modified run_relay or a way to get the bound port
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert_der = cert.cert.der().clone();
        let priv_key = rustls_pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());

        let mut crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert_der],
                rustls_pki_types::PrivateKeyDer::Pkcs8(priv_key),
            )
            .unwrap();
        crypto.alpn_protocols = vec![b"justsync".to_vec()];

        let mut server_config = quinn::ServerConfig::with_crypto(std::sync::Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(crypto).unwrap(),
        ));
        let mut transport_config = quinn::TransportConfig::default();
        transport_config
            .max_idle_timeout(Some(std::time::Duration::from_secs(60).try_into().unwrap()));
        server_config.transport_config(std::sync::Arc::new(transport_config));

        let endpoint = quinn::Endpoint::server(server_config, relay_addr).unwrap();
        let actual_addr = endpoint.local_addr().unwrap();
        let _ = addr_tx.send(actual_addr);

        let server = just_sync_server::server::Server::setup();
        while let Some(incoming) = endpoint.accept().await {
            let server_ref = server.clone();
            tokio::spawn(async move {
                if let Ok(connection) = incoming.await {
                    // We need to reach into server's handle_connection logic
                    // For now, let's copy it or make it public in the server crate.
                    // To keep this test moving, I'll assume we made a public handler.
                    let (mut send, mut recv) = connection.accept_bi().await.unwrap();
                    let mut buf = vec![0u8; 1024];
                    let n = recv.read(&mut buf).await.unwrap().unwrap_or(0);
                    let msg: just_sync_server::ControlMessage = serde_json::from_slice(&buf[..n]).unwrap();

                    match msg {
                        just_sync_server::ControlMessage::Register { key: _ } => {
                            let session = just_sync_server::session::Session::new(
                                std::sync::Arc::new(connection.clone()),
                                "test-key".to_string(),
                            );
                            let session_name = session.name.clone();
                            server_ref.register_session(session);
                            let ans = just_sync_server::ControlMessage::SessionCreated {
                                status: "ok".to_string(),
                                name: session_name,
                            };
                            send.write_all(&serde_json::to_vec(&ans).unwrap())
                                .await
                                .unwrap();
                            send.finish().unwrap();
                        }
                        just_sync_server::ControlMessage::Join { name, key: _ } => {
                            if let Some(mut session) = server_ref.find_session(&name) {
                                let _ = session
                                    .join(
                                        std::sync::Arc::new(connection.clone()),
                                        "test-key".to_string(),
                                        &mut send,
                                    )
                                    .await;
                            }
                        }
                        _ => {}
                    }
                }
            });
        }
    });

    let actual_relay_addr = addr_rx.await.unwrap();

    // Setup Host Client
    let host_id = "host-client".to_string();
    let (host_net_tx, host_net_rx) = mpsc::channel(100);
    let (host_editor_tx, _host_editor_rx) = mpsc::channel::<EditorCommand>(100);

    // This channel is where the network adapter sends events
    let (network_to_test_tx, mut network_to_test_rx) = mpsc::channel(100);
    // This channel is where we forward events to the Core
    let (test_to_core_tx, test_to_core_rx) = mpsc::channel(100);

    let host_session = SessionCfg {
        agent_id: host_id.clone(),
        key: "test-key".to_string(),
        relay_addr: actual_relay_addr,
        role: SessionRole::Host {},
    };

    tokio::spawn(QuicNetworkAdapter::connect_and_run(
        host_session,
        network_to_test_tx,
        host_net_rx,
    ));

    // Spawn Host Core
    let host_core = Core::new(host_id, host_net_tx, host_editor_tx);
    tokio::spawn(host_core.run(test_to_core_rx, true, MockFs));

    // Proxy loop to capture session name
    let proxy_handle = tokio::spawn({
        let test_to_core_tx = test_to_core_tx.clone();
        async move {
            let mut name = String::new();
            while let Some(event) = network_to_test_rx.recv().await {
                if let Event::SessionRegistered { name: n } = &event {
                    name = n.clone();
                }
                let _ = test_to_core_tx.send(event).await;
                if !name.is_empty() {
                    return name;
                }
            }
            name
        }
    });

    let session_name = proxy_handle.await.unwrap();

    // Setup Peer Client
    let peer_id = "peer-client".to_string();
    let (peer_net_tx, peer_net_rx) = mpsc::channel(100);
    let (peer_editor_tx, _peer_editor_rx) = mpsc::channel::<EditorCommand>(100);
    let (peer_network_to_test_tx, mut peer_network_to_test_rx) = mpsc::channel(100);
    let (peer_test_to_core_tx, peer_test_to_core_rx) = mpsc::channel(100);

    let peer_session = SessionCfg {
        agent_id: peer_id.clone(),
        key: "test-key".to_string(),
        relay_addr: actual_relay_addr,
        role: SessionRole::Peer {
            session_name: session_name.clone(),
        },
    };

    tokio::spawn(QuicNetworkAdapter::connect_and_run(
        peer_session,
        peer_network_to_test_tx,
        peer_net_rx,
    ));

    // Spawn Peer Core
    let peer_core = Core::new(peer_id, peer_net_tx, peer_editor_tx);
    tokio::spawn(peer_core.run(peer_test_to_core_rx, false, MockFs));

    // Wait for the Host and Peer to find each other via the relay
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Peer proxy loop to look for patches
    let peer_proxy_handle = tokio::spawn(async move {
        while let Some(event) = peer_network_to_test_rx.recv().await {
            if let Event::RemotePatch { .. } = &event {
                return true;
            }
            let _ = peer_test_to_core_tx.send(event).await;
        }
        false
    });

    // Host types "Hello"
    let host_change = Event::LocalChange {
        uri: "test.txt".to_string(),
        changes: vec![TextDocumentContentChangeEvent {
            range: Some(just_sync_client::internal::lsp::Range {
                start: just_sync_client::internal::lsp::Position {
                    line: 0,
                    character: 0,
                },
                end: just_sync_client::internal::lsp::Position {
                    line: 0,
                    character: 0,
                },
            }),
            text: "Hello from Host".to_string(),
        }],
    };
    test_to_core_tx.send(host_change).await.unwrap();

    // Peer should eventually receive a RemotePatch
    let received = tokio::time::timeout(Duration::from_secs(5), peer_proxy_handle)
        .await
        .expect("Timeout waiting for peer to receive patch")
        .unwrap();

    assert!(received, "Peer never received patch from Host");
}
