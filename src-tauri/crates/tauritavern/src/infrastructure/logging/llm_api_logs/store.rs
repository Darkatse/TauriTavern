use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime};
use tt_contracts::observability::{
    ExchangeViewDto, LlmApiLogIndexEntryDto, LlmApiLogPreviewDto, LlmApiLogRawDto,
    LlmApiLogViewDto, LlmApiRawKindDto,
};

use super::LLM_API_LOG_EVENT;
use super::files::{
    delete_entry_files, index_path, load_meta, meta_path, persist_index_file, persist_meta_file,
    persist_raw_files, request_raw_path, response_raw_json_path, response_raw_sse_path, sse_events,
};
use super::readable::{
    merge_stream_events, project_response_view, render_request_readable, render_response_readable,
};
use super::request::request_view;
use super::types::LlmApiLogMeta;

const DEFAULT_KEEP: usize = 5;

pub struct LlmApiLogStore {
    publish_entry: Box<dyn Fn(&LlmApiLogIndexEntryDto) + Send + Sync>,
    log_root: PathBuf,
    next_id: AtomicU64,
    stream_enabled: AtomicBool,
    keep: AtomicU64,
    index: Arc<Mutex<VecDeque<LlmApiLogIndexEntryDto>>>,
    persist_lock: Arc<tokio::sync::Mutex<()>>,
}

impl LlmApiLogStore {
    pub fn new<R: Runtime>(app_handle: AppHandle<R>, log_root: PathBuf) -> Self {
        Self::with_publisher(log_root, move |entry| {
            let _ = app_handle.emit(LLM_API_LOG_EVENT, entry);
        })
    }

    pub(super) fn with_publisher(
        log_root: PathBuf,
        publish_entry: impl Fn(&LlmApiLogIndexEntryDto) + Send + Sync + 'static,
    ) -> Self {
        let mut index = VecDeque::new();
        let mut next_id = 1_u64;

        if let Ok(content) = std::fs::read_to_string(index_path(&log_root))
            && let Ok(entries) = serde_json::from_str::<Vec<LlmApiLogIndexEntryDto>>(&content)
        {
            for entry in entries {
                next_id = next_id.max(entry.id.saturating_add(1));
                index.push_back(entry);
            }
        }

        Self {
            publish_entry: Box::new(publish_entry),
            log_root,
            next_id: AtomicU64::new(next_id),
            stream_enabled: AtomicBool::new(false),
            keep: AtomicU64::new(DEFAULT_KEEP as u64),
            index: Arc::new(Mutex::new(index)),
            persist_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub(super) fn log_root(&self) -> &Path {
        &self.log_root
    }

    pub fn set_stream_enabled(&self, enabled: bool) {
        self.stream_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn apply_settings(&self, keep: u32) {
        let keep = keep as usize;
        self.keep.store(keep as u64, Ordering::Relaxed);

        let removed_ids = {
            let mut index = self.index.lock().unwrap();
            let mut removed = Vec::new();
            while index.len() > keep {
                if let Some(entry) = index.pop_front() {
                    removed.push(entry.id);
                }
            }
            removed
        };

        let log_root = self.log_root.clone();
        let index = Arc::clone(&self.index);
        let persist_lock = Arc::clone(&self.persist_lock);
        tauri::async_runtime::spawn(async move {
            let _guard = persist_lock.lock().await;
            for removed_id in removed_ids {
                let _ = delete_entry_files(&log_root, removed_id).await;
            }
            let index_snapshot = index.lock().unwrap().iter().cloned().collect::<Vec<_>>();
            if let Err(error) = persist_index_file(&log_root, &index_snapshot).await {
                tracing::error!("Failed to persist LLM API log index: {}", error);
            }
        });
    }

    pub(super) fn allocate_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn tail_index(&self, limit: usize) -> Vec<LlmApiLogIndexEntryDto> {
        let entries = self.index.lock().unwrap();
        let len = entries.len();
        let start = len.saturating_sub(limit);
        entries.iter().skip(start).cloned().collect()
    }

    pub async fn get_view(&self, id: u64) -> Result<LlmApiLogViewDto, std::io::Error> {
        let (meta, view) = self.read_entry(id).await?;
        Ok(LlmApiLogViewDto {
            id: meta.id,
            timestamp_ms: meta.timestamp_ms,
            level: meta.level,
            ok: meta.ok,
            source: meta.source,
            model: meta.model,
            endpoint: meta.endpoint,
            duration_ms: meta.duration_ms,
            stream: meta.stream,
            response_raw_kind: meta.response_raw_kind,
            view,
        })
    }

    pub async fn get_preview(&self, id: u64) -> Result<LlmApiLogPreviewDto, std::io::Error> {
        let (meta, view) = self.read_entry(id).await?;
        let request_readable = render_request_readable(&view.request);
        let response_readable = view
            .response
            .as_ref()
            .map(|response| render_response_readable(view.request.kind, response))
            .unwrap_or_default();

        Ok(LlmApiLogPreviewDto {
            id: meta.id,
            timestamp_ms: meta.timestamp_ms,
            level: meta.level,
            ok: meta.ok,
            source: meta.source,
            model: meta.model,
            endpoint: meta.endpoint,
            duration_ms: meta.duration_ms,
            stream: meta.stream,
            error_message: meta.error_message,
            request_readable,
            response_readable,
            response_raw_kind: meta.response_raw_kind,
        })
    }

    async fn read_entry(
        &self,
        id: u64,
    ) -> Result<(LlmApiLogMeta, ExchangeViewDto), std::io::Error> {
        let meta = load_meta(meta_path(&self.log_root, id)).await?;
        let request_raw = tokio::fs::read_to_string(request_raw_path(&self.log_root, id)).await?;
        let request_value = serde_json::from_str::<Value>(&request_raw).map_err(|error| {
            std::io::Error::other(format!("Failed to parse LLM API request JSON: {error}"))
        })?;
        let kind = meta.request_kind();
        let request = request_view(kind, &request_value, meta.http.clone());

        let mut response = match meta.response_raw_kind {
            Some(LlmApiRawKindDto::Json) => {
                let response_raw =
                    tokio::fs::read_to_string(response_raw_json_path(&self.log_root, id)).await?;
                let response_value =
                    serde_json::from_str::<Value>(&response_raw).map_err(|error| {
                        std::io::Error::other(format!(
                            "Failed to parse LLM API response JSON: {error}"
                        ))
                    })?;
                Some(project_response_view(kind, &response_value, Some("")))
            }
            Some(LlmApiRawKindDto::Sse) => {
                let sse_raw =
                    tokio::fs::read_to_string(response_raw_sse_path(&self.log_root, id)).await?;
                let document = merge_stream_events(kind, sse_events(&sse_raw));
                Some(project_response_view(kind, &document, None))
            }
            None => None,
        };
        // The recorded failure stands in front of whatever the response itself reports.
        if let Some(error) = &meta.error_message {
            response.get_or_insert_default().error = Some(error.clone());
        }

        Ok((meta, ExchangeViewDto { request, response }))
    }

    pub async fn get_raw(&self, id: u64) -> Result<LlmApiLogRawDto, std::io::Error> {
        let meta = load_meta(meta_path(&self.log_root, id)).await?;
        let request_raw = tokio::fs::read_to_string(request_raw_path(&self.log_root, id)).await?;

        let response_raw = match meta.response_raw_kind {
            Some(LlmApiRawKindDto::Json) => {
                tokio::fs::read_to_string(response_raw_json_path(&self.log_root, id)).await?
            }
            Some(LlmApiRawKindDto::Sse) => {
                tokio::fs::read_to_string(response_raw_sse_path(&self.log_root, id)).await?
            }
            None => String::new(),
        };

        Ok(LlmApiLogRawDto {
            id,
            request_raw,
            response_raw,
            response_raw_kind: meta.response_raw_kind,
        })
    }

    /// Persists raw files first, then meta, then updates index and emits the live event so every
    /// visible index entry already has readable files on disk.
    pub(super) async fn finalize_entry(
        &self,
        meta: LlmApiLogMeta,
        request_raw: &str,
        response_json: Option<&str>,
    ) {
        let persisted = async {
            persist_raw_files(&self.log_root, meta.id, request_raw, response_json).await?;
            persist_meta_file(&self.log_root, &meta).await
        };
        if let Err(error) = persisted.await {
            tracing::error!(
                "Failed to persist LLM API log entry {} under {}: {}",
                meta.id,
                self.log_root.display(),
                error
            );
            let _ = delete_entry_files(&self.log_root, meta.id).await;
            return;
        }

        let _guard = self.persist_lock.lock().await;
        let index_entry = LlmApiLogIndexEntryDto::from(&meta);
        let keep = self.keep.load(Ordering::Relaxed) as usize;
        let should_stream = self.stream_enabled.load(Ordering::Relaxed);

        let (removed_ids, index_snapshot) = {
            let mut index = self.index.lock().unwrap();
            index.push_back(index_entry.clone());
            let mut removed = Vec::new();
            while index.len() > keep {
                if let Some(entry) = index.pop_front() {
                    removed.push(entry.id);
                }
            }
            (removed, index.iter().cloned().collect::<Vec<_>>())
        };

        if should_stream {
            (self.publish_entry)(&index_entry);
        }

        for removed_id in removed_ids {
            if let Err(error) = delete_entry_files(&self.log_root, removed_id).await {
                tracing::warn!(
                    "Failed to delete old LLM API log entry {}: {}",
                    removed_id,
                    error
                );
            }
        }

        if let Err(error) = persist_index_file(&self.log_root, &index_snapshot).await {
            tracing::error!("Failed to persist LLM API log index: {}", error);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::sync::Arc;

    use async_trait::async_trait;
    use serde_json::{Value, json};
    use tt_contracts::observability::{
        RequestDescriptionDto, RequestHeaderDto, RequestTransportDto,
    };
    use tt_domain::errors::DomainError;
    use tt_ports::generation_stream::{GenerationCancelReceiver, GenerationStreamSender};
    use tt_ports::repositories::chat_completion_repository::{
        AnthropicBetaHeaderMode, ChatCompletionApiConfig, ChatCompletionNormalizationReport,
        ChatCompletionRepository, ChatCompletionRepositoryGenerateResponse, ChatCompletionSource,
        ChatCompletionStreamDelta, GenerationCall, WireRequest,
    };

    use super::LlmApiLogStore;
    use super::response_raw_sse_path;
    use crate::infrastructure::logging::llm_api_logs::LoggingChatCompletionRepository;

    #[derive(Default)]
    struct FakeChatCompletionRepository {
        stream_never_ends: bool,
    }

    #[async_trait]
    impl ChatCompletionRepository for FakeChatCompletionRepository {
        async fn list_models(
            &self,
            _source: ChatCompletionSource,
            _config: &ChatCompletionApiConfig,
        ) -> Result<Value, DomainError> {
            unreachable!()
        }

        async fn generate(
            &self,
            _source: ChatCompletionSource,
            _config: &ChatCompletionApiConfig,
            _endpoint_path: &str,
            _payload: &Value,
        ) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
            Ok(ChatCompletionRepositoryGenerateResponse::new(
                json!({
                    "choices": [{
                        "finish_reason": "stop",
                        "message": { "role": "assistant", "content": "Hello JSON" }
                    }],
                    "usage": { "prompt_tokens": 12, "completion_tokens": 3 }
                }),
                ChatCompletionNormalizationReport::default(),
            ))
        }

        async fn generate_stream(
            &self,
            _source: ChatCompletionSource,
            _config: &ChatCompletionApiConfig,
            _endpoint_path: &str,
            _payload: &Value,
            sender: GenerationStreamSender,
            _cancel: GenerationCancelReceiver,
        ) -> Result<(), DomainError> {
            sender
                .send(
                    r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"Hello SSE"},"finish_reason":"stop"}],"usage":{"prompt_tokens":15,"completion_tokens":4}}"#
                        .to_string(),
                )
                .unwrap();
            if self.stream_never_ends {
                std::future::pending::<()>().await;
            }
            sender.send("[DONE]".to_string()).unwrap();
            Ok(())
        }

        async fn generate_with_deltas(
            &self,
            _source: ChatCompletionSource,
            _config: &ChatCompletionApiConfig,
            _endpoint_path: &str,
            _payload: &Value,
            _on_delta: &mut (dyn FnMut(ChatCompletionStreamDelta) + Send),
        ) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
            unreachable!()
        }

        async fn close_provider_session(&self, _session_id: &str) {}

        fn describe_request<'a>(
            &self,
            _source: ChatCompletionSource,
            _config: &ChatCompletionApiConfig,
            _endpoint_path: &str,
            payload: &'a Value,
            call: GenerationCall,
        ) -> Result<WireRequest<'a>, DomainError> {
            Ok(WireRequest {
                http: described(call),
                body: Cow::Borrowed(payload),
            })
        }
    }

    fn described(call: GenerationCall) -> RequestDescriptionDto {
        RequestDescriptionDto {
            transport: RequestTransportDto::Http,
            method: "POST".to_string(),
            url: "https://api.example.com/v1/chat/completions".to_string(),
            headers: vec![RequestHeaderDto {
                name: "x-call".to_string(),
                value: format!("{call:?}"),
            }],
        }
    }

    fn test_config() -> ChatCompletionApiConfig {
        ChatCompletionApiConfig {
            base_url: "https://api.example.com/v1".to_string(),
            user_configured_endpoint: false,
            api_key: String::new(),
            authorization_header: None,
            vertexai_service_account_json: None,
            extra_headers: Default::default(),
            additional_headers: Default::default(),
            anthropic_beta_header_mode: AnthropicBetaHeaderMode::None,
            aws_bedrock_custom_response_path: None,
            aws_bedrock_custom_stream_path: None,
        }
    }

    #[tokio::test]
    async fn published_entries_are_readable_immediately() {
        let temp_dir = tempfile::tempdir().unwrap();
        let (published_tx, mut published_rx) = tokio::sync::mpsc::unbounded_channel();
        let store = Arc::new(LlmApiLogStore::with_publisher(
            temp_dir.path().to_path_buf(),
            move |entry| {
                let _ = published_tx.send(entry.clone());
            },
        ));
        store.set_stream_enabled(true);

        let repo = LoggingChatCompletionRepository::new(
            Arc::new(FakeChatCompletionRepository::default()),
            Arc::clone(&store),
        );
        let config = test_config();
        let payload = json!({
            "model": "gpt-5",
            "messages": [{ "role": "user", "content": "Ping" }]
        });

        repo.generate(
            ChatCompletionSource::OpenAi,
            &config,
            "/chat/completions",
            &payload,
        )
        .await
        .unwrap();

        let json_entry = published_rx.recv().await.expect("JSON entry published");
        let json_view = store.get_view(json_entry.id).await.unwrap();
        assert_eq!(
            json_view.view.request.http,
            Some(described(GenerationCall::Generate))
        );
        assert!(store.get_preview(json_entry.id).await.is_ok());
        assert!(store.get_raw(json_entry.id).await.is_ok());

        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        repo.generate_stream(
            ChatCompletionSource::OpenAi,
            &config,
            "/chat/completions",
            &payload,
            stream_tx,
            cancel_rx,
        )
        .await
        .unwrap();
        assert!(stream_rx.recv().await.is_some());

        let sse_entry = published_rx.recv().await.expect("SSE entry published");
        let sse_view = store.get_view(sse_entry.id).await.unwrap();
        assert_eq!(
            sse_view.view.request.http,
            Some(described(GenerationCall::GenerateStream))
        );
        assert!(store.get_preview(sse_entry.id).await.is_ok());
        assert!(store.get_raw(sse_entry.id).await.is_ok());
    }

    #[tokio::test]
    async fn stream_dropped_by_its_caller_is_recorded_as_cancelled_with_what_arrived() {
        let temp_dir = tempfile::tempdir().unwrap();
        let (published_tx, mut published_rx) = tokio::sync::mpsc::unbounded_channel();
        let store = Arc::new(LlmApiLogStore::with_publisher(
            temp_dir.path().to_path_buf(),
            move |entry| {
                let _ = published_tx.send(entry.clone());
            },
        ));
        store.set_stream_enabled(true);

        let repo = LoggingChatCompletionRepository::new(
            Arc::new(FakeChatCompletionRepository {
                stream_never_ends: true,
            }),
            Arc::clone(&store),
        );
        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let generation = tokio::spawn(async move {
            let payload = json!({
                "model": "gpt-5",
                "messages": [{ "role": "user", "content": "Ping" }]
            });
            repo.generate_stream(
                ChatCompletionSource::OpenAi,
                &test_config(),
                "/chat/completions",
                &payload,
                stream_tx,
                cancel_rx,
            )
            .await
        });
        assert!(stream_rx.recv().await.is_some());
        generation.abort();

        let entry = published_rx.recv().await.expect("entry published");
        assert_eq!(entry.level, "WARN");
        let response = store
            .get_view(entry.id)
            .await
            .unwrap()
            .view
            .response
            .unwrap();
        assert!(response.error.is_some());
        assert_eq!(response.candidates.len(), 1);
    }

    #[tokio::test]
    async fn stream_whose_response_file_cannot_be_written_is_not_published() {
        let temp_dir = tempfile::tempdir().unwrap();
        // A fresh store allocates id 1 first, so a directory at its SSE path fails the write.
        std::fs::create_dir(response_raw_sse_path(temp_dir.path(), 1)).unwrap();

        let (published_tx, mut published_rx) = tokio::sync::mpsc::unbounded_channel();
        let store = Arc::new(LlmApiLogStore::with_publisher(
            temp_dir.path().to_path_buf(),
            move |entry| {
                let _ = published_tx.send(entry.clone());
            },
        ));
        store.set_stream_enabled(true);

        let repo = LoggingChatCompletionRepository::new(
            Arc::new(FakeChatCompletionRepository::default()),
            Arc::clone(&store),
        );
        let config = test_config();
        let payload = json!({
            "model": "gpt-5",
            "messages": [{ "role": "user", "content": "Ping" }]
        });

        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        repo.generate_stream(
            ChatCompletionSource::OpenAi,
            &config,
            "/chat/completions",
            &payload,
            stream_tx,
            cancel_rx,
        )
        .await
        .unwrap();
        assert!(stream_rx.recv().await.is_some());

        repo.generate(
            ChatCompletionSource::OpenAi,
            &config,
            "/chat/completions",
            &payload,
        )
        .await
        .unwrap();

        let entry = published_rx.recv().await.expect("JSON entry published");
        assert!(!entry.stream);
        assert_eq!(store.tail_index(10), vec![entry]);
    }
}
