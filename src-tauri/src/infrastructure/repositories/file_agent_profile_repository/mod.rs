use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::fs;
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::models::agent::profile::{
    AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentProfileDefinition, AgentProfileId,
    AgentProfileSummary,
};
use crate::domain::repositories::agent_profile_repository::AgentProfileRepository;
use crate::infrastructure::persistence::file_system::{
    list_files_with_extension, read_json_file, replace_file_with_fallback,
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

    async fn load_profile_file(&self, path: &Path) -> Result<AgentProfileDefinition, DomainError> {
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
        let profile: AgentProfileDefinition = read_json_file(path).await?;
        validate_profile_file_identity(&profile, &file_id, path)?;
        Ok(profile)
    }
}

#[async_trait]
impl AgentProfileRepository for FileAgentProfileRepository {
    async fn list_profiles(&self) -> Result<Vec<AgentProfileSummary>, DomainError> {
        let mut files = list_files_with_extension(&self.profiles_dir(), "json").await?;
        files.sort();

        let mut profiles = Vec::with_capacity(files.len());
        for path in files {
            profiles.push(self.load_profile_file(&path).await?.summary());
        }
        Ok(profiles)
    }

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

fn validate_profile_file_identity(
    profile: &AgentProfileDefinition,
    file_id: &AgentProfileId,
    path: &Path,
) -> Result<(), DomainError> {
    if profile.schema_version != AGENT_PROFILE_SCHEMA_VERSION {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use uuid::Uuid;

    use super::FileAgentProfileRepository;
    use crate::domain::models::agent::AgentRunPresentation;
    use crate::domain::models::agent::plan::{AgentPlanMode, AgentPlanPolicy};
    use crate::domain::models::agent::profile::{
        AGENT_PROFILE_KIND, AGENT_PROFILE_SCHEMA_VERSION, AgentModelBinding, AgentModelBindingMode,
        AgentOutputArtifact, AgentOutputArtifactTarget, AgentOutputPolicy, AgentPresetBinding,
        AgentPresetBindingMode, AgentProfileDefinition, AgentProfileId, AgentProfileInstructions,
        AgentRunPolicy, AgentSkillPolicy, AgentToolPolicy, AgentWorkspacePolicy,
    };
    use crate::domain::repositories::agent_profile_repository::AgentProfileRepository;

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
        let listed = repository.list_profiles().await.expect("list profiles");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id.as_str(), "writer");

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
            },
            run: AgentRunPolicy {
                presentation: AgentRunPresentation::Background,
            },
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
