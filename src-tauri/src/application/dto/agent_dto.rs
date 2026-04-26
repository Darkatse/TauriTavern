use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::models::agent::{AgentChatRef, AgentRunEvent, AgentRunStatus, Checkpoint};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStartRunDto {
    pub chat_ref: AgentChatRef,
    #[serde(default, alias = "stableId")]
    pub stable_chat_id: String,
    #[serde(default = "default_generation_type")]
    pub generation_type: String,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub prompt_snapshot: Option<Value>,
    #[serde(default)]
    pub generation_intent: Option<Value>,
    #[serde(default)]
    pub options: AgentStartRunOptionsDto,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentStartRunOptionsDto {
    #[serde(default)]
    pub auto_commit: bool,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunHandleDto {
    pub run_id: String,
    pub workspace_id: String,
    pub stable_chat_id: String,
    pub status: AgentRunStatus,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCancelRunDto {
    pub run_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReadEventsDto {
    pub run_id: String,
    #[serde(default)]
    pub after_seq: Option<u64>,
    #[serde(default)]
    pub before_seq: Option<u64>,
    #[serde(default = "default_event_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReadEventsResultDto {
    pub events: Vec<AgentRunEvent>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReadWorkspaceFileDto {
    pub run_id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkspaceFileDto {
    pub path: String,
    pub text: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPrepareCommitDto {
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCommitDraftDto {
    pub run_id: String,
    pub stable_chat_id: String,
    pub chat_ref: AgentChatRef,
    pub generation_type: String,
    pub checkpoint: Checkpoint,
    pub message: AgentCommitMessageDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCommitMessageDto {
    pub mes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFinalizeCommitDto {
    pub run_id: String,
    #[serde(default)]
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCommitResultDto {
    pub run_id: String,
    pub status: AgentRunStatus,
}

fn default_generation_type() -> String {
    "normal".to_string()
}

fn default_event_limit() -> usize {
    100
}
