use serde::Serialize;

use super::profile::AgentProfileId;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileHealth {
    pub profile_id: AgentProfileId,
    pub preview_available: bool,
    pub prompt_assembly_available: bool,
    pub direct_run_available: bool,
    pub sub_agent_available: bool,
    #[serde(default)]
    pub diagnostics: Vec<AgentProfileDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileDiagnostic {
    pub code: String,
    pub severity: AgentProfileDiagnosticSeverity,
    pub path: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<AgentProfileDiagnosticResource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<AgentProfileDiagnosticBlock>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repair_actions: Vec<AgentProfileDiagnosticRepairAction>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentProfileDiagnosticSeverity {
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentProfileDiagnosticBlock {
    Preview,
    PromptAssembly,
    DirectRun,
    SubAgent,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentProfileDiagnosticRepairAction {
    SelectPreset,
    SelectModel,
    SetModelRequiresConfiguration,
    OpenJsonEditor,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileDiagnosticResource {
    pub kind: AgentProfileDiagnosticResourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentProfileDiagnosticResourceKind {
    Preset,
    LlmConnection,
    Model,
}
