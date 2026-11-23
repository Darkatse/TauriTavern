use serde::{Deserialize, Serialize};
use tt_contracts::observability::{
    LlmApiLogIndexEntryDto, LlmApiRawKindDto, RequestDescriptionDto, RequestKindDto,
};

pub(super) const TEXT_COMPLETION_SOURCE_PREFIX: &str = "text_completion:";

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
    pub response_raw_kind: Option<LlmApiRawKindDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<RequestDescriptionDto>,
}

impl LlmApiLogMeta {
    pub(super) fn request_kind(&self) -> RequestKindDto {
        if self.source.starts_with(TEXT_COMPLETION_SOURCE_PREFIX) {
            RequestKindDto::Text
        } else {
            RequestKindDto::Chat
        }
    }
}

impl From<&LlmApiLogMeta> for LlmApiLogIndexEntryDto {
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
