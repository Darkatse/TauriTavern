use std::path::Path;
use std::sync::Once;
use tracing::{Level, info, error};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    EnvFilter,
    prelude::*,
};

static INIT: Once = Once::new();

/// Initialize the logger with file and console output
pub fn init_logger(log_dir: &Path) -> Result<(), String> {
    INIT.call_once(|| {
        let file_appender = RollingFileAppender::new(
            Rotation::DAILY,
            log_dir,
            "tauritavern.log",
        );
        
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
        
        // Keep the guard alive to prevent the logger from being dropped
        // This is a memory leak, but it's fine for our use case
        Box::leak(Box::new(_guard));
        
        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info"));
            
        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::Layer::new()
                .with_writer(std::io::stdout)
                .with_ansi(true)
                .with_span_events(FmtSpan::CLOSE)
                .with_target(true))
            .with(fmt::Layer::new()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_span_events(FmtSpan::CLOSE)
                .with_target(true));
                
        if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
            eprintln!("Failed to set global default subscriber: {}", e);
        }
        
        info!("Logger initialized");
    });
    
    Ok(())
}

/// Log a debug message
pub fn debug(message: &str) {
    tracing::debug!("{}", message);
}

/// Log an info message
pub fn info(message: &str) {
    tracing::info!("{}", message);
}

/// Log a warning message
pub fn warn(message: &str) {
    tracing::warn!("{}", message);
}

/// Log an error message
pub fn error(message: &str) {
    tracing::error!("{}", message);
}

/// Log an error message with the error object
pub fn error_with_cause(message: &str, error: &dyn std::error::Error) {
    tracing::error!("{}: {}", message, error);
}
