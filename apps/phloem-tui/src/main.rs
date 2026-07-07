use std::path::PathBuf;
use time::macros::format_description;
use tracing::Level;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::time::OffsetTime;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::EnvFilter;

use phloem_tui::app::App;

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

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("phloem_tui=debug,agentik_core=debug,agentik_sdk=debug")),
        )
        .with_writer(file_writer)
        .with_ansi(false)
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(FmtSpan::NONE)
        .with_timer(timer)
        .init();

    tracing::info!("logging initialized — logs directory: {}", log_dir.display());
    Ok(())
}

fn main() -> color_eyre::Result<()> {
    init_logging()?;
    let mut app = App::new();
    app.start()?;
    Ok(())
}
