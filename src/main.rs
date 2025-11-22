use std::{env, process::exit};

use crate::proxy::start_proxy;

pub mod proxy;

#[tokio::main]
pub async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "Failed to run proxy: At least 1 argument (the target to execute) must be provided."
        );
        exit(1);
    }

    let target = args[1].clone();
    let target_args = args[2..].to_vec();

    match start_proxy(target, target_args).await {
        Ok(_) => {
            eprintln!("Proxy exited successfully.");
        }
        Err(e) => {
            eprintln!("Proxy failed: {}", e);
            exit(1);
        }
    }
}
