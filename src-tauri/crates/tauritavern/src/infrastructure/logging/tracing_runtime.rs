use std::path::Path;
use std::sync::Arc;

use tracing::Subscriber;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, format::FmtSpan},
    prelude::*,
};

use super::devtools::BackendLogStore;

pub type BackendErrorSink = Arc<dyn Fn(String) + Send + Sync + 'static>;

pub struct TracingRuntimeGuard {
    _file_guard: tracing_appender::non_blocking::WorkerGuard,
}

pub fn init_tracing(
    log_dir: &Path,
    backend_log_store: Option<Arc<BackendLogStore>>,
    backend_error_sink: BackendErrorSink,
) -> Result<TracingRuntimeGuard, String> {
    std::fs::create_dir_all(log_dir)
        .map_err(|error| format!("Failed to create log directory {:?}: {}", log_dir, error))?;

    let file_appender = RollingFileAppender::new(Rotation::DAILY, log_dir, "tauritavern.log");
    let (non_blocking, file_guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

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
        )
        .with(backend_log_store.map(|store| store.layer()))
        .with(BackendUserErrorLayer {
            sink: backend_error_sink,
        });

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|error| format!("Failed to set global tracing subscriber: {error}"))?;

    tracing::debug!("Tracing initialized");
    Ok(TracingRuntimeGuard {
        _file_guard: file_guard,
    })
}

struct BackendUserErrorLayer {
    sink: BackendErrorSink,
}

impl<S> Layer<S> for BackendUserErrorLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let Some(message) = backend_user_error_message(event) else {
            return;
        };

        (self.sink)(message);
    }
}

fn backend_user_error_message(event: &tracing::Event<'_>) -> Option<String> {
    let metadata = event.metadata();
    if *metadata.level() != tracing::Level::ERROR
        || metadata.target() != crate::observability_targets::USER_VISIBLE_ERROR
    {
        return None;
    }

    let mut visitor = UserErrorVisitor::default();
    event.record(&mut visitor);
    let message = visitor.into_message()?;
    let normalized = message.trim();
    if normalized.is_empty() {
        return None;
    }

    Some(normalized.to_string())
}

#[derive(Default)]
struct UserErrorVisitor {
    message: Option<String>,
}

impl UserErrorVisitor {
    fn into_message(self) -> Option<String> {
        self.message
    }
}

impl tracing::field::Visit for UserErrorVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let value = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(value);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing::Dispatch;

    use super::*;

    struct CaptureUserErrorLayer {
        messages: Arc<Mutex<Vec<String>>>,
    }

    impl<S> Layer<S> for CaptureUserErrorLayer
    where
        S: Subscriber,
    {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            if let Some(message) = backend_user_error_message(event) {
                self.messages.lock().unwrap().push(message);
            }
        }
    }

    #[test]
    fn extracts_only_user_visible_error_events() {
        let messages = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(CaptureUserErrorLayer {
            messages: messages.clone(),
        });
        let dispatch = Dispatch::new(subscriber);

        tracing::dispatcher::with_default(&dispatch, || {
            tracing::error!(target: "tauritavern::other", "ordinary error");
            tracing::warn!(target: crate::observability_targets::USER_VISIBLE_ERROR, "warning");
            tracing::error!(target: crate::observability_targets::USER_VISIBLE_ERROR, "visible {}", "error");
        });

        assert_eq!(*messages.lock().unwrap(), vec!["visible error".to_string()]);
    }
}
