use std::borrow::Cow;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::domain::errors::DomainError;
use crate::domain::repositories::chat_completion_repository::{
    CHAT_COMPLETION_PROVIDER_STATE_FIELD, ChatCompletionApiConfig, ChatCompletionCancelReceiver,
    ChatCompletionRepository, ChatCompletionRepositoryGenerateResponse, ChatCompletionSource,
    ChatCompletionStreamSender,
};

pub const LLM_API_LOG_EVENT: &str = "tauritavern-llm-api-log";

const DEFAULT_KEEP: usize = 5;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmApiRawKind {
    Json,
    Sse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmApiLogIndexEntry {
    pub id: u64,
    pub timestamp_ms: i64,
    pub level: String,
    pub ok: bool,
    pub source: String,
    pub model: Option<String>,
    pub endpoint: String,
    pub duration_ms: u32,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmApiLogEntryPreview {
    pub id: u64,
    pub timestamp_ms: i64,
    pub level: String,
    pub ok: bool,
    pub source: String,
    pub model: Option<String>,
    pub endpoint: String,
    pub duration_ms: u32,
    pub stream: bool,
    pub error_message: Option<String>,
    pub request_readable: String,
    pub response_readable: String,
    pub response_raw_kind: Option<LlmApiRawKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmApiLogEntryRaw {
    pub id: u64,
    pub request_raw: String,
    pub response_raw: String,
    pub response_raw_kind: Option<LlmApiRawKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LlmApiLogMeta {
    id: u64,
    timestamp_ms: i64,
    level: String,
    ok: bool,
    source: String,
    model: Option<String>,
    endpoint: String,
    duration_ms: u32,
    stream: bool,
    error_message: Option<String>,
    request_readable: String,
    response_readable: String,
    request_raw_kind: LlmApiRawKind,
    response_raw_kind: Option<LlmApiRawKind>,
}

impl From<&LlmApiLogMeta> for LlmApiLogIndexEntry {
    fn from(meta: &LlmApiLogMeta) -> Self {
        Self {
            id: meta.id,
            timestamp_ms: meta.timestamp_ms,
            level: meta.level.clone(),
            ok: meta.ok,
            source: meta.source.clone(),
            model: meta.model.clone(),
            endpoint: meta.endpoint.clone(),
            duration_ms: meta.duration_ms,
            stream: meta.stream,
        }
    }
}

impl From<LlmApiLogMeta> for LlmApiLogEntryPreview {
    fn from(meta: LlmApiLogMeta) -> Self {
        Self {
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
            request_readable: meta.request_readable,
            response_readable: meta.response_readable,
            response_raw_kind: meta.response_raw_kind,
        }
    }
}

pub struct LlmApiLogStore {
    app_handle: AppHandle,
    log_root: PathBuf,
    next_id: AtomicU64,
    stream_enabled: AtomicBool,
    keep: AtomicU64,
    index: Mutex<VecDeque<LlmApiLogIndexEntry>>,
}

impl LlmApiLogStore {
    pub fn new(app_handle: AppHandle, log_root: PathBuf) -> Self {
        let mut index = VecDeque::new();
        let mut next_id = 1_u64;

        if let Ok(content) = std::fs::read_to_string(index_path(&log_root)) {
            if let Ok(entries) = serde_json::from_str::<Vec<LlmApiLogIndexEntry>>(&content) {
                for entry in entries {
                    next_id = next_id.max(entry.id.saturating_add(1));
                    index.push_back(entry);
                }
            }
        }

        Self {
            app_handle,
            log_root,
            next_id: AtomicU64::new(next_id),
            stream_enabled: AtomicBool::new(false),
            keep: AtomicU64::new(DEFAULT_KEEP as u64),
            index: Mutex::new(index),
        }
    }

    pub fn set_stream_enabled(&self, enabled: bool) {
        self.stream_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn apply_settings(&self, keep: u32) {
        let keep = keep as usize;
        self.keep.store(keep as u64, Ordering::Relaxed);
        self.enforce_keep_limit();

        let index_snapshot = {
            let index = self.index.lock().unwrap();
            index.iter().cloned().collect::<Vec<_>>()
        };
        let log_root = self.log_root.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = persist_index_file(&log_root, &index_snapshot).await {
                tracing::error!("Failed to persist LLM API log index: {}", error);
            }
        });
    }

    pub fn allocate_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn tail_index(&self, limit: usize) -> Vec<LlmApiLogIndexEntry> {
        let entries = self.index.lock().unwrap();
        let len = entries.len();
        let start = len.saturating_sub(limit);
        entries.iter().skip(start).cloned().collect()
    }

    pub async fn get_preview(&self, id: u64) -> Result<LlmApiLogEntryPreview, std::io::Error> {
        Ok(load_meta(meta_path(&self.log_root, id)).await?.into())
    }

    pub async fn get_raw(&self, id: u64) -> Result<LlmApiLogEntryRaw, std::io::Error> {
        let meta = load_meta(meta_path(&self.log_root, id)).await?;
        let request_raw = tokio::fs::read_to_string(request_raw_path(&self.log_root, id)).await?;

        let response_raw = match meta.response_raw_kind {
            Some(LlmApiRawKind::Json) => {
                tokio::fs::read_to_string(response_raw_json_path(&self.log_root, id)).await?
            }
            Some(LlmApiRawKind::Sse) => {
                tokio::fs::read_to_string(response_raw_sse_path(&self.log_root, id)).await?
            }
            None => String::new(),
        };

        Ok(LlmApiLogEntryRaw {
            id,
            request_raw,
            response_raw,
            response_raw_kind: meta.response_raw_kind,
        })
    }

    async fn record_entry(
        &self,
        meta: LlmApiLogMeta,
        request_raw_inline: Option<String>,
        response_raw_inline: Option<String>,
    ) {
        if let Err(error) = persist_meta_file(&self.log_root, &meta).await {
            tracing::error!(
                "Failed to persist LLM API meta entry {}: {}",
                meta.id,
                error
            );
            return;
        }

        let index_entry = LlmApiLogIndexEntry::from(&meta);
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
            let _ = self.app_handle.emit(LLM_API_LOG_EVENT, index_entry);
        }

        let log_root = self.log_root.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = persist_raw_files(
                &log_root,
                meta.id,
                request_raw_inline.as_deref(),
                meta.response_raw_kind,
                response_raw_inline.as_deref(),
            )
            .await
            {
                tracing::error!(
                    "Failed to persist LLM API log raw entry {}: {}",
                    meta.id,
                    error
                );
            }

            for removed_id in removed_ids {
                if let Err(error) = delete_entry_files(&log_root, removed_id).await {
                    tracing::warn!(
                        "Failed to delete old LLM API log entry {}: {}",
                        removed_id,
                        error
                    );
                }
            }

            if let Err(error) = persist_index_file(&log_root, &index_snapshot).await {
                tracing::error!("Failed to persist LLM API log index: {}", error);
            }
        });
    }

    fn enforce_keep_limit(&self) {
        let keep = self.keep.load(Ordering::Relaxed) as usize;
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

        if removed_ids.is_empty() {
            return;
        }

        let log_root = self.log_root.clone();
        tauri::async_runtime::spawn(async move {
            for removed_id in removed_ids {
                let _ = delete_entry_files(&log_root, removed_id).await;
            }
        });
    }
}

pub struct LoggingChatCompletionRepository {
    inner: Arc<dyn ChatCompletionRepository>,
    store: Arc<LlmApiLogStore>,
}

impl LoggingChatCompletionRepository {
    pub fn new(inner: Arc<dyn ChatCompletionRepository>, store: Arc<LlmApiLogStore>) -> Self {
        Self { inner, store }
    }
}

#[async_trait]
impl ChatCompletionRepository for LoggingChatCompletionRepository {
    async fn list_models(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
    ) -> Result<Value, DomainError> {
        self.inner.list_models(source, config).await
    }

    async fn generate(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
    ) -> Result<ChatCompletionRepositoryGenerateResponse, DomainError> {
        let started = Instant::now();
        let started_at_ms = chrono::Utc::now().timestamp_millis();

        let result = self
            .inner
            .generate(source, config, endpoint_path, payload)
            .await;

        let id = self.store.allocate_id();
        let duration_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;

        let (ok, level, error_message, response_value) = match &result {
            Ok(response) => (true, "INFO".to_string(), None, Some(&response.body)),
            Err(error) => {
                let level = if matches!(error, DomainError::Cancelled(_)) {
                    "WARN"
                } else {
                    "ERROR"
                };
                (false, level.to_string(), Some(error.to_string()), None)
            }
        };

        let endpoint = format_endpoint(&config.base_url, endpoint_path);
        let log_payload = wire_log_payload(payload);
        let model = extract_model(&log_payload);

        let request_raw = pretty_json(&log_payload);
        let request_readable = format_request_readable(source, &log_payload);
        let (response_readable, response_raw_inline, response_raw_kind) = match response_value {
            Some(value) => (
                format_response_readable(value),
                Some(pretty_json(value)),
                Some(LlmApiRawKind::Json),
            ),
            None => (error_message.clone().unwrap_or_default(), None, None),
        };

        let meta = LlmApiLogMeta {
            id,
            timestamp_ms: started_at_ms,
            level,
            ok,
            source: source.key().to_string(),
            model,
            endpoint,
            duration_ms,
            stream: false,
            error_message,
            request_readable,
            response_readable,
            request_raw_kind: LlmApiRawKind::Json,
            response_raw_kind,
        };

        self.store
            .record_entry(meta, Some(request_raw), response_raw_inline)
            .await;
        result
    }

    async fn generate_stream(
        &self,
        source: ChatCompletionSource,
        config: &ChatCompletionApiConfig,
        endpoint_path: &str,
        payload: &Value,
        sender: ChatCompletionStreamSender,
        cancel: ChatCompletionCancelReceiver,
    ) -> Result<(), DomainError> {
        let started = Instant::now();
        let started_at_ms = chrono::Utc::now().timestamp_millis();

        let id = self.store.allocate_id();
        let endpoint = format_endpoint(&config.base_url, endpoint_path);
        let log_payload = wire_log_payload(payload);
        let model = extract_model(&log_payload);

        let request_raw = pretty_json(&log_payload);
        let request_readable = format_request_readable(source, &log_payload);
        let request_path = request_raw_path(self.store.log_root.as_path(), id);
        tauri::async_runtime::spawn(async move {
            if let Err(error) = tokio::fs::write(&request_path, request_raw).await {
                tracing::error!(
                    "Failed to write LLM API request log file {}: {}",
                    request_path.display(),
                    error
                );
            }
        });

        let response_path = response_raw_sse_path(self.store.log_root.as_path(), id);
        let response_writer = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&response_path)
            .await;

        let response_writer = match response_writer {
            Ok(file) => Some(tokio::io::BufWriter::new(file)),
            Err(error) => {
                tracing::error!(
                    "Failed to open LLM API SSE log file {}: {}",
                    response_path.display(),
                    error
                );
                None
            }
        };
        let response_raw_kind = response_writer.as_ref().map(|_| LlmApiRawKind::Sse);

        let readable_source = stream_readable_source(source, endpoint_path);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let forward_task = tauri::async_runtime::spawn(async move {
            let mut writer = response_writer;
            let mut readable = StreamReadableCollector::new(readable_source);

            while let Some(chunk) = rx.recv().await {
                let _ = sender.send(chunk.clone());
                readable.push(&chunk);

                if let Some(writer_ref) = writer.as_mut() {
                    if tokio::io::AsyncWriteExt::write_all(writer_ref, chunk.as_bytes())
                        .await
                        .is_err()
                        || tokio::io::AsyncWriteExt::write_all(writer_ref, b"\n")
                            .await
                            .is_err()
                    {
                        writer = None;
                    }
                }
            }

            if let Some(mut writer) = writer {
                let _ = tokio::io::AsyncWriteExt::flush(&mut writer).await;
            }

            readable.into_string()
        });

        let result = self
            .inner
            .generate_stream(source, config, endpoint_path, payload, tx, cancel)
            .await;

        let response_readable = match forward_task.await {
            Ok(text) => text,
            Err(error) => format!("Stream forward task join failed: {error}"),
        };

        let duration_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
        let ok = result.is_ok();
        let (level, error_message) = match &result {
            Ok(()) => ("INFO".to_string(), None),
            Err(error) => {
                let level = if matches!(error, DomainError::Cancelled(_)) {
                    "WARN"
                } else {
                    "ERROR"
                };
                (level.to_string(), Some(error.to_string()))
            }
        };

        let meta = LlmApiLogMeta {
            id,
            timestamp_ms: started_at_ms,
            level,
            ok,
            source: source.key().to_string(),
            model,
            endpoint,
            duration_ms,
            stream: true,
            error_message,
            request_readable,
            response_readable,
            request_raw_kind: LlmApiRawKind::Json,
            response_raw_kind,
        };

        self.store.record_entry(meta, None, None).await;
        result
    }

    async fn close_provider_session(&self, session_id: &str) {
        self.inner.close_provider_session(session_id).await;
    }
}

fn stream_readable_source(
    source: ChatCompletionSource,
    endpoint_path: &str,
) -> ChatCompletionSource {
    if matches!(source, ChatCompletionSource::Custom) && endpoint_path.trim() == "/messages" {
        return ChatCompletionSource::Claude;
    }

    source
}

struct StreamReadableCollector {
    source: ChatCompletionSource,
    buffer: String,
}

impl StreamReadableCollector {
    fn new(source: ChatCompletionSource) -> Self {
        Self {
            source,
            buffer: String::new(),
        }
    }

    fn push(&mut self, chunk: &str) {
        let trimmed = chunk.trim();
        if trimmed.is_empty() || trimmed == "[DONE]" {
            return;
        }

        match self.source {
            ChatCompletionSource::Claude => self.push_claude(trimmed),
            ChatCompletionSource::Cohere => self.push_cohere(trimmed),
            ChatCompletionSource::Makersuite | ChatCompletionSource::VertexAi => {
                self.push_gemini_like(trimmed)
            }
            _ => self.push_openai_like(trimmed),
        }
    }

    fn push_openai_like(&mut self, trimmed: &str) {
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            return;
        };

        let choices = value.get("choices").and_then(Value::as_array);
        let Some(choices) = choices else {
            return;
        };

        for choice in choices {
            let delta = choice.get("delta").and_then(Value::as_object);
            let Some(delta) = delta else {
                continue;
            };

            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                self.buffer.push_str(text);
            }
        }
    }

    fn push_cohere(&mut self, trimmed: &str) {
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            return;
        };

        let message = value
            .get("delta")
            .and_then(Value::as_object)
            .and_then(|delta| delta.get("message"))
            .and_then(Value::as_object);
        let Some(message) = message else {
            return;
        };

        if let Some(text) = message
            .get("content")
            .and_then(Value::as_object)
            .and_then(|content| content.get("text"))
            .and_then(Value::as_str)
        {
            self.buffer.push_str(text);
            return;
        }

        if let Some(tool_plan) = message.get("tool_plan").and_then(Value::as_str) {
            self.buffer.push_str(tool_plan);
        }
    }

    fn push_claude(&mut self, trimmed: &str) {
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            return;
        };

        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if kind != "content_block_delta" {
            return;
        }

        let Some(delta) = value.get("delta").and_then(Value::as_object) else {
            return;
        };
        let delta_type = delta
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if delta_type != "text_delta" {
            return;
        }

        if let Some(text) = delta.get("text").and_then(Value::as_str) {
            self.buffer.push_str(text);
        }
    }

    fn push_gemini_like(&mut self, trimmed: &str) {
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            return;
        };

        let Some(candidates) = value.get("candidates").and_then(Value::as_array) else {
            return;
        };
        let Some(first) = candidates.first().and_then(Value::as_object) else {
            return;
        };
        let Some(content) = first.get("content").and_then(Value::as_object) else {
            return;
        };
        let Some(parts) = content.get("parts").and_then(Value::as_array) else {
            return;
        };
        for part in parts {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                self.buffer.push_str(text);
            }
        }
    }

    fn into_string(self) -> String {
        self.buffer
    }
}

fn format_endpoint(base_url: &str, endpoint_path: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let path = endpoint_path.trim();
    let joined = match (base.is_empty(), path.is_empty()) {
        (true, true) => String::new(),
        (false, true) => base.to_string(),
        (true, false) => path.to_string(),
        (false, false) if path.starts_with('/') => format!("{base}{path}"),
        (false, false) => format!("{base}/{path}"),
    };

    let Ok(mut url) = reqwest::Url::parse(&joined) else {
        return joined;
    };

    let _ = url.set_username("");
    let _ = url.set_password(None);

    let formatted = url.to_string();
    if path.is_empty() {
        formatted.trim_end_matches('/').to_string()
    } else {
        formatted
    }
}

fn extract_model(payload: &Value) -> Option<String> {
    payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn wire_log_payload(payload: &Value) -> Cow<'_, Value> {
    let Some(object) = payload.as_object() else {
        return Cow::Borrowed(payload);
    };
    if !object.contains_key(CHAT_COMPLETION_PROVIDER_STATE_FIELD) {
        return Cow::Borrowed(payload);
    }

    let mut object = object.clone();
    object.remove(CHAT_COMPLETION_PROVIDER_STATE_FIELD);
    Cow::Owned(Value::Object(object))
}

fn format_request_readable(source: ChatCompletionSource, payload: &Value) -> String {
    match source {
        ChatCompletionSource::Makersuite | ChatCompletionSource::VertexAi => {
            format_gemini_contents(payload)
        }
        ChatCompletionSource::Claude => format_claude_messages(payload),
        _ => format_openai_like_request(payload),
    }
}

fn format_openai_like_request(payload: &Value) -> String {
    if payload.get("messages").and_then(Value::as_array).is_some() {
        return format_openai_messages(payload);
    }

    if payload.get("input").is_some() {
        return format_input_items(payload);
    }

    format_openai_messages(payload)
}

fn format_input_items(payload: &Value) -> String {
    let Some(input) = payload.get("input") else {
        return "<input unavailable>".to_string();
    };

    let mut out = String::new();

    if let Some(system_instruction) = payload
        .get("system_instruction")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str("[system]\n");
        out.push_str(system_instruction);
        out.push_str("\n\n");
    }

    match input {
        Value::String(text) => {
            out.push_str("[user]\n");
            out.push_str(text);
        }
        Value::Array(items) => {
            for item in items {
                let Some(object) = item.as_object() else {
                    continue;
                };

                if let Some(role) = object.get("role").and_then(Value::as_str) {
                    out.push('[');
                    out.push_str(role);
                    out.push_str("]\n");
                    append_input_content(&mut out, object.get("content"));
                    out.push_str("\n\n");
                    continue;
                }

                if let Some(ty) = object.get("type").and_then(Value::as_str) {
                    if ty == "function_call_output" {
                        out.push_str("[tool");
                        if let Some(call_id) = object
                            .get("call_id")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                        {
                            out.push_str(" call_id=");
                            out.push_str(call_id);
                        }
                        out.push_str("]\n");
                        if let Some(output) = object.get("output") {
                            append_input_content(&mut out, Some(output));
                        }
                        out.push_str("\n\n");
                        continue;
                    }

                    out.push('[');
                    out.push_str(ty);
                    out.push_str("]\n");
                    append_input_content(&mut out, Some(item));
                    out.push_str("\n\n");
                }
            }
        }
        _ => return "<input unavailable>".to_string(),
    }

    out.truncate(out.trim_end().len());
    out
}

fn append_input_content(out: &mut String, content: Option<&Value>) {
    let Some(content) = content else {
        return;
    };

    if let Some(text) = content.as_str() {
        out.push_str(text);
        return;
    }

    if let Some(items) = content.as_array() {
        for item in items {
            if let Some(text) = item.as_str() {
                out.push_str(text);
                continue;
            }

            let Some(object) = item.as_object() else {
                continue;
            };

            let item_type = object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();

            if item_type == "text" {
                if let Some(text) = object.get("text").and_then(Value::as_str) {
                    out.push_str(text);
                }
                continue;
            }

            if item_type == "function_call" {
                let name = object.get("name").and_then(Value::as_str).unwrap_or("tool");
                out.push_str("\n[function_call ");
                out.push_str(name);
                out.push(']');
                if let Some(arguments) = object.get("arguments") {
                    if let Some(text) = arguments.as_str() {
                        if !text.trim().is_empty() {
                            out.push('\n');
                            out.push_str(text);
                        }
                    } else if !arguments.is_null() {
                        out.push('\n');
                        out.push_str(&arguments.to_string());
                    }
                }
                continue;
            }

            if item_type == "function_result" {
                let name = object.get("name").and_then(Value::as_str).unwrap_or("tool");
                out.push_str("\n[function_result ");
                out.push_str(name);
                out.push(']');
                if let Some(result) = object.get("result") {
                    if let Some(text) = result.as_str() {
                        if !text.trim().is_empty() {
                            out.push('\n');
                            out.push_str(text);
                        }
                    } else if !result.is_null() {
                        out.push('\n');
                        out.push_str(&result.to_string());
                    }
                }
                continue;
            }

            out.push('\n');
            out.push('[');
            out.push_str(item_type);
            out.push(']');
        }
        return;
    }

    out.push_str(&content.to_string());
}

fn format_openai_messages(payload: &Value) -> String {
    let Some(messages) = payload.get("messages").and_then(Value::as_array) else {
        return "<messages unavailable>".to_string();
    };

    let mut out = String::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        out.push('[');
        out.push_str(role);
        out.push_str("]\n");
        append_openai_message_content(&mut out, message);
        out.push_str("\n\n");
    }
    out.truncate(out.trim_end().len());
    out
}

fn format_claude_messages(payload: &Value) -> String {
    let Some(messages) = payload.get("messages").and_then(Value::as_array) else {
        return "<messages unavailable>".to_string();
    };

    let mut out = String::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        out.push('[');
        out.push_str(role);
        out.push_str("]\n");

        let content = message.get("content");
        append_claude_content(&mut out, content);
        out.push_str("\n\n");
    }
    out.truncate(out.trim_end().len());
    out
}

fn append_claude_content(out: &mut String, content: Option<&Value>) {
    let Some(content) = content else {
        return;
    };

    if let Some(text) = content.as_str() {
        out.push_str(text);
        return;
    }

    let Some(blocks) = content.as_array() else {
        return;
    };

    for block in blocks {
        if let Some(text) = block.get("text").and_then(Value::as_str) {
            out.push_str(text);
        }
    }
}

fn format_gemini_contents(payload: &Value) -> String {
    let Some(contents) = payload.get("contents").and_then(Value::as_array) else {
        return "<contents unavailable>".to_string();
    };

    let mut out = String::new();
    for content in contents {
        let role = content
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        out.push('[');
        out.push_str(role);
        out.push_str("]\n");
        let Some(parts) = content.get("parts").and_then(Value::as_array) else {
            out.push_str("\n\n");
            continue;
        };
        for part in parts {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                out.push_str(text);
            }
        }
        out.push_str("\n\n");
    }
    out.truncate(out.trim_end().len());
    out
}

fn append_openai_message_content(out: &mut String, message: &Value) {
    let Some(content) = message.get("content") else {
        return;
    };

    if let Some(text) = content.as_str() {
        out.push_str(text);
        return;
    }

    let Some(items) = content.as_array() else {
        return;
    };

    for item in items {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
        if item_type == "text" {
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                out.push_str(text);
            }
            continue;
        }

        out.push('[');
        out.push_str(item_type);
        out.push(']');
    }
}

fn format_response_readable(response: &Value) -> String {
    let Some(choices) = response.get("choices").and_then(Value::as_array) else {
        return "<response unavailable>".to_string();
    };
    let Some(first) = choices.first() else {
        return "<response unavailable>".to_string();
    };
    let Some(message) = first.get("message").and_then(Value::as_object) else {
        return "<response unavailable>".to_string();
    };
    if let Some(content) = message.get("content").and_then(Value::as_str) {
        return content.to_string();
    }
    "<non-text response>".to_string()
}

fn index_path(log_root: &Path) -> PathBuf {
    log_root.join("llm-api-index.json")
}

fn meta_path(log_root: &Path, id: u64) -> PathBuf {
    log_root.join(format!("llm-api-{id}.meta.json"))
}

fn request_raw_path(log_root: &Path, id: u64) -> PathBuf {
    log_root.join(format!("llm-api-{id}.request.json"))
}

fn response_raw_json_path(log_root: &Path, id: u64) -> PathBuf {
    log_root.join(format!("llm-api-{id}.response.json"))
}

fn response_raw_sse_path(log_root: &Path, id: u64) -> PathBuf {
    log_root.join(format!("llm-api-{id}.response.sse"))
}

async fn load_meta(path: PathBuf) -> Result<LlmApiLogMeta, std::io::Error> {
    let content = tokio::fs::read_to_string(path).await?;
    serde_json::from_str::<LlmApiLogMeta>(&content)
        .map_err(|error| std::io::Error::other(format!("Failed to parse meta JSON: {error}")))
}

async fn persist_meta_file(log_root: &Path, meta: &LlmApiLogMeta) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(log_root).await?;
    let meta_json = serde_json::to_string_pretty(meta).map_err(|error| {
        std::io::Error::other(format!("Failed to serialize LLM API meta: {error}"))
    })?;
    tokio::fs::write(meta_path(log_root, meta.id), meta_json).await?;
    Ok(())
}

async fn persist_raw_files(
    log_root: &Path,
    id: u64,
    request_raw_inline: Option<&str>,
    response_raw_kind: Option<LlmApiRawKind>,
    response_raw_inline: Option<&str>,
) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(log_root).await?;

    if let Some(content) = request_raw_inline {
        tokio::fs::write(request_raw_path(log_root, id), content).await?;
    }

    if let Some(kind) = response_raw_kind {
        if let Some(content) = response_raw_inline {
            match kind {
                LlmApiRawKind::Json => {
                    tokio::fs::write(response_raw_json_path(log_root, id), content).await?;
                }
                LlmApiRawKind::Sse => {
                    tokio::fs::write(response_raw_sse_path(log_root, id), content).await?;
                }
            }
        }
    }

    Ok(())
}

async fn delete_entry_files(log_root: &Path, id: u64) -> Result<(), std::io::Error> {
    for path in [
        meta_path(log_root, id),
        request_raw_path(log_root, id),
        response_raw_json_path(log_root, id),
        response_raw_sse_path(log_root, id),
    ] {
        if tokio::fs::remove_file(&path).await.is_err() {
            // Ignore missing files.
        }
    }
    Ok(())
}

async fn persist_index_file(
    log_root: &Path,
    index_snapshot: &[LlmApiLogIndexEntry],
) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(log_root).await?;
    let content = serde_json::to_string_pretty(index_snapshot).map_err(|error| {
        std::io::Error::other(format!("Failed to serialize LLM API index: {error}"))
    })?;
    tokio::fs::write(index_path(log_root), content).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::domain::repositories::chat_completion_repository::CHAT_COMPLETION_PROVIDER_STATE_FIELD;

    use super::{
        ChatCompletionSource, StreamReadableCollector, format_endpoint, format_request_readable,
        pretty_json, stream_readable_source, wire_log_payload,
    };

    #[test]
    fn format_endpoint_keeps_base_path() {
        assert_eq!(
            format_endpoint("https://example.com/v1", "/chat/completions"),
            "https://example.com/v1/chat/completions"
        );
    }

    #[test]
    fn format_endpoint_strips_userinfo_but_keeps_path() {
        assert_eq!(
            format_endpoint("https://user:pass@example.com/v1", "/messages"),
            "https://example.com/v1/messages"
        );
    }

    #[test]
    fn format_request_readable_supports_openai_responses_input_items() {
        let payload = json!({
            "model": "gpt-5",
            "input": [
                { "role": "developer", "content": "sys" },
                { "role": "user", "content": "hi" },
                { "type": "function_call_output", "call_id": "call_123", "output": "ok" }
            ],
            "store": false
        });

        let readable = format_request_readable(ChatCompletionSource::Custom, &payload);

        assert_eq!(
            readable,
            "[developer]\nsys\n\n[user]\nhi\n\n[tool call_id=call_123]\nok"
        );
    }

    #[test]
    fn wire_log_payload_strips_internal_provider_state() {
        let mut payload = json!({
            "model": "gpt-5",
            "input": [{ "role": "user", "content": "hi" }]
        });
        payload.as_object_mut().unwrap().insert(
            CHAT_COMPLETION_PROVIDER_STATE_FIELD.to_string(),
            json!({
                "sessionId": "run_123",
                "previousResponseId": "resp_123"
            }),
        );

        let payload = wire_log_payload(&payload);

        assert!(payload.get(CHAT_COMPLETION_PROVIDER_STATE_FIELD).is_none());
        assert!(!pretty_json(&payload).contains(CHAT_COMPLETION_PROVIDER_STATE_FIELD));
        assert_eq!(
            format_request_readable(ChatCompletionSource::Custom, &payload),
            "[user]\nhi"
        );
    }

    #[test]
    fn format_request_readable_supports_gemini_interactions_input_outputs() {
        let payload = json!({
            "model": "gemini-3",
            "system_instruction": "sys",
            "input": [
                { "role": "user", "content": "hi" },
                { "role": "model", "content": [
                    { "type": "text", "text": "hello" },
                    { "type": "function_call", "id": "call_1", "name": "get_weather", "arguments": { "location": "Paris" } }
                ]},
                { "role": "user", "content": [
                    { "type": "function_result", "name": "get_weather", "call_id": "call_1", "result": { "temp": 20 } }
                ]}
            ],
            "stream": true
        });

        let readable = format_request_readable(ChatCompletionSource::Custom, &payload);

        assert_eq!(
            readable,
            "[system]\nsys\n\n[user]\nhi\n\n[model]\nhello\n[function_call get_weather]\n{\"location\":\"Paris\"}\n\n[user]\n\n[function_result get_weather]\n{\"temp\":20}"
        );
    }

    #[test]
    fn stream_readable_source_maps_custom_messages_to_claude() {
        assert!(matches!(
            stream_readable_source(ChatCompletionSource::Custom, "/messages"),
            ChatCompletionSource::Claude
        ));
    }

    #[test]
    fn stream_readable_collector_collects_custom_claude_text_deltas() {
        let readable_source = stream_readable_source(ChatCompletionSource::Custom, "/messages");
        let mut collector = StreamReadableCollector::new(readable_source);

        collector
            .push(r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello"}}"#);
        collector.push(
            r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":" world"}}"#,
        );

        assert_eq!(collector.into_string(), "Hello world");
    }
}
