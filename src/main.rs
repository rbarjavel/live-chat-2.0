mod client;
mod compression;
mod error;
mod gui_client;
mod media;
mod media_cache;
mod media_player;
mod message;
mod server;
mod setup;
mod ytdlp;

use clap::{Parser, Subcommand};
use error::Result;

#[derive(Parser)]
#[command(name = "image-chat")]
#[command(about = "A TCP chat application with file transfer support", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the chat server
    Server {
        /// Port to listen on
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
    /// Connect to a chat server
    Client {
        /// Server host
        #[arg(short = 'H', long, default_value = "127.0.0.1")]
        host: String,

        /// Server port
        #[arg(short, long, default_value_t = 8080)]
        port: u16,

        /// Your username
        #[arg(short, long)]
        username: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Server { port } => {
            server::run_server(port).await?;
        }
        Commands::Client {
            host,
            port,
            username,
        } => {
            gui_client::run_gui_client(&host, port, username)?;
        }
    }

    Ok(())
}
