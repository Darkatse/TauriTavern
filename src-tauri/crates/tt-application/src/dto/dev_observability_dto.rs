use serde::{Deserialize, Serialize};

pub use tt_contracts::observability::{
    ChatCompletionRequestPreviewDto, ExchangeViewDto, LlmApiLogIndexEntryDto, LlmApiLogPreviewDto,
    LlmApiLogRawDto, LlmApiLogViewDto, LlmApiRawKindDto, MessageViewDto, ParameterViewDto,
    PartViewDto, RequestKindDto, RequestViewDto, ResponseViewDto, ToolViewDto, UsageViewDto,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendLogEntryDto {
    pub level: String,
    pub message: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendLogEntrySnapshotDto {
    pub id: u64,
    pub timestamp_ms: i64,
    pub level: String,
    pub message: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendLogEntryDto {
    pub id: u64,
    pub timestamp_ms: i64,
    pub level: String,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevBundleVersionDto {
    pub agent: String,
    #[serde(rename = "pkgVersion")]
    pub pkg_version: String,
    #[serde(rename = "tauriVersion")]
    pub tauri_version: String,
    #[serde(rename = "gitRevision")]
    pub git_revision: Option<String>,
    #[serde(rename = "gitBranch")]
    pub git_branch: Option<String>,
}
