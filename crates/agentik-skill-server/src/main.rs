use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use agentik_skill_server::{run, SqliteSkillStore};

#[derive(Parser)]
#[command(name = "skill-registry", about = "Skill registry gRPC server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the gRPC server with a SQLite-backed skill store.
    Serve {
        /// Address to listen on
        #[arg(long, default_value = "127.0.0.1:50051")]
        addr: SocketAddr,

        /// Path to the SQLite database file
        #[arg(long)]
        db: PathBuf,

        /// Skill directories to import on startup (optional)
        #[arg(long = "skill-dir", num_args = 0..)]
        skill_dirs: Vec<PathBuf>,
    },

    /// Import skills from a directory into a SQLite database.
    Import {
        /// Path to the SQLite database file
        db: PathBuf,

        /// Directory to import skills from
        dir: PathBuf,
    },

    /// Export skills from a SQLite database to a directory.
    Export {
        /// Path to the SQLite database file
        db: PathBuf,

        /// Directory to write exported skills to
        dir: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Serve {
            addr,
            db,
            skill_dirs,
        } => {
            run(addr, db, skill_dirs).await?;
        }
        Command::Import { db, dir } => {
            let store = SqliteSkillStore::open(db).await?;
            let n = store.import_from_dir(&dir).await?;
            println!("Imported {n} skill(s) from {}", dir.display());
        }
        Command::Export { db, dir } => {
            let store = SqliteSkillStore::open(db).await?;
            let n = store.export_to_dir(&dir).await?;
            println!("Exported {n} skill(s) to {}", dir.display());
        }
    }
    Ok(())
}
