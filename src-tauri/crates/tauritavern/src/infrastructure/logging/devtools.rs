use std::collections::VecDeque;
use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tracing::Subscriber;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

pub const BACKEND_LOG_EVENT: &str = "tauritavern-backend-log";

const BACKEND_LOG_BUFFER_LIMIT: usize = 800;
const BACKEND_LOG_MAX_MESSAGE_BYTES: usize = 3072;

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    if max_bytes <= 1 {
        return "…".to_string();
    }

    let mut end = max_bytes.saturating_sub(1);
    while end > 0 && !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }

    let mut out = value[..end].to_string();
    out.push('…');
    out
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendLogEntry {
    pub id: u64,
    pub timestamp_ms: i64,
    pub level: String,
    pub target: String,
    pub message: String,
}

pub struct BackendLogStore {
    app_handle: AppHandle,
    next_id: AtomicU64,
    stream_enabled: AtomicBool,
    entries: Mutex<VecDeque<BackendLogEntry>>,
}

impl BackendLogStore {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            next_id: AtomicU64::new(1),
            stream_enabled: AtomicBool::new(false),
            entries: Mutex::new(VecDeque::new()),
        }
    }

    pub fn layer(self: &Arc<Self>) -> BackendLogLayer {
        BackendLogLayer {
            store: self.clone(),
        }
    }

    pub fn set_stream_enabled(&self, enabled: bool) {
        self.stream_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn tail(&self, limit: usize) -> Vec<BackendLogEntry> {
        let entries = self.entries.lock().unwrap();
        let len = entries.len();
        let start = len.saturating_sub(limit);
        entries.iter().skip(start).cloned().collect::<Vec<_>>()
    }

    fn push(&self, mut entry: BackendLogEntry) {
        if entry.id == 0 {
            entry.id = self.next_id.fetch_add(1, Ordering::Relaxed);
        }

        if entry.message.len() > BACKEND_LOG_MAX_MESSAGE_BYTES {
            entry.message = truncate_utf8(&entry.message, BACKEND_LOG_MAX_MESSAGE_BYTES);
        }

        let should_stream = self.stream_enabled.load(Ordering::Relaxed);

        {
            let mut entries = self.entries.lock().unwrap();
            entries.push_back(entry.clone());
            if entries.len() > BACKEND_LOG_BUFFER_LIMIT {
                entries.pop_front();
            }
        }

        if should_stream {
            let _ = self.app_handle.emit(BACKEND_LOG_EVENT, entry);
        }
    }
}

pub struct BackendLogLayer {
    store: Arc<BackendLogStore>,
}

impl<S> Layer<S> for BackendLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        if metadata.target() == "frontend" {
            return;
        }

        let mut visitor = EventFieldVisitor::default();
        event.record(&mut visitor);

        let message = visitor.into_message();

        let entry = BackendLogEntry {
            id: 0,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            level: metadata.level().to_string(),
            target: metadata.target().to_string(),
            message,
        };

        self.store.push(entry);
    }
}

#[derive(Default)]
struct EventFieldVisitor {
    message: Option<String>,
    fields: Vec<(String, String)>,
}

impl EventFieldVisitor {
    fn into_message(self) -> String {
        if let Some(message) = self.message {
            if self.fields.is_empty() {
                return message;
            }

            let suffix = self
                .fields
                .into_iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(" ");
            return format!("{message} ({suffix})");
        }

        if self.fields.is_empty() {
            return String::new();
        }

        self.fields
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl tracing::field::Visit for EventFieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
            return;
        }

        self.fields
            .push((field.name().to_string(), format!("{value:?}")));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
            return;
        }

        self.fields
            .push((field.name().to_string(), value.to_string()));
    }
}

pub fn purge_old_log_files(log_root: &Path, max_age: Duration) -> std::io::Result<usize> {
    let now = SystemTime::now();
    let cutoff = now
        .checked_sub(max_age)
        .ok_or_else(|| std::io::Error::other("Invalid log retention duration"))?;

    let mut deleted = 0usize;

    for entry in std::fs::read_dir(log_root)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }

        let metadata = entry.metadata()?;
        let modified = metadata.modified()?;
        if modified > cutoff {
            continue;
        }

        std::fs::remove_file(entry.path())?;
        deleted += 1;
    }

    Ok(deleted)
}
