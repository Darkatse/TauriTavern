use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum SkillScope {
    Global,
    Preset { api_id: String, name: String },
    Profile { profile_id: String },
    Character { character_id: String },
}

impl Default for SkillScope {
    fn default() -> Self {
        Self::Global
    }
}

impl SkillScope {
    pub fn stable_key(&self) -> String {
        match self {
            Self::Global => "global".to_string(),
            Self::Preset { api_id, name } => format!("preset\0{api_id}\0{name}"),
            Self::Profile { profile_id } => format!("profile\0{profile_id}"),
            Self::Character { character_id } => format!("character\0{character_id}"),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Global => "global".to_string(),
            Self::Preset { api_id, name } => format!("preset:{api_id}:{name}"),
            Self::Profile { profile_id } => format!("profile:{profile_id}"),
            Self::Character { character_id } => format!("character:{character_id}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum SkillScopeFilter {
    All,
    Global,
    Preset { api_id: String, name: String },
    Profile { profile_id: String },
    Character { character_id: String },
}

impl Default for SkillScopeFilter {
    fn default() -> Self {
        Self::Global
    }
}

impl SkillScopeFilter {
    pub fn matches(&self, scope: &SkillScope) -> bool {
        match self {
            Self::All => true,
            Self::Global => matches!(scope, SkillScope::Global),
            Self::Preset { api_id, name } => matches!(
                scope,
                SkillScope::Preset {
                    api_id: scope_api_id,
                    name: scope_name,
                } if scope_api_id == api_id && scope_name == name
            ),
            Self::Profile { profile_id } => matches!(
                scope,
                SkillScope::Profile {
                    profile_id: scope_profile_id,
                } if scope_profile_id == profile_id
            ),
            Self::Character { character_id } => matches!(
                scope,
                SkillScope::Character {
                    character_id: scope_character_id,
                } if scope_character_id == character_id
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillIndexEntry {
    #[serde(default)]
    pub scope: SkillScope,
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub installed_hash: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub has_scripts: bool,
    pub has_binary: bool,
    pub installed_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SkillSourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSourceRef {
    pub kind: String,
    pub id: String,
    pub label: String,
    pub installed_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillFileRef {
    pub path: String,
    pub kind: SkillFileKind,
    pub media_type: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillFileKind {
    Text,
    Binary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillImportPreview {
    pub skill: SkillIndexEntry,
    pub files: Vec<SkillFileRef>,
    pub conflict: SkillImportConflict,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub source: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillImportConflict {
    pub kind: SkillImportConflictKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillImportConflictKind {
    New,
    Same,
    Different,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillInstallConflictStrategy {
    Skip,
    Replace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillInstallRequest {
    pub input: SkillImportInput,
    #[serde(default)]
    pub target_scope: SkillScope,
    #[serde(default)]
    pub conflict_strategy: Option<SkillInstallConflictStrategy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillInstallResult {
    pub scope: SkillScope,
    pub name: String,
    pub action: SkillInstallAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<SkillIndexEntry>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillInstallAction {
    Installed,
    Replaced,
    AlreadyInstalled,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SkillImportInput {
    InlineFiles {
        files: Vec<SkillInlineFile>,
        #[serde(default)]
        source: Value,
    },
    Directory {
        path: String,
        #[serde(default)]
        source: Value,
    },
    ArchiveFile {
        path: String,
        #[serde(default)]
        source: Value,
    },
    ArchiveBase64 {
        file_name: String,
        content_base64: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
        #[serde(default)]
        source: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillInlineFile {
    pub path: String,
    #[serde(default = "default_inline_encoding")]
    pub encoding: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillReadRequest {
    #[serde(default)]
    pub scope: SkillScope,
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_char: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillWriteRequest {
    #[serde(default)]
    pub scope: SkillScope,
    pub name: String,
    pub path: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillReadResult {
    pub scope: SkillScope,
    pub name: String,
    pub path: String,
    pub content: String,
    pub chars: usize,
    pub total_chars: usize,
    pub start_char: usize,
    pub end_char: usize,
    pub total_lines: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub bytes: u64,
    pub sha256: String,
    pub truncated: bool,
    pub resource_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSearchRequest {
    #[serde(default)]
    pub scope: SkillScope,
    pub name: String,
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub limit: usize,
    pub context_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSearchHit {
    pub path: String,
    pub score: f32,
    pub start_line: usize,
    pub end_line: usize,
    pub snippet: String,
    pub bytes: u64,
    pub sha256: String,
    pub resource_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSearchResult {
    pub scope: SkillScope,
    pub name: String,
    pub query: String,
    pub hits: Vec<SkillSearchHit>,
    pub searched_files: usize,
    pub skipped_files: usize,
    pub truncated: bool,
    pub returned_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillExportResult {
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillMoveRequest {
    pub name: String,
    pub from_scope: SkillScope,
    pub to_scope: SkillScope,
    #[serde(default)]
    pub conflict_strategy: Option<SkillInstallConflictStrategy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillScopeRetargetRequest {
    pub from_scope: SkillScope,
    pub to_scope: SkillScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillScopeRetargetResult {
    pub moved: usize,
    pub merged: usize,
    /// Number of Skill index entries whose source_refs were rewritten.
    pub source_refs_updated: usize,
}

fn default_inline_encoding() -> String {
    "utf8".to_string()
}
