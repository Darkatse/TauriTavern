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
    ChatCompletionApiConfig, ChatCompletionCancelReceiver, ChatCompletionRepository,
    ChatCompletionSource, ChatCompletionStreamSender,
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
    recent_meta: Mutex<VecDeque<LlmApiLogMeta>>,
    recent_raw: Mutex<VecDeque<LlmApiLogEntryRaw>>,
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
            recent_meta: Mutex::new(VecDeque::new()),
            recent_raw: Mutex::new(VecDeque::new()),
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
        if let Some(meta) = self
            .recent_meta
            .lock()
            .unwrap()
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
        {
            return Ok(meta.into());
        }

        Ok(load_meta(meta_path(&self.log_root, id)).await?.into())
    }

    pub async fn get_raw(&self, id: u64) -> Result<LlmApiLogEntryRaw, std::io::Error> {
        if let Some(entry) = self
            .recent_raw
            .lock()
            .unwrap()
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
        {
            return Ok(entry);
        }

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

    fn record_entry(
        &self,
        meta: LlmApiLogMeta,
        request_raw_inline: Option<String>,
        response_raw_inline: Option<String>,
    ) {
        let index_entry = LlmApiLogIndexEntry::from(&meta);
        let keep = self.keep.load(Ordering::Relaxed) as usize;
        let should_stream = self.stream_enabled.load(Ordering::Relaxed);
        let recent_raw_entry = request_raw_inline
            .as_ref()
            .map(|request_raw| LlmApiLogEntryRaw {
                id: meta.id,
                request_raw: request_raw.clone(),
                response_raw: response_raw_inline.clone().unwrap_or_default(),
                response_raw_kind: meta.response_raw_kind,
            });

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

        {
            let mut recent_meta = self.recent_meta.lock().unwrap();
            recent_meta.push_back(meta.clone());
            while recent_meta.len() > keep {
                recent_meta.pop_front();
            }
        }

        if let Some(recent_raw_entry) = recent_raw_entry {
            let mut recent_raw = self.recent_raw.lock().unwrap();
            recent_raw.push_back(recent_raw_entry);
            while recent_raw.len() > keep {
                recent_raw.pop_front();
            }
        }

        if should_stream {
            let _ = self.app_handle.emit(LLM_API_LOG_EVENT, index_entry);
        }

        let log_root = self.log_root.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = persist_entry_files(
                &log_root,
                &meta,
                request_raw_inline.as_deref(),
                response_raw_inline.as_deref(),
            )
            .await
            {
                tracing::error!("Failed to persist LLM API log entry {}: {}", meta.id, error);
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

        {
            let mut recent_meta = self.recent_meta.lock().unwrap();
            while recent_meta.len() > keep {
                recent_meta.pop_front();
            }
        }

        {
            let mut recent_raw = self.recent_raw.lock().unwrap();
            while recent_raw.len() > keep {
                recent_raw.pop_front();
            }
        }

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
    ) -> Result<Value, DomainError> {
        let started = Instant::now();
        let started_at_ms = chrono::Utc::now().timestamp_millis();

        let result = self
            .inner
            .generate(source, config, endpoint_path, payload)
            .await;

        let id = self.store.allocate_id();
        let duration_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;

        let (ok, level, error_message, response_value) = match &result {
            Ok(value) => (true, "INFO".to_string(), None, Some(value)),
            Err(error) => (false, "ERROR".to_string(), Some(error.to_string()), None),
        };

        let endpoint = format_endpoint(&config.base_url, endpoint_path);
        let model = extract_model(payload);

        let request_raw = pretty_json(payload);
        let request_readable = format_request_readable(source, payload);
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
            source: source.to_string(),
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
            .record_entry(meta, Some(request_raw), response_raw_inline);
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
        let model = extract_model(payload);

        let request_raw = pretty_json(payload);
        let request_readable = format_request_readable(source, payload);
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

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let forward_task = tauri::async_runtime::spawn(async move {
            let mut writer = response_writer;
            let mut readable = StreamReadableCollector::new(source);

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
        let (level, error_message) = if ok {
            ("INFO".to_string(), None)
        } else {
            (
                "ERROR".to_string(),
                result.as_ref().err().map(ToString::to_string),
            )
        };

        let meta = LlmApiLogMeta {
            id,
            timestamp_ms: started_at_ms,
            level,
            ok,
            source: source.to_string(),
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

        self.store.record_entry(meta, None, None);
        result
    }
}

impl ToString for ChatCompletionSource {
    fn to_string(&self) -> String {
        match self {
            ChatCompletionSource::OpenAi => "openai",
            ChatCompletionSource::OpenRouter => "openrouter",
            ChatCompletionSource::Custom => "custom",
            ChatCompletionSource::Claude => "claude",
            ChatCompletionSource::Makersuite => "makersuite",
            ChatCompletionSource::VertexAi => "vertexai",
            ChatCompletionSource::DeepSeek => "deepseek",
            ChatCompletionSource::Moonshot => "moonshot",
            ChatCompletionSource::SiliconFlow => "siliconflow",
            ChatCompletionSource::Zai => "zai",
        }
        .to_string()
    }
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

fn format_request_readable(source: ChatCompletionSource, payload: &Value) -> String {
    match source {
        ChatCompletionSource::Makersuite | ChatCompletionSource::VertexAi => {
            format_gemini_contents(payload)
        }
        ChatCompletionSource::Claude => format_claude_messages(payload),
        _ => format_openai_messages(payload),
    }
}

fn format_openai_messages(payload: &Value) -> String {
    let Some(messages) = payload.get("messages").and_then(Value::as_array) else {
        return pretty_json(payload);
    };

    let mut out = String::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        out.push_str(&format!("[{role}]\n"));
        out.push_str(&extract_message_content(message));
        out.push_str("\n\n");
    }
    out.trim().to_string()
}

fn format_claude_messages(payload: &Value) -> String {
    let Some(messages) = payload.get("messages").and_then(Value::as_array) else {
        return pretty_json(payload);
    };

    let mut out = String::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        out.push_str(&format!("[{role}]\n"));

        let content = message.get("content");
        out.push_str(&extract_claude_content(content));
        out.push_str("\n\n");
    }
    out.trim().to_string()
}

fn extract_claude_content(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };

    if let Some(text) = content.as_str() {
        return text.to_string();
    }

    let Some(blocks) = content.as_array() else {
        return pretty_json(content);
    };

    let mut out = String::new();
    for block in blocks {
        if let Some(text) = block.get("text").and_then(Value::as_str) {
            out.push_str(text);
        }
    }
    out
}

fn format_gemini_contents(payload: &Value) -> String {
    let Some(contents) = payload.get("contents").and_then(Value::as_array) else {
        return pretty_json(payload);
    };

    let mut out = String::new();
    for content in contents {
        let role = content
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        out.push_str(&format!("[{role}]\n"));
        let Some(parts) = content.get("parts").and_then(Value::as_array) else {
            out.push_str(&pretty_json(content));
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
    out.trim().to_string()
}

fn extract_message_content(message: &Value) -> String {
    let Some(content) = message.get("content") else {
        return String::new();
    };

    if let Some(text) = content.as_str() {
        return text.to_string();
    }

    let Some(items) = content.as_array() else {
        return pretty_json(content);
    };

    let mut out = String::new();
    for item in items {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
        if item_type == "text" {
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                out.push_str(text);
            }
            continue;
        }

        out.push_str(&format!("[{item_type}]"));
    }
    out
}

fn format_response_readable(response: &Value) -> String {
    let Some(choices) = response.get("choices").and_then(Value::as_array) else {
        return pretty_json(response);
    };
    let Some(first) = choices.first() else {
        return pretty_json(response);
    };
    let Some(message) = first.get("message").and_then(Value::as_object) else {
        return pretty_json(response);
    };
    if let Some(content) = message.get("content").and_then(Value::as_str) {
        return content.to_string();
    }
    pretty_json(response)
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

async fn persist_entry_files(
    log_root: &Path,
    meta: &LlmApiLogMeta,
    request_raw_inline: Option<&str>,
    response_raw_inline: Option<&str>,
) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(log_root).await?;

    if let Some(content) = request_raw_inline {
        tokio::fs::write(request_raw_path(log_root, meta.id), content).await?;
    }

    if let Some(kind) = meta.response_raw_kind {
        if let Some(content) = response_raw_inline {
            match kind {
                LlmApiRawKind::Json => {
                    tokio::fs::write(response_raw_json_path(log_root, meta.id), content).await?;
                }
                LlmApiRawKind::Sse => {
                    tokio::fs::write(response_raw_sse_path(log_root, meta.id), content).await?;
                }
            }
        }
    }

    let meta_json = serde_json::to_string_pretty(meta).map_err(|error| {
        std::io::Error::other(format!("Failed to serialize LLM API meta: {error}"))
    })?;
    tokio::fs::write(meta_path(log_root, meta.id), meta_json).await?;

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
    use super::format_endpoint;

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
}
