//! warp CLI - GPU-accelerated bulk data transfer

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;

#[derive(Parser)]
#[command(name = "warp")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Send files to a remote destination
    Send {
        /// Source path (local file or directory)
        source: String,
        /// Destination (e.g., server:/path or /local/path)
        destination: String,
        /// Compression algorithm override
        #[arg(long)]
        compress: Option<String>,
        /// Disable GPU acceleration
        #[arg(long)]
        no_gpu: bool,
        /// Encrypt the archive with a password
        #[arg(long)]
        encrypt: bool,
        /// Password for encryption (prompts if --encrypt is set but password not provided)
        #[arg(long)]
        password: Option<String>,
    },
    /// Fetch files from a remote source
    Fetch {
        /// Source (e.g., server:/path)
        source: String,
        /// Local destination path
        destination: String,
        /// Password for decryption (prompts if archive is encrypted)
        #[arg(long)]
        password: Option<String>,
    },
    /// Start a listener daemon
    Listen {
        /// Port to listen on
        #[arg(short, long, default_value = "9999")]
        port: u16,
        /// Bind address
        #[arg(short, long, default_value = "0.0.0.0")]
        bind: String,
    },
    /// Analyze and plan a transfer without executing
    Plan {
        /// Source path
        source: String,
        /// Destination
        destination: String,
    },
    /// Probe remote server capabilities
    Probe {
        /// Remote server address
        server: String,
    },
    /// Show local system capabilities
    Info,
    /// Resume an interrupted transfer
    Resume {
        /// Session ID to resume
        #[arg(long)]
        session: String,
    },
    /// Benchmark transfer to a remote server
    Bench {
        /// Remote server address
        server: String,
        /// Size of test data
        #[arg(long, default_value = "1G")]
        size: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| filter.into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    match cli.command {
        Commands::Send { source, destination, compress, no_gpu, encrypt, password } => {
            commands::send::execute(&source, &destination, compress.as_deref(), no_gpu, encrypt, password.as_deref()).await
        }
        Commands::Fetch { source, destination, password } => {
            commands::fetch::execute(&source, &destination, password.as_deref()).await
        }
        Commands::Listen { port, bind } => {
            commands::listen::execute(&bind, port).await
        }
        Commands::Plan { source, destination } => {
            commands::plan::execute(&source, &destination).await
        }
        Commands::Probe { server } => {
            commands::probe::execute(&server).await
        }
        Commands::Info => {
            commands::info::execute().await
        }
        Commands::Resume { session } => {
            commands::resume::execute(&session).await
        }
        Commands::Bench { server, size } => {
            commands::bench::execute(&server, &size).await
        }
    }
}
