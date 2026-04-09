use quinn::Connection;
use tokio::io;

pub async fn hotwire(a: Connection, b: Connection) -> () {
    // Open a <-> relay and b <-> relay streams
    let (mut a_send, mut a_recv) = a
        .open_bi()
        .await
        .expect("Couldn't open new peer stream (relay <-> host)");
    let (mut b_send, mut b_recv) = b
        .open_bi()
        .await
        .expect("Couldn't open stream to host (relay <-> peer)");

    // Join streams
    let _ = tokio::select! {
        result = io::copy(&mut b_recv, &mut a_send) => {
            let _ = a_send.finish();
            let _ = b_send.finish();
            result
        },
        result = io::copy(&mut a_recv, &mut b_send) => {
            let _ = b_send.finish();
            let _ = a_send.finish();
            result
        },
    };
}
