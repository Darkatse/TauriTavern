use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use tokio::fs;
use uuid::Uuid;

use tt_adapter_storage_core::file_system::{
    list_files_with_extension, read_json_file, replace_file_with_fallback,
};
use tt_domain::errors::DomainError;
use tt_domain::models::agent::profile::{
    AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentProfileDefinition, AgentProfileId,
};
use tt_ports::repositories::agent_profile_repository::AgentProfileRepository;
use tt_ports::repositories::agent_profile_storage_health_repository::{
    AgentProfileStorageHealthRepository, AgentProfileStorageIssue, AgentProfileStorageIssueKind,
    AgentProfileStorageRepairAction, AgentProfileStorageScan,
};

pub struct FileAgentProfileRepository {
    root: PathBuf,
}

impl FileAgentProfileRepository {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }

    fn staging_dir(&self) -> PathBuf {
        self.root.join(".staging")
    }

    fn profile_path(&self, id: &AgentProfileId) -> PathBuf {
        self.profiles_dir().join(format!("{}.json", id.as_str()))
    }

    fn profile_file_identity(&self, path: &Path) -> Result<(AgentProfileId, String), DomainError> {
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Agent profile filename is not valid UTF-8: {}",
                    path.display()
                ))
            })?
            .to_string();
        let file_id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                DomainError::InvalidData(format!(
                    "Agent profile filename is not valid UTF-8: {}",
                    path.display()
                ))
            })?;
        let file_id = AgentProfileId::parse(file_id).map_err(DomainError::InvalidData)?;
        Ok((file_id, file_name))
    }

    async fn load_profile_file(&self, path: &Path) -> Result<AgentProfileDefinition, DomainError> {
        let (file_id, _) = self.profile_file_identity(path)?;
        let profile: AgentProfileDefinition = read_json_file(path).await?;
        validate_profile_file_identity(&profile, &file_id, path)?;
        Ok(profile)
    }

    async fn scan_profile_file(
        &self,
        path: &Path,
    ) -> Result<AgentProfileStorageFileScan, DomainError> {
        let (file_id, file_name) = self.profile_file_identity(path)?;
        let content = match fs::read_to_string(path).await {
            Ok(content) => content,
            Err(error) if error.kind() == ErrorKind::InvalidData => {
                return Ok(AgentProfileStorageFileScan::Issue(profile_file_issue(
                    file_id,
                    file_name,
                    AgentProfileStorageIssueKind::InvalidJson,
                    format!("Agent profile file is not valid UTF-8 JSON text: {error}"),
                )));
            }
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to read Agent profile file {}: {}",
                    path.display(),
                    error
                )));
            }
        };
        let value = match serde_json::from_str::<Value>(&content) {
            Ok(value) => value,
            Err(error) => {
                return Ok(AgentProfileStorageFileScan::Issue(profile_file_issue(
                    file_id,
                    file_name,
                    AgentProfileStorageIssueKind::InvalidJson,
                    format!(
                        "Invalid Agent profile JSON in {}: {}",
                        path.display(),
                        error
                    ),
                )));
            }
        };
        match profile_from_value(value.clone(), &file_id, path) {
            Ok(profile) => Ok(AgentProfileStorageFileScan::Profile(profile)),
            Err(original_error) => {
                let mut normalized = value;
                let normalized_result =
                    normalize_profile_file_identity_value(&mut normalized, &file_id)
                        .and_then(|_| profile_from_value(normalized, &file_id, path));
                if normalized_result.is_ok() {
                    return Ok(AgentProfileStorageFileScan::Issue(profile_file_issue(
                        file_id,
                        file_name,
                        AgentProfileStorageIssueKind::InvalidFileIdentity,
                        original_error.to_string(),
                    )));
                }
                Ok(AgentProfileStorageFileScan::Issue(profile_file_issue(
                    file_id,
                    file_name,
                    AgentProfileStorageIssueKind::InvalidProfile,
                    original_error.to_string(),
                )))
            }
        }
    }
}

#[async_trait]
impl AgentProfileRepository for FileAgentProfileRepository {
    async fn load_profile(
        &self,
        id: &AgentProfileId,
    ) -> Result<Option<AgentProfileDefinition>, DomainError> {
        let path = self.profile_path(id);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(self.load_profile_file(&path).await?))
    }

    async fn save_profile(&self, profile: &AgentProfileDefinition) -> Result<(), DomainError> {
        validate_profile_file_identity(profile, &profile.id, &self.profile_path(&profile.id))?;

        fs::create_dir_all(self.profiles_dir())
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create Agent profile directory: {error}"
                ))
            })?;
        fs::create_dir_all(self.staging_dir())
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create Agent profile staging: {error}"
                ))
            })?;

        let target = self.profile_path(&profile.id);
        let temp = self.staging_dir().join(format!(
            "{}.{}.json",
            profile.id.as_str(),
            Uuid::new_v4().simple()
        ));
        let json = serde_json::to_string_pretty(profile).map_err(|error| {
            DomainError::InvalidData(format!("Failed to serialize Agent profile: {error}"))
        })?;
        fs::write(&temp, json.as_bytes()).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write Agent profile staging file {}: {}",
                temp.display(),
                error
            ))
        })?;
        replace_file_with_fallback(&temp, &target).await
    }

    async fn delete_profile(&self, id: &AgentProfileId) -> Result<(), DomainError> {
        let path = self.profile_path(id);
        fs::remove_file(&path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                DomainError::NotFound(format!("Agent profile not found: {}", id.as_str()))
            } else {
                DomainError::InternalError(format!(
                    "Failed to delete Agent profile {}: {}",
                    path.display(),
                    error
                ))
            }
        })
    }
}

#[async_trait]
impl AgentProfileStorageHealthRepository for FileAgentProfileRepository {
    async fn scan_profiles(&self) -> Result<AgentProfileStorageScan, DomainError> {
        let mut files = list_files_with_extension(&self.profiles_dir(), "json").await?;
        files.sort();

        let mut scan = AgentProfileStorageScan {
            profiles: Vec::with_capacity(files.len()),
            issues: Vec::new(),
        };
        for path in files {
            match self.scan_profile_file(&path).await? {
                AgentProfileStorageFileScan::Profile(profile) => {
                    scan.profiles.push(profile.summary())
                }
                AgentProfileStorageFileScan::Issue(issue) => scan.issues.push(issue),
            }
        }
        Ok(scan)
    }

    async fn normalize_profile_file_identity(
        &self,
        id: &AgentProfileId,
    ) -> Result<(), DomainError> {
        let path = self.profile_path(id);
        let content = fs::read_to_string(&path).await.map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                DomainError::NotFound(format!("Agent profile not found: {}", id.as_str()))
            } else {
                DomainError::InternalError(format!(
                    "Failed to read Agent profile file {}: {}",
                    path.display(),
                    error
                ))
            }
        })?;
        let mut value: Value = serde_json::from_str(&content).map_err(|error| {
            DomainError::InvalidData(format!(
                "Agent profile file {} is not valid JSON and cannot be identity-repaired: {}",
                path.display(),
                error
            ))
        })?;
        let changed = normalize_profile_file_identity_value(&mut value, id)?;
        profile_from_value(value.clone(), id, &path).map_err(|error| {
            DomainError::InvalidData(format!(
                "Agent profile file {} cannot be repaired without replacing profile content: {}",
                path.display(),
                error
            ))
        })?;
        if !changed {
            return Ok(());
        }
        self.write_profile_json(id, &value).await
    }
}

impl FileAgentProfileRepository {
    async fn write_profile_json(
        &self,
        id: &AgentProfileId,
        value: &Value,
    ) -> Result<(), DomainError> {
        fs::create_dir_all(self.profiles_dir())
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create Agent profile directory: {error}"
                ))
            })?;
        fs::create_dir_all(self.staging_dir())
            .await
            .map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to create Agent profile staging: {error}"
                ))
            })?;

        let target = self.profile_path(id);
        let temp =
            self.staging_dir()
                .join(format!("{}.{}.json", id.as_str(), Uuid::new_v4().simple()));
        let json = serde_json::to_string_pretty(value).map_err(|error| {
            DomainError::InvalidData(format!("Failed to serialize Agent profile repair: {error}"))
        })?;
        fs::write(&temp, json.as_bytes()).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to write Agent profile repair staging file {}: {}",
                temp.display(),
                error
            ))
        })?;
        replace_file_with_fallback(&temp, &target).await
    }
}

enum AgentProfileStorageFileScan {
    Profile(AgentProfileDefinition),
    Issue(AgentProfileStorageIssue),
}

fn profile_file_issue(
    profile_id: AgentProfileId,
    file_name: String,
    kind: AgentProfileStorageIssueKind,
    message: String,
) -> AgentProfileStorageIssue {
    let recommended_action = match kind {
        AgentProfileStorageIssueKind::InvalidJson => Some(AgentProfileStorageRepairAction::Delete),
        AgentProfileStorageIssueKind::InvalidFileIdentity => {
            Some(AgentProfileStorageRepairAction::NormalizeIdentity)
        }
        AgentProfileStorageIssueKind::InvalidProfile => None,
    };
    AgentProfileStorageIssue {
        profile_id,
        file_name,
        kind,
        recommended_action,
        message,
    }
}

fn normalize_profile_file_identity_value(
    value: &mut Value,
    id: &AgentProfileId,
) -> Result<bool, DomainError> {
    let object = value.as_object_mut().ok_or_else(|| {
        DomainError::InvalidData("Agent profile file must contain a JSON object".to_string())
    })?;

    let mut changed = false;
    if !object
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .is_some_and(|version| {
            u32::try_from(version)
                .ok()
                .is_some_and(is_supported_profile_schema_version)
        })
    {
        object.insert(
            "schemaVersion".to_string(),
            Value::from(AGENT_PROFILE_SCHEMA_VERSION),
        );
        changed = true;
    }
    if object.get("kind").and_then(Value::as_str) != Some(AGENT_PROFILE_KIND) {
        object.insert("kind".to_string(), Value::from(AGENT_PROFILE_KIND));
        changed = true;
    }
    if object.get("id").and_then(Value::as_str) != Some(id.as_str()) {
        object.insert("id".to_string(), Value::from(id.as_str()));
        changed = true;
    }
    Ok(changed)
}

fn profile_from_value(
    value: Value,
    file_id: &AgentProfileId,
    path: &Path,
) -> Result<AgentProfileDefinition, DomainError> {
    let profile: AgentProfileDefinition = serde_json::from_value(value)
        .map_err(|error| DomainError::InvalidData(error.to_string()))?;
    validate_profile_file_identity(&profile, file_id, path)?;
    Ok(profile)
}

fn validate_profile_file_identity(
    profile: &AgentProfileDefinition,
    file_id: &AgentProfileId,
    path: &Path,
) -> Result<(), DomainError> {
    if !is_supported_profile_schema_version(profile.schema_version) {
        return Err(DomainError::InvalidData(format!(
            "Unsupported Agent profile schemaVersion {} in {}",
            profile.schema_version,
            path.display()
        )));
    }
    if profile.kind != AGENT_PROFILE_KIND {
        return Err(DomainError::InvalidData(format!(
            "Invalid Agent profile kind `{}` in {}",
            profile.kind,
            path.display()
        )));
    }
    if profile.id != *file_id {
        return Err(DomainError::InvalidData(format!(
            "Agent profile id `{}` does not match file name `{}`",
            profile.id.as_str(),
            file_id.as_str()
        )));
    }
    Ok(())
}

fn is_supported_profile_schema_version(version: u32) -> bool {
    matches!(version, 1 | AGENT_PROFILE_SCHEMA_VERSION)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use uuid::Uuid;

    use super::FileAgentProfileRepository;
    use tt_domain::models::agent::AgentRunPresentation;
    use tt_domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy};
    use tt_domain::models::agent::profile::{
        AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentContextPolicy,
        AgentDelegationPolicy, AgentModelBinding, AgentModelBindingMode, AgentOutputArtifact,
        AgentOutputArtifactTarget, AgentOutputPolicy, AgentPresetBinding, AgentPresetBindingMode,
        AgentProfileDefinition, AgentProfileId, AgentProfileInstructions, AgentRunPolicy,
        AgentSkillPolicy, AgentToolPolicy, AgentWorkspacePolicy,
    };
    use tt_ports::repositories::agent_profile_repository::AgentProfileRepository;
    use tt_ports::repositories::agent_profile_storage_health_repository::{
        AgentProfileStorageHealthRepository, AgentProfileStorageIssueKind,
        AgentProfileStorageRepairAction,
    };

    #[tokio::test]
    async fn repository_round_trips_profile_files() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-agent-profile-repo-{}",
            Uuid::new_v4().simple()
        ));
        let repository = FileAgentProfileRepository::new(root.clone());
        let profile = sample_profile("writer");

        repository
            .save_profile(&profile)
            .await
            .expect("save profile");
        let listed = repository
            .scan_profiles()
            .await
            .expect("scan profile storage");
        assert_eq!(listed.profiles.len(), 1);
        assert_eq!(listed.profiles[0].id.as_str(), "writer");

        let loaded = repository
            .load_profile(&AgentProfileId::parse("writer").unwrap())
            .await
            .expect("load profile")
            .expect("profile exists");
        assert_eq!(loaded.id.as_str(), "writer");

        repository
            .delete_profile(&AgentProfileId::parse("writer").unwrap())
            .await
            .expect("delete profile");
        assert!(
            repository
                .load_profile(&AgentProfileId::parse("writer").unwrap())
                .await
                .expect("load missing")
                .is_none()
        );

        tokio::fs::remove_dir_all(root).await.expect("cleanup");
    }

    #[tokio::test]
    async fn repository_lists_valid_profiles_and_reports_repairable_file_issues() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-agent-profile-repo-{}",
            Uuid::new_v4().simple()
        ));
        let profiles_dir = root.join("profiles");
        tokio::fs::create_dir_all(&profiles_dir)
            .await
            .expect("create profiles dir");
        let repository = FileAgentProfileRepository::new(root.clone());
        repository
            .save_profile(&sample_profile("writer"))
            .await
            .expect("save profile");

        tokio::fs::write(profiles_dir.join("broken-json.json"), b"{")
            .await
            .expect("write broken json");
        let mut mismatched = sample_profile("mismatched");
        mismatched.id = AgentProfileId::parse("other").expect("profile id");
        tokio::fs::write(
            profiles_dir.join("mismatched.json"),
            serde_json::to_string_pretty(&mismatched).expect("serialize mismatched profile"),
        )
        .await
        .expect("write mismatched profile");

        let listed = repository
            .scan_profiles()
            .await
            .expect("scan profile storage");
        assert_eq!(listed.profiles.len(), 1);
        assert_eq!(listed.profiles[0].id.as_str(), "writer");
        assert_eq!(listed.issues.len(), 2);
        assert_eq!(listed.issues[0].profile_id.as_str(), "broken-json");
        assert_eq!(
            listed.issues[0].kind,
            AgentProfileStorageIssueKind::InvalidJson
        );
        assert_eq!(
            listed.issues[0].recommended_action,
            Some(AgentProfileStorageRepairAction::Delete)
        );
        assert_eq!(listed.issues[1].profile_id.as_str(), "mismatched");
        assert_eq!(
            listed.issues[1].kind,
            AgentProfileStorageIssueKind::InvalidFileIdentity
        );
        assert_eq!(
            listed.issues[1].recommended_action,
            Some(AgentProfileStorageRepairAction::NormalizeIdentity)
        );

        tokio::fs::remove_dir_all(root).await.expect("cleanup");
    }

    #[tokio::test]
    async fn repository_identity_repair_preserves_profile_content() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-agent-profile-repo-{}",
            Uuid::new_v4().simple()
        ));
        let profiles_dir = root.join("profiles");
        tokio::fs::create_dir_all(&profiles_dir)
            .await
            .expect("create profiles dir");
        let repository = FileAgentProfileRepository::new(root.clone());

        let mut profile = sample_profile("other");
        profile.schema_version = 99;
        profile.kind = "wrong.kind".to_string();
        profile.display_name = "Keep Me".to_string();
        profile.description = Some("Do not replace this profile body.".to_string());
        profile.tools.max_rounds = 7;
        tokio::fs::write(
            profiles_dir.join("writer.json"),
            serde_json::to_string_pretty(&profile).expect("serialize profile"),
        )
        .await
        .expect("write mismatched profile");

        repository
            .normalize_profile_file_identity(&AgentProfileId::parse("writer").unwrap())
            .await
            .expect("repair profile identity");

        let loaded = repository
            .load_profile(&AgentProfileId::parse("writer").unwrap())
            .await
            .expect("load repaired profile")
            .expect("profile exists");
        assert_eq!(loaded.schema_version, AGENT_PROFILE_SCHEMA_VERSION);
        assert_eq!(loaded.kind, AGENT_PROFILE_KIND);
        assert_eq!(loaded.id.as_str(), "writer");
        assert_eq!(loaded.display_name, "Keep Me");
        assert_eq!(
            loaded.description.as_deref(),
            Some("Do not replace this profile body.")
        );
        assert_eq!(loaded.tools.max_rounds, 7);

        tokio::fs::remove_dir_all(root).await.expect("cleanup");
    }

    #[tokio::test]
    async fn repository_identity_repair_refuses_to_replace_missing_profile_content() {
        let root = std::env::temp_dir().join(format!(
            "tauritavern-agent-profile-repo-{}",
            Uuid::new_v4().simple()
        ));
        let profiles_dir = root.join("profiles");
        tokio::fs::create_dir_all(&profiles_dir)
            .await
            .expect("create profiles dir");
        let repository = FileAgentProfileRepository::new(root.clone());
        let path = profiles_dir.join("writer.json");
        tokio::fs::write(
            &path,
            r#"{"schemaVersion":99,"kind":"wrong.kind","id":"writer","displayName":"Keep Me"}"#,
        )
        .await
        .expect("write incomplete profile");

        let error = repository
            .normalize_profile_file_identity(&AgentProfileId::parse("writer").unwrap())
            .await
            .expect_err("repair must not replace missing profile content");
        assert!(
            error
                .to_string()
                .contains("cannot be repaired without replacing profile content")
        );
        let raw = tokio::fs::read_to_string(&path)
            .await
            .expect("read unchanged profile");
        assert!(raw.contains(r#""kind":"wrong.kind""#));
        assert!(raw.contains(r#""schemaVersion":99"#));

        tokio::fs::remove_dir_all(root).await.expect("cleanup");
    }

    fn sample_profile(id: &str) -> AgentProfileDefinition {
        AgentProfileDefinition {
            schema_version: AGENT_PROFILE_SCHEMA_VERSION,
            kind: AGENT_PROFILE_KIND.to_string(),
            id: AgentProfileId::parse(id).expect("profile id"),
            display_name: "Writer".to_string(),
            description: None,
            preset: AgentPresetBinding {
                mode: AgentPresetBindingMode::CurrentPromptSnapshot,
                ref_: None,
                required: false,
            },
            model: AgentModelBinding {
                mode: AgentModelBindingMode::CurrentPromptSnapshot,
                connection_ref: None,
                model_id: None,
            },
            run: AgentRunPolicy {
                presentation: AgentRunPresentation::Background,
                direct_runnable: true,
                model_retry: Default::default(),
            },
            context: AgentContextPolicy::default(),
            delegation: AgentDelegationPolicy::default(),
            instructions: AgentProfileInstructions {
                agent_system_prompt: None,
            },
            tools: AgentToolPolicy {
                allow: vec!["workspace.finish".to_string()],
                deny: Vec::new(),
                tool_descriptions: BTreeMap::new(),
                max_rounds: 1,
                max_calls_per_run: 1,
                max_calls_per_tool: BTreeMap::new(),
            },
            skills: AgentSkillPolicy {
                visible: vec!["*".to_string()],
                deny: Vec::new(),
                max_read_chars_per_call: 1,
                max_read_chars_per_run: 1,
            },
            workspace: AgentWorkspacePolicy {
                visible_roots: vec!["output".to_string()],
                writable_roots: vec!["output".to_string()],
            },
            plan: AgentPlanPolicy {
                mode: AgentPlanMode::None,
                beta: true,
                nodes: Vec::new(),
            },
            output: AgentOutputPolicy {
                artifacts: vec![AgentOutputArtifact {
                    id: "main".to_string(),
                    path: "output/main.md".to_string(),
                    kind: "markdown".to_string(),
                    target: AgentOutputArtifactTarget::MessageBody,
                    required: true,
                    assembly_order: 0,
                }],
            },
        }
    }
}
