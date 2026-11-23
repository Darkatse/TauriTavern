use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tt_contracts::observability::LlmApiRawKindDto;
use tt_domain::errors::DomainError;
use tt_ports::generation_stream::GenerationStreamSender;

use super::files::{append_sse_event, create_sse_file, delete_entry_files, response_raw_sse_path};
use super::readable::{pretty_json, wire_log_payload};
use super::request::LoggedRequest;
use super::store::LlmApiLogStore;
use super::types::LlmApiLogMeta;

/// One generation request on its way to the log store, for every API family.
pub(super) struct GenerationLog<'a> {
    store: Arc<LlmApiLogStore>,
    request: LoggedRequest<'a>,
    stream: bool,
    started: Instant,
    started_at_ms: i64,
}

impl<'a> GenerationLog<'a> {
    pub(super) fn start(
        store: Arc<LlmApiLogStore>,
        request: LoggedRequest<'a>,
        stream: bool,
    ) -> Self {
        Self {
            store,
            request,
            stream,
            started: Instant::now(),
            started_at_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub(super) fn finish_json(self, result: Result<&Value, &DomainError>) {
        let id = self.store.allocate_id();
        let (response_raw, response_raw_kind) = match result {
            Ok(response) => (
                Some(pretty_json(&wire_log_payload(response))),
                Some(LlmApiRawKindDto::Json),
            ),
            Err(_) => (None, None),
        };

        let meta = self.meta(id, result.err(), response_raw_kind);
        let request_raw = self.request.raw();
        let store = self.store;
        tauri::async_runtime::spawn(async move {
            store
                .finalize_entry(meta, &request_raw, response_raw.as_deref())
                .await;
        });
    }

    /// Runs `generate` with a sender that passes every event on to `sender` while streaming raw
    /// events to disk.
    pub(super) async fn record_stream<Fut>(
        self,
        sender: GenerationStreamSender,
        generate: impl FnOnce(GenerationStreamSender) -> Fut,
    ) -> Result<(), DomainError>
    where
        Fut: Future<Output = Result<(), DomainError>>,
    {
        let id = self.store.allocate_id();
        let log_root = self.store.log_root().to_path_buf();
        let (tee, mut events) = tokio::sync::mpsc::unbounded_channel::<String>();
        let forward_task = tauri::async_runtime::spawn({
            let log_root = log_root.clone();
            async move {
                // Holds the file until its first error; events reach `sender` either way.
                let mut file = create_sse_file(&log_root, id).await;
                while let Some(event) = events.recv().await {
                    let _ = sender.send(event.clone());
                    if let Ok(writer) = &mut file
                        && let Err(error) = append_sse_event(writer, &event).await
                    {
                        file = Err(error);
                    }
                }
                file?.flush().await
            }
        });

        let result = generate(tee).await;
        let recorded = forward_task.await.map_err(std::io::Error::other).flatten();

        let meta = self.meta(id, result.as_ref().err(), Some(LlmApiRawKindDto::Sse));
        let request_raw = self.request.raw();
        let store = self.store;
        tauri::async_runtime::spawn(async move {
            match recorded {
                Ok(()) => store.finalize_entry(meta, &request_raw, None).await,
                Err(error) => {
                    tracing::error!(
                        "Failed to write LLM API SSE log file {}: {}",
                        response_raw_sse_path(&log_root, id).display(),
                        error
                    );
                    let _ = delete_entry_files(&log_root, id).await;
                }
            }
        });

        result
    }

    fn meta(
        &self,
        id: u64,
        error: Option<&DomainError>,
        response_raw_kind: Option<LlmApiRawKindDto>,
    ) -> LlmApiLogMeta {
        let level = match error {
            None => "INFO",
            Some(DomainError::Cancelled(_)) => "WARN",
            Some(_) => "ERROR",
        };

        LlmApiLogMeta {
            id,
            timestamp_ms: self.started_at_ms,
            level: level.to_string(),
            ok: error.is_none(),
            source: self.request.source.clone(),
            model: self.request.model.clone(),
            endpoint: self.request.endpoint.clone(),
            duration_ms: self.started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
            stream: self.stream,
            error_message: error.map(ToString::to_string),
            response_raw_kind,
        }
    }
}
