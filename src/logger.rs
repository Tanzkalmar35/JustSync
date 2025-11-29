use std::fs::OpenOptions;
use std::io::Write;

pub fn log(msg: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/lsp_proxy.log")
        .unwrap();

    if let Err(e) = writeln!(file, "{}", msg) {
        eprintln!("Failed to write to log: {}", e);
    }
}
