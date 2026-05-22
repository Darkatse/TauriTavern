use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::models::agent::profile::{
    AgentPresetRef, AgentProfileDefinition, AgentProfileSummary,
};
use crate::domain::models::agent::{
    AgentChatRef, AgentRunEvent, AgentRunPresentation, AgentRunStatus, AgentToolSpec,
};

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
    pub persist_base_state_id: Option<String>,
    #[serde(default)]
    pub prompt_snapshot: Option<Value>,
    #[serde(default)]
    pub generation_intent: Option<Value>,
    #[serde(default)]
    pub skill_scope_refs: AgentSkillScopeRefsDto,
    #[serde(default)]
    pub options: AgentStartRunOptionsDto,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkillScopeRefsDto {
    #[serde(default)]
    pub preset: Option<AgentPresetRef>,
    #[serde(default)]
    pub character_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileIdDto {
    pub profile_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResolveSystemPromptDto {
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResolveSystemPromptResultDto {
    pub agent_system_prompt: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSaveProfileDto {
    pub profile: AgentProfileDefinition,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentListProfilesResultDto {
    pub profiles: Vec<AgentProfileSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentListToolSpecsResultDto {
    pub tools: Vec<AgentToolSpec>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLoadProfileResultDto {
    pub profile: Option<AgentProfileDefinition>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentStartRunOptionsDto {
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub presentation: Option<AgentRunPresentation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunHandleDto {
    pub run_id: String,
    pub workspace_id: String,
    pub stable_chat_id: String,
    pub generation_type: String,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReadModelTurnDto {
    pub run_id: String,
    pub round: usize,
    #[serde(default = "default_model_turn_text_limit")]
    pub max_chars: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkspaceFileDto {
    pub path: String,
    pub text: String,
    pub chars: usize,
    pub words: usize,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelTurnDisplayDto {
    pub run_id: String,
    pub round: usize,
    pub model_response_path: String,
    pub provider: AgentModelTurnProviderDto,
    pub assistant: AgentModelTurnTextDto,
    pub reasoning: Vec<AgentModelTurnReasoningDto>,
    pub tool_calls: Vec<AgentModelTurnToolCallDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelTurnProviderDto {
    pub source: Option<String>,
    pub format: Option<String>,
    pub model: Option<String>,
    pub response_id: Option<String>,
    pub usage: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelTurnTextDto {
    pub text: String,
    pub total_chars: usize,
    pub total_words: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelTurnReasoningDto {
    pub source: String,
    pub text: String,
    pub total_chars: usize,
    pub total_words: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelTurnToolCallDto {
    pub call_id: String,
    pub name: String,
    pub model_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResolveChatCommitDto {
    pub run_id: String,
    pub commit_id: String,
    pub message_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResolvePersistentStateMetadataUpdateDto {
    pub run_id: String,
    pub update_id: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPruneChatPersistentStatesDto {
    pub chat_ref: AgentChatRef,
    #[serde(default, alias = "stableId")]
    pub stable_chat_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPruneChatPersistentStatesResultDto {
    pub workspace_id: String,
    pub removed_state_ids: Vec<String>,
}

fn default_generation_type() -> String {
    "normal".to_string()
}

fn default_event_limit() -> usize {
    100
}

fn default_model_turn_text_limit() -> usize {
    40_000
}
