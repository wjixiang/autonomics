use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;

use agentik_skill_server::run;

#[derive(Parser)]
#[command(name = "skill-registry", about = "Skill registry gRPC server")]
struct Args {
    /// Address to listen on
    #[arg(long, default_value = "127.0.0.1:50051")]
    addr: SocketAddr,

    /// Skill directories to scan
    #[arg(long = "skill-dir", num_args = 1..)]
    skill_dirs: Vec<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();
    run(args.addr, args.skill_dirs).await
}
