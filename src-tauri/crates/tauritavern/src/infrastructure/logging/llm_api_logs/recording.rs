use std::borrow::Cow;
use std::future::Future;
use std::time::Instant;

use serde_json::Value;
use tokio::io::{AsyncWriteExt, BufWriter};

use tt_domain::errors::DomainError;
use tt_ports::generation_stream::GenerationStreamSender;

use super::files::{request_raw_path, response_raw_sse_path};
use super::readable::{extract_model, pretty_json, wire_log_payload};
use super::store::LlmApiLogStore;
use super::types::{LlmApiLogMeta, LlmApiRawKind};

/// Reads the output text of a stream from its events.
pub(super) trait StreamReadable: Send + 'static {
    fn push(&mut self, event: &str);
    fn into_string(self) -> String;
}

/// One generation request on its way to the log store, for every API family.
pub(super) struct GenerationLog<'a> {
    store: &'a LlmApiLogStore,
    source: String,
    endpoint: String,
    payload: Cow<'a, Value>,
    request_readable: String,
    stream: bool,
    started: Instant,
    started_at_ms: i64,
}

impl<'a> GenerationLog<'a> {
    pub(super) fn start(
        store: &'a LlmApiLogStore,
        source: String,
        endpoint: String,
        payload: &'a Value,
        stream: bool,
        request_readable: impl FnOnce(&Value) -> String,
    ) -> Self {
        let payload = wire_log_payload(payload);
        let request_readable = request_readable(&payload);
        Self {
            store,
            source,
            endpoint,
            payload,
            request_readable,
            stream,
            started: Instant::now(),
            started_at_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub(super) async fn finish_json(
        self,
        result: Result<&Value, &DomainError>,
        response_readable: impl FnOnce(&Value) -> String,
    ) {
        let id = self.store.allocate_id();
        let (response_readable, response_raw, response_raw_kind) = match result {
            Ok(response) => {
                let response = wire_log_payload(response);
                (
                    response_readable(&response),
                    Some(pretty_json(&response)),
                    Some(LlmApiRawKind::Json),
                )
            }
            Err(error) => (error.to_string(), None, None),
        };

        let request_raw = pretty_json(&self.payload);
        let meta = self.meta(id, result.err(), response_readable, response_raw_kind);
        self.store
            .record_entry(meta, Some(request_raw), response_raw)
            .await;
    }

    /// Runs `generate` with a sender that passes every event on to `sender` while writing it to
    /// the log and to `readable`.
    pub(super) async fn record_stream<Fut>(
        self,
        sender: GenerationStreamSender,
        mut readable: impl StreamReadable,
        generate: impl FnOnce(GenerationStreamSender) -> Fut,
    ) -> Result<(), DomainError>
    where
        Fut: Future<Output = Result<(), DomainError>>,
    {
        let id = self.store.allocate_id();
        let request_path = request_raw_path(self.store.log_root(), id);
        let request_raw = pretty_json(&self.payload);
        tauri::async_runtime::spawn(async move {
            if let Err(error) = tokio::fs::write(&request_path, request_raw).await {
                tracing::error!(
                    "Failed to write LLM API request log file {}: {}",
                    request_path.display(),
                    error
                );
            }
        });

        let response_path = response_raw_sse_path(self.store.log_root(), id);
        let mut writer = match tokio::fs::File::create(&response_path).await {
            Ok(file) => Some(BufWriter::new(file)),
            Err(error) => {
                tracing::error!(
                    "Failed to open LLM API SSE log file {}: {}",
                    response_path.display(),
                    error
                );
                None
            }
        };
        let response_raw_kind = writer.as_ref().map(|_| LlmApiRawKind::Sse);

        let (tee, mut events) = tokio::sync::mpsc::unbounded_channel::<String>();
        let forward_task = tauri::async_runtime::spawn(async move {
            while let Some(event) = events.recv().await {
                let _ = sender.send(event.clone());
                readable.push(&event);

                if let Some(file) = writer.as_mut()
                    && (file.write_all(event.as_bytes()).await.is_err()
                        || file.write_all(b"\n").await.is_err())
                {
                    writer = None;
                }
            }

            if let Some(mut file) = writer {
                let _ = file.flush().await;
            }
            readable.into_string()
        });

        let result = generate(tee).await;
        let response_readable = match forward_task.await {
            Ok(text) => text,
            Err(error) => format!("Stream forward task join failed: {error}"),
        };

        let meta = self.meta(
            id,
            result.as_ref().err(),
            response_readable,
            response_raw_kind,
        );
        self.store.record_entry(meta, None, None).await;
        result
    }

    fn meta(
        &self,
        id: u64,
        error: Option<&DomainError>,
        response_readable: String,
        response_raw_kind: Option<LlmApiRawKind>,
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
            source: self.source.clone(),
            model: extract_model(&self.payload),
            endpoint: self.endpoint.clone(),
            duration_ms: self.started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32,
            stream: self.stream,
            error_message: error.map(ToString::to_string),
            request_readable: self.request_readable.clone(),
            response_readable,
            request_raw_kind: LlmApiRawKind::Json,
            response_raw_kind,
        }
    }
}
