use std::panic;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use time::macros::format_description;
use tracing::Level;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::time::OffsetTime;
use tracing_subscriber::fmt::writer::MakeWriterExt;

use opengwas::OpengwasClient;
use tui::app::App;

fn init_logging() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let log_dir = PathBuf::from("logs");
    std::fs::create_dir_all(&log_dir)?;

    let file_appender = RollingFileAppender::new(Rotation::DAILY, &log_dir, "phloem-tui.log");
    let file_writer = file_appender.with_max_level(Level::DEBUG);

    let timer = OffsetTime::new(
        time::UtcOffset::current_local_offset().expect("timezone"),
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]"),
    );

    // Retain the previously-installed hook so it can be re-invoked once the
    // TUI's own panic handling is finalised; currently we log only.
    let _default = panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        tracing::error!(
            target: "panic",
            payload = %info,           // "panicked at src/...: xxx"
            backtrace = %bt,
            "thread panicked"
        );
        // _default(info); // 保留默认行为(打到 stderr)
    }));

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("autonomics_tui=debug,agentik_core=debug,agentik_sdk=debug")
        }))
        .with_writer(file_writer)
        .with_ansi(false)
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(FmtSpan::NONE)
        .with_timer(timer)
        .init();

    tracing::info!(
        "logging initialized — logs directory: {}",
        log_dir.display()
    );
    Ok(())
}

/// Top-level CLI for the autonomics-tui binary.
///
/// The default behaviour (no subcommand given) is identical to running
/// `autonomics-tui tui` — launching the interactive TUI.
#[derive(Debug, Parser)]
#[command(
    name = "autonomics-tui",
    about = "Autonomics interactive terminal UI",
    version,
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Launch the interactive TUI (default when no subcommand is given).
    Tui(TuiArgs),

    /// Local cache management helpers (refresh, inspect, purge).
    Cache(CacheArgs),
}

#[derive(Debug, Args)]
struct TuiArgs {
    /// Optional path to a TUI configuration file.
    #[arg(long, short, value_name = "PATH")]
    config: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct CacheArgs {
    #[command(subcommand)]
    action: CacheAction,
}

/// Subcommands under `autonomics-tui cache ...`.
#[derive(Debug, Subcommand)]
enum CacheAction {
    /// Re-fetch the OpenGWAS gwasinfo catalog from the remote API and
    /// persist the snapshot to the on-disk SQLite cache.
    ///
    /// Equivalent to `OpengwasClient::refresh_disk_cache`. On the next
    /// process restart the new data is picked up automatically without
    /// any further network call.
    RefreshOpengwas(RefreshOpengwasArgs),

    /// Delete the on-disk OpenGWAS gwasinfo cache file. The next query
    /// will trigger a fresh fetch from the remote API.
    ClearOpengwas(ClearOpengwasArgs),
}

#[derive(Debug, Args)]
struct RefreshOpengwasArgs {
    /// Print the resolved on-disk cache file path before refreshing.
    #[arg(long)]
    show_cache_path: bool,
}

#[derive(Debug, Args)]
struct ClearOpengwasArgs {
    /// Skip the interactive confirmation prompt.
    #[arg(long, short = 'y')]
    yes: bool,
}

fn run_tui(_args: TuiArgs) -> color_eyre::Result<()> {
    let mut app = App::new();
    app.start()
}

/// Resolve the OpenGWAS token from env. Mirrors the convention used by
/// the `runtime` crate and `OpengwasClient::new`.
fn opengwas_token() -> color_eyre::Result<String> {
    std::env::var("OPENGWAS_TOKEN")
        .map_err(|_| color_eyre::eyre::eyre!("OPENGWAS_TOKEN env var is not set"))
}

fn run_refresh_opengwas(args: RefreshOpengwasArgs) -> color_eyre::Result<()> {
    let token = opengwas_token()?;
    let client = OpengwasClient::with_cache_dir(Some(&token), default_opengwas_cache_dir())?;

    if args.show_cache_path {
        println!(
            "OpenGWAS cache file: {}",
            client.cache_file_path().display()
        );
    }

    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| color_eyre::eyre::eyre!("failed to build tokio runtime: {e}"))?;

    let refreshed = runtime.block_on(async { client.refresh_disk_cache().await })?;
    println!(
        "OpenGWAS cache refreshed: {} dataset(s) persisted to {}",
        refreshed.len(),
        client.cache_file_path().display()
    );
    Ok(())
}

fn run_clear_opengwas(args: ClearOpengwasArgs) -> color_eyre::Result<()> {
    let token = opengwas_token()?;
    let client = OpengwasClient::with_cache_dir(Some(&token), default_opengwas_cache_dir())?;
    let path = client.cache_file_path();

    if !args.yes {
        eprint!(
            "About to delete OpenGWAS cache at {}. Continue? [y/N] ",
            path.display()
        );
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        if !matches!(buf.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("aborted");
            return Ok(());
        }
    }

    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| color_eyre::eyre::eyre!("failed to build tokio runtime: {e}"))?;
    runtime.block_on(async { client.clear_disk_cache().await })?;
    println!("OpenGWAS cache deleted: {}", path.display());
    Ok(())
}

/// Resolve the cache directory the same way `OpengwasClient::new` does,
/// without round-tripping through env twice.
fn default_opengwas_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("OPENGWAS_CACHE_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache").join("opengwas");
    }
    std::env::temp_dir().join("opengwas")
}

fn main() -> color_eyre::Result<()> {
    init_logging()?;

    let cli = Cli::parse();
    match cli
        .command
        .unwrap_or(Command::Tui(TuiArgs { config: None }))
    {
        Command::Tui(args) => run_tui(args),
        Command::Cache(cache) => match cache.action {
            CacheAction::RefreshOpengwas(args) => run_refresh_opengwas(args),
            CacheAction::ClearOpengwas(args) => run_clear_opengwas(args),
        },
    }
}
