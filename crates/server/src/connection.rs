use quinn::Connection;

pub async fn hotwire(a: Connection, b: Connection) -> () {
    // Open a <-> relay and b <-> relay streams
    println!("Hotwiring {} to {}", a.remote_address(), b.remote_address());
    let (mut a_send, mut a_recv) = a
        .open_bi()
        .await
        .expect("Couldn't open new peer stream (relay <-> host)");
    let (mut b_send, mut b_recv) = b
        .open_bi()
        .await
        .expect("Couldn't open stream to host (relay <-> peer)");

    let _ = a_send.write_all(&[0, 0, 0, 0]).await;
    let _ = b_send.write_all(&[0, 0, 0, 0]).await;

    println!("Hotwired both");

    // Join streams
    tokio::spawn(async move {
        if let Err(e) = tokio::io::copy(&mut b_recv, &mut a_send).await {
            eprintln!("Hotwire copy (B->A) error: {}", e);
        }
        let _ = a_send.finish();
    });
    tokio::spawn(async move {
        if let Err(e) = tokio::io::copy(&mut a_recv, &mut b_send).await {
            eprintln!("Hotwire copy (A->B) error: {}", e);
        }
        let _ = b_send.finish();
    });

    println!("Copy tasks started");
}
