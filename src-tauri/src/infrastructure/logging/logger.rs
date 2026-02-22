use std::path::Path;
use std::sync::{Once, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    prelude::*,
    EnvFilter,
};

static INIT: Once = Once::new();
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

pub const BACKEND_ERROR_EVENT: &str = "tauritavern-backend-error";

#[derive(Clone, Serialize)]
struct BackendErrorEventPayload {
    message: String,
}

pub fn bind_app_handle(app_handle: AppHandle) {
    let _ = APP_HANDLE.set(app_handle);
}

/// Initialize the logger with file and console output
pub fn init_logger(log_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(log_dir)
        .map_err(|error| format!("Failed to create log directory {:?}: {}", log_dir, error))?;

    INIT.call_once(|| {
        let file_appender = RollingFileAppender::new(Rotation::DAILY, log_dir, "tauritavern.log");

        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

        // Keep the guard alive to prevent the logger from being dropped
        // This is a memory leak, but it's fine for our use case
        Box::leak(Box::new(_guard));

        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(
                fmt::Layer::new()
                    .with_writer(std::io::stdout)
                    .with_ansi(true)
                    .with_span_events(FmtSpan::CLOSE)
                    .with_target(true),
            )
            .with(
                fmt::Layer::new()
                    .with_writer(non_blocking)
                    .with_ansi(false)
                    .with_span_events(FmtSpan::CLOSE)
                    .with_target(true),
            );

        if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
            eprintln!("Failed to set global default subscriber: {}", e);
        }

        tracing::debug!("Logger initialized");
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
    emit_error_event(message);
}

fn emit_error_event(message: &str) {
    let normalized = message.trim();
    if normalized.is_empty() {
        return;
    }

    let Some(app_handle) = APP_HANDLE.get() else {
        return;
    };

    let payload = BackendErrorEventPayload {
        message: normalized.to_string(),
    };

    if let Err(error) = app_handle.emit(BACKEND_ERROR_EVENT, payload) {
        eprintln!("Failed to emit backend error event: {}", error);
    }
}
