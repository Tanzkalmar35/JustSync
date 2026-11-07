use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "just_sync_client")]
#[command(version = "0.1", about = "A real-time, editor agnostic collaboration engine")]
pub struct ClientContext {
    #[command(subcommand)]
    pub mode: ClientMode,
    #[arg(long, hide = true, global = true)]
    stdio: bool,
}

#[derive(Subcommand, Debug)]
pub enum ClientMode {
    Host {
        #[arg(short = 'r', long = "relay")]
        relay_ip: String,
    },
    Peer {
        #[arg(short = 'i', long = "invitation")]
        invitation: String,
    },
}
