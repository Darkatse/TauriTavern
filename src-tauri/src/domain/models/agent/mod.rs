use std::path::Component;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::domain::errors::DomainError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum AgentChatRef {
    Character {
        character_id: String,
        file_name: String,
    },
    Group {
        chat_id: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Created,
    InitializingWorkspace,
    AssemblingContext,
    CallingModel,
    DispatchingTool,
    ApplyingWorkspacePatch,
    CreatingCheckpoint,
    AssemblingArtifacts,
    AwaitingCommit,
    Committing,
    Completed,
    Cancelling,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRun {
    pub id: String,
    pub workspace_id: String,
    pub stable_chat_id: String,
    pub chat_ref: AgentChatRef,
    pub generation_type: String,
    pub profile_id: Option<String>,
    pub status: AgentRunStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunEvent {
    pub seq: u64,
    pub id: String,
    pub run_id: String,
    pub timestamp: DateTime<Utc>,
    pub level: AgentRunEventLevel,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunEventLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolSpec {
    pub name: String,
    pub model_name: String,
    pub title: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    #[serde(default)]
    pub annotations: Value,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    #[serde(default)]
    pub provider_metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolResult {
    pub call_id: String,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub structured: Value,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default)]
    pub resource_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentModelRole {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum AgentModelContentPart {
    Text {
        text: String,
    },
    Reasoning {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default)]
        provider_metadata: Value,
    },
    ToolCall {
        call: AgentToolCall,
    },
    ToolResult {
        result: AgentToolResult,
    },
    Media {
        mime_type: String,
        value: Value,
    },
    ResourceRef {
        uri: String,
    },
    Native {
        provider: String,
        value: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelMessage {
    pub role: AgentModelRole,
    #[serde(default)]
    pub parts: Vec<AgentModelContentPart>,
    #[serde(default)]
    pub provider_metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelRequest {
    pub payload: Map<String, Value>,
    pub messages: Vec<AgentModelMessage>,
    pub tools: Vec<AgentToolSpec>,
    #[serde(default)]
    pub tool_choice: Value,
    #[serde(default)]
    pub provider_state: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelResponse {
    pub message: AgentModelMessage,
    pub tool_calls: Vec<AgentToolCall>,
    pub text: String,
    #[serde(default)]
    pub provider_metadata: Value,
    #[serde(default)]
    pub raw_response: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct WorkspacePath(String);

impl WorkspacePath {
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, DomainError> {
        let raw = raw.as_ref();
        if raw.is_empty() {
            return Err(DomainError::InvalidData(
                "Workspace path cannot be empty".to_string(),
            ));
        }
        if raw.contains('\0') {
            return Err(DomainError::InvalidData(
                "Workspace path cannot contain NUL".to_string(),
            ));
        }
        if raw.starts_with('/') || raw.starts_with('\\') {
            return Err(DomainError::InvalidData(
                "Workspace path must be relative".to_string(),
            ));
        }
        if raw.len() >= 2 && raw.as_bytes()[1] == b':' && raw.as_bytes()[0].is_ascii_alphabetic() {
            return Err(DomainError::InvalidData(
                "Workspace path cannot use a Windows drive prefix".to_string(),
            ));
        }

        let normalized = raw.replace('\\', "/");
        let path = std::path::Path::new(&normalized);
        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                Component::Normal(value) => {
                    let segment = value.to_string_lossy();
                    if segment.is_empty() {
                        continue;
                    }
                    parts.push(segment.to_string());
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    return Err(DomainError::InvalidData(
                        "Workspace path cannot contain ..".to_string(),
                    ));
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(DomainError::InvalidData(
                        "Workspace path must be relative".to_string(),
                    ));
                }
            }
        }

        if parts.is_empty() {
            return Err(DomainError::InvalidData(
                "Workspace path cannot be empty".to_string(),
            ));
        }

        Ok(Self(parts.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceManifest {
    pub workspace_version: u32,
    pub run_id: String,
    pub stable_chat_id: String,
    pub chat_ref: AgentChatRef,
    pub created_at: DateTime<Utc>,
    pub input: WorkspaceInputManifest,
    pub roots: Vec<WorkspaceRootSpec>,
    pub artifacts: Vec<ArtifactSpec>,
    pub commit_policy: CommitPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInputManifest {
    pub mode: String,
    pub prompt_snapshot_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRootSpec {
    pub path: String,
    pub lifecycle: WorkspaceRootLifecycle,
    pub scope: WorkspaceRootScope,
    pub mount: WorkspaceRootMount,
    pub visible: bool,
    pub writable: bool,
    pub commit: WorkspaceRootCommit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRootLifecycle {
    Run,
    Persistent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRootScope {
    Run,
    Chat,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRootMount {
    Materialized,
    ProjectedOverlay,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRootCommit {
    Never,
    OnRunCompleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSpec {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub target: ArtifactTarget,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub assembly_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactTarget {
    MessageBody,
    MessageExtra { key: String },
    CombinedMarkdown,
    HiddenRunArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitPolicy {
    pub default_target: ArtifactTarget,
    #[serde(default)]
    pub combine_template: Option<String>,
    #[serde(default)]
    pub store_artifacts_in_extra: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub id: String,
    pub seq: u64,
    pub run_id: String,
    pub created_at: DateTime<Utc>,
    pub reason: String,
    pub event_seq: u64,
    pub files: Vec<CheckpointFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointFile {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePersistentChangeSet {
    pub changes: Vec<WorkspacePersistentChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePersistentChange {
    pub path: String,
    pub kind: WorkspacePersistentChangeKind,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePersistentChangeKind {
    Added,
    Modified,
}

#[cfg(test)]
mod tests {
    use super::{AgentChatRef, WorkspacePath};
    use serde_json::json;

    #[test]
    fn chat_ref_accepts_frontend_abi_shape() {
        let ref_from_frontend: AgentChatRef = serde_json::from_value(json!({
            "kind": "character",
            "characterId": "Seraphina",
            "fileName": "chapter-1"
        }))
        .expect("frontend character ref");

        assert_eq!(
            ref_from_frontend,
            AgentChatRef::Character {
                character_id: "Seraphina".to_string(),
                file_name: "chapter-1".to_string(),
            }
        );

        let group_ref: AgentChatRef = serde_json::from_value(json!({
            "kind": "group",
            "chatId": "group-chat"
        }))
        .expect("frontend group ref");

        assert_eq!(
            group_ref,
            AgentChatRef::Group {
                chat_id: "group-chat".to_string(),
            }
        );
    }

    #[test]
    fn chat_ref_serializes_to_frontend_abi_shape() {
        let value = serde_json::to_value(AgentChatRef::Character {
            character_id: "Seraphina".to_string(),
            file_name: "chapter-1".to_string(),
        })
        .expect("serialize character ref");

        assert_eq!(
            value,
            json!({
                "kind": "character",
                "characterId": "Seraphina",
                "fileName": "chapter-1"
            })
        );
    }

    #[test]
    fn chat_ref_rejects_internal_field_names_at_abi_boundary() {
        let result = serde_json::from_value::<AgentChatRef>(json!({
            "kind": "character",
            "character_id": "Seraphina",
            "file_name": "chapter-1"
        }));

        assert!(result.is_err());
    }

    #[test]
    fn workspace_path_normalizes_relative_paths() {
        let path = WorkspacePath::parse("output/./main.md").expect("valid path");
        assert_eq!(path.as_str(), "output/main.md");
    }

    #[test]
    fn workspace_path_rejects_escape_paths() {
        assert!(WorkspacePath::parse("../secrets.json").is_err());
        assert!(WorkspacePath::parse("/tmp/file").is_err());
        assert!(WorkspacePath::parse("C:\\Users\\me\\file").is_err());
        assert!(WorkspacePath::parse("scratch/\0bad").is_err());
    }
}
