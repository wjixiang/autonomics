use crate::config::LogLevel;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

/// Initialize logging based on the configuration
pub fn init_logging(log_level: &LogLevel) {
    let level = match log_level {
        LogLevel::Error => Level::ERROR,
        LogLevel::Warn => Level::WARN,
        LogLevel::Info => Level::INFO,
        LogLevel::Debug => Level::DEBUG,
        LogLevel::Off => return, // Don't initialize logging if disabled
    };

    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();

    let _ = tracing::subscriber::set_global_default(subscriber);
}




