use serde::{Deserialize, Serialize};

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
pub(super) struct LlmApiLogMeta {
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
    pub request_raw_kind: LlmApiRawKind,
    pub response_raw_kind: Option<LlmApiRawKind>,
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
