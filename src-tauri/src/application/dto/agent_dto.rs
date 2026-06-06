use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::models::agent::profile::{
    AgentContextPolicy, AgentPresetRef, AgentProfileDefinition, AgentProfileSummary,
};
use crate::domain::models::agent::{
    AgentChatRef, AgentRunEvent, AgentRunPresentation, AgentRunStatus, AgentTaskStatus,
    AgentToolSpec,
};
use crate::domain::repositories::agent_profile_storage_health_repository::{
    AgentProfileStorageIssue, AgentProfileStorageRepairAction,
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
    pub frozen_run_input_snapshot: Option<Value>,
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
pub struct AgentPreparePromptAssemblyDto {
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default = "default_generation_type")]
    pub generation_type: String,
    pub frozen_run_input_snapshot: Value,
    #[serde(default)]
    pub json_schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPreparePromptAssemblyResultDto {
    pub mode: AgentPromptAssemblyModeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<AgentPromptAssemblyBrokerRequestDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly: Option<AgentPromptAssemblyRequestMetadataDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentPromptAssemblyModeDto {
    CurrentPromptSnapshot,
    FrontendPromptAssembly,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptAssemblyBrokerRequestDto {
    pub schema_version: u32,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<AgentPromptAssemblyScopeDto>,
    pub profile_id: String,
    pub generation_type: String,
    pub frozen_run_input_snapshot: Value,
    pub settings: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub preset_ref: AgentPresetRef,
    pub agent_context_policy: AgentContextPolicy,
    pub agent_system_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_task_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_agent_prompt_components: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_schema: Option<Value>,
    pub fingerprint: AgentPromptAssemblyFingerprintDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptAssemblyFingerprintDto {
    pub preset_sha256: String,
    pub frozen_run_input_snapshot_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_task_prompt_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptAssemblyScopeDto {
    pub run_id: String,
    pub invocation_id: String,
    pub invocation_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_invocation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPromptAssemblyRequestMetadataDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly_id: Option<String>,
    pub schema_version: u32,
    pub engine: String,
    pub profile_id: String,
    pub preset_ref: AgentPresetRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<AgentPromptAssemblyScopeDto>,
    pub fingerprint: AgentPromptAssemblyFingerprintDto,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReadPromptAssemblyRequestDto {
    pub run_id: String,
    pub assembly_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSaveProfileDto {
    pub profile: AgentProfileDefinition,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRetargetPresetRefsDto {
    pub from: AgentPresetRef,
    pub to: AgentPresetRef,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRetargetPresetRefsResultDto {
    pub updated: usize,
    pub profile_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentListProfilesResultDto {
    pub profiles: Vec<AgentProfileSummary>,
    #[serde(default)]
    pub issues: Vec<AgentProfileStorageIssue>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRepairProfileFileDto {
    pub profile_id: String,
    pub action: AgentProfileStorageRepairAction,
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
    #[serde(default)]
    pub include_timeline_projection: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReadEventsResultDto {
    pub events: Vec<AgentRunEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_projection: Option<AgentRunTimelineProjectionDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunTimelineProjectionDto {
    pub foreground_invocation_ids: Vec<String>,
    pub handoff_edges: Vec<AgentRunTimelineHandoffEdgeDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunTimelineHandoffEdgeDto {
    pub task_id: String,
    pub source_invocation_id: String,
    pub new_invocation_id: String,
    pub target_profile_id: String,
    pub workspace_key: String,
    pub status: AgentTaskStatus,
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
    #[serde(default)]
    pub invocation_id: Option<String>,
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
pub struct AgentResolvePromptAssemblyDto {
    pub run_id: String,
    pub assembly_id: String,
    #[serde(default)]
    pub prompt_snapshot: Option<Value>,
    #[serde(default)]
    pub frozen_run_input_snapshot: Option<Value>,
    #[serde(default)]
    pub generation_intent: Option<Value>,
    #[serde(default)]
    pub assembly: Option<Value>,
    #[serde(default)]
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
