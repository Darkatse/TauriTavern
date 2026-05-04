use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{AgentRunPresentation, ArtifactSpec};

pub const AGENT_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const AGENT_PROFILE_KIND: &str = "tauritavern.agentProfile";
pub const DEFAULT_AGENT_PROFILE_ID: &str = "default-writer";
pub const DEFAULT_AGENT_TOOL_MAX_ROUNDS: usize = 80;
pub const DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN: usize = 80;
pub const DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL: usize = 20_000;
pub const DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN: usize = 80_000;
pub const DEFAULT_AGENT_MODEL_MAX_RETRIES: usize = 3;
pub const DEFAULT_AGENT_MODEL_RETRY_INTERVAL_MS: u64 = 3_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentProfileId(String);

impl AgentProfileId {
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, String> {
        let raw = raw.as_ref().trim();
        if raw.is_empty() {
            return Err("agent.profile_id_empty: profile id cannot be empty".to_string());
        }
        if raw.len() > 128 {
            return Err("agent.profile_id_too_long: profile id must be <= 128 chars".to_string());
        }
        if !raw.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        }) {
            return Err(
                "agent.profile_id_invalid: profile id must use lowercase ASCII, digits, '-' or '_'"
                    .to_string(),
            );
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for AgentProfileId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AgentProfileId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProfileSummary {
    pub id: AgentProfileId,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProfileDefinition {
    pub schema_version: u32,
    pub kind: String,
    pub id: AgentProfileId,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub preset: AgentPresetBinding,
    pub model: AgentModelBinding,
    pub run: AgentRunPolicy,
    #[serde(default)]
    pub instructions: AgentProfileInstructions,
    pub tools: AgentToolPolicy,
    pub skills: AgentSkillPolicy,
    pub workspace: AgentWorkspacePolicy,
    pub plan: super::plan::AgentPlanPolicy,
    pub output: AgentOutputPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedAgentProfile {
    pub schema_version: u32,
    pub kind: String,
    pub id: AgentProfileId,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub preset: AgentPresetBinding,
    pub model: AgentModelBinding,
    pub run: AgentRunPolicy,
    pub instructions: AgentProfileInstructions,
    pub tools: AgentToolPolicy,
    pub skills: AgentSkillPolicy,
    pub workspace: AgentWorkspacePolicy,
    pub plan: super::plan::AgentPlanPolicy,
    pub output: ResolvedAgentOutputPolicy,
    pub source_trace: AgentProfileSourceTrace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProfileSourceTrace {
    pub profile_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPresetBinding {
    pub mode: AgentPresetBindingMode,
    #[serde(rename = "ref")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_: Option<AgentPresetRef>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentPresetBindingMode {
    CurrentPromptSnapshot,
    Ref,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPresetRef {
    pub api_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentModelBinding {
    pub mode: AgentModelBindingMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentModelBindingMode {
    CurrentPromptSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentRunPolicy {
    pub presentation: AgentRunPresentation,
    #[serde(default)]
    pub model_retry: AgentModelRetryPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentModelRetryPolicy {
    #[serde(default = "default_agent_model_max_retries")]
    pub max_retries: usize,
    #[serde(default = "default_agent_model_retry_interval_ms")]
    pub interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentProfileInstructions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_system_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentToolPolicy {
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default)]
    pub tool_descriptions: BTreeMap<String, AgentToolDescriptionOverride>,
    pub max_rounds: usize,
    #[serde(default = "default_agent_tool_max_calls_per_run")]
    pub max_calls_per_run: usize,
    #[serde(default)]
    pub max_calls_per_tool: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentToolDescriptionOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSkillPolicy {
    pub visible: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default = "default_agent_skill_max_read_chars_per_call")]
    pub max_read_chars_per_call: usize,
    #[serde(default = "default_agent_skill_max_read_chars_per_run")]
    pub max_read_chars_per_run: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentWorkspacePolicy {
    pub visible_roots: Vec<String>,
    pub writable_roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentOutputPolicy {
    pub artifacts: Vec<AgentOutputArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedAgentOutputPolicy {
    pub artifacts: Vec<ArtifactSpec>,
    pub message_body_artifact_id: String,
    pub message_body_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentOutputArtifact {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub target: AgentOutputArtifactTarget,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub assembly_order: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentOutputArtifactTarget {
    MessageBody,
}

impl AgentProfileDefinition {
    pub fn summary(&self) -> AgentProfileSummary {
        AgentProfileSummary {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            description: self.description.clone(),
        }
    }
}

fn default_agent_tool_max_calls_per_run() -> usize {
    DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN
}

fn default_agent_skill_max_read_chars_per_call() -> usize {
    DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL
}

fn default_agent_skill_max_read_chars_per_run() -> usize {
    DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN
}

fn default_agent_model_max_retries() -> usize {
    DEFAULT_AGENT_MODEL_MAX_RETRIES
}

fn default_agent_model_retry_interval_ms() -> u64 {
    DEFAULT_AGENT_MODEL_RETRY_INTERVAL_MS
}

impl Default for AgentModelRetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_AGENT_MODEL_MAX_RETRIES,
            interval_ms: DEFAULT_AGENT_MODEL_RETRY_INTERVAL_MS,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        AgentProfileDefinition, DEFAULT_AGENT_MODEL_MAX_RETRIES,
        DEFAULT_AGENT_MODEL_RETRY_INTERVAL_MS, DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL,
        DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN, DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN,
    };
    use crate::domain::models::agent::plan::DEFAULT_AGENT_PLAN_BETA;

    #[test]
    fn profile_definition_applies_schema_defaults_for_optional_fields() {
        let profile: AgentProfileDefinition = serde_json::from_value(json!({
            "schemaVersion": 1,
            "kind": "tauritavern.agentProfile",
            "id": "minimal",
            "displayName": "Minimal",
            "preset": {
                "mode": "currentPromptSnapshot"
            },
            "model": {
                "mode": "currentPromptSnapshot"
            },
            "run": {
                "presentation": "foreground"
            },
            "tools": {
                "allow": ["workspace.write_file", "workspace.commit", "workspace.finish"],
                "maxRounds": 80
            },
            "skills": {
                "visible": ["*"]
            },
            "workspace": {
                "visibleRoots": ["output"],
                "writableRoots": ["output"]
            },
            "plan": {
                "mode": "none"
            },
            "output": {
                "artifacts": [{
                    "id": "main",
                    "path": "output/main.md",
                    "kind": "markdown",
                    "target": "messageBody"
                }]
            }
        }))
        .expect("profile with optional fields omitted");

        assert!(!profile.preset.required);
        assert_eq!(
            profile.run.model_retry.max_retries,
            DEFAULT_AGENT_MODEL_MAX_RETRIES
        );
        assert_eq!(
            profile.run.model_retry.interval_ms,
            DEFAULT_AGENT_MODEL_RETRY_INTERVAL_MS
        );
        assert!(profile.instructions.agent_system_prompt.is_none());
        assert!(profile.tools.deny.is_empty());
        assert!(profile.tools.tool_descriptions.is_empty());
        assert_eq!(
            profile.tools.max_calls_per_run,
            DEFAULT_AGENT_TOOL_MAX_CALLS_PER_RUN
        );
        assert!(profile.tools.max_calls_per_tool.is_empty());
        assert!(profile.skills.deny.is_empty());
        assert_eq!(
            profile.skills.max_read_chars_per_call,
            DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_CALL
        );
        assert_eq!(
            profile.skills.max_read_chars_per_run,
            DEFAULT_AGENT_SKILL_MAX_READ_CHARS_PER_RUN
        );
        assert_eq!(profile.plan.beta, DEFAULT_AGENT_PLAN_BETA);
        assert!(profile.plan.nodes.is_empty());
    }
}
