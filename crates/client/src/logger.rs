use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;

static LOG_FILE: OnceLock<String> = OnceLock::new();

pub fn init(is_host: bool) {
    let suffix = if is_host { "host" } else { "peer" };
    // Separate log files
    LOG_FILE
        .set(format!("/tmp/lsp_proxy_{}.log", suffix))
        .unwrap();
}

pub fn log(msg: &str) {
    let unknown_path = "/tmp/lsp_proxy_unknown.log".to_string();
    let path = LOG_FILE.get().unwrap_or(&unknown_path);

    // Get PID
    let pid = std::process::id();

    // Print to stderr (captured by VS Code output panel usually)
    eprintln!("[{}] {}", pid, msg);

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();

    // Write with PID prefix
    let _ = writeln!(file, "[{}] {}", pid, msg);
}
