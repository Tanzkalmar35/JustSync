use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::sync::OnceLock;

static LOG_FILE: OnceLock<String> = OnceLock::new();

/// Initializes the binary's logger to /tmp/justsync.log.
/// If a log file from a previous session exists, it will be deleted first.
///
/// # Panics
///
/// Panics should only happen on one of two occasions:
///
/// 1. For some reason, the file path could not be initialized
/// 2. Something went wrong deleting the old log file - no previous file existing is okay and
///    handled, that's not a panic!
///
/// # Examples
///
/// ```
/// use std::fs;
///
/// logger::init();
/// assert!(fs::exists("/tmp/justsync.log"));
/// ```
pub fn init() {
    LOG_FILE.set(String::from("/tmp/justsync.log")).unwrap();

    let path = LOG_FILE.get().unwrap();

    // Delete file if it already exists
    let deleted = fs::remove_file(path);
    if let Err(e) = deleted
        && e.kind() != ErrorKind::NotFound
    {
        panic!("Unable to prepare log file: {}", e);
    }
}

/// Writes a log message to the log file, and creates the log file if none already exists.
///
/// # Arguments
///
/// * `msg` - The message to write to the file - format: '\[process_id] msg'
///
/// # Examples
///
/// ```
/// use std::fs;
///
/// logger::init();
/// logger::log("Test message!");
/// assert_eq!(fs::read("/tmp/justsync.log").unwrap(), "Test message!".as_bytes());
/// ```
pub fn log(msg: &str) {
    let unknown_path = "/tmp/justsync.log".to_string();
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
