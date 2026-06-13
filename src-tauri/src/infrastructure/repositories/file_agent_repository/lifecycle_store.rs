use std::collections::BTreeSet;
use std::path::Path;

use async_trait::async_trait;
use tokio::fs;

use super::FileAgentRepository;
use super::fs_tree::should_skip_platform_metadata_file;
use super::paths::validate_segment;
use crate::domain::errors::DomainError;
use crate::domain::repositories::agent_workspace_lifecycle_repository::{
    AgentChatWorkspaceDeletion, AgentPersistentStatePrune, AgentPersistentStatePruneRequest,
    AgentWorkspaceLifecycleRepository,
};

#[async_trait]
impl AgentWorkspaceLifecycleRepository for FileAgentRepository {
    async fn delete_chat_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<AgentChatWorkspaceDeletion, DomainError> {
        validate_segment(workspace_id, "workspace_id")?;
        let chat_dir = self.chat_dir(workspace_id)?;
        let metadata = match fs::symlink_metadata(&chat_dir).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AgentChatWorkspaceDeletion {
                    workspace_id: workspace_id.to_string(),
                    removed: false,
                    run_ids: Vec::new(),
                });
            }
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect agent chat workspace {}: {}",
                    chat_dir.display(),
                    error
                )));
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "Agent chat workspace is not a directory: {}",
                chat_dir.display()
            )));
        }

        let run_ids = self.run_ids_in_chat_workspace(&chat_dir).await?;
        fs::remove_dir_all(&chat_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to delete agent chat workspace {}: {}",
                chat_dir.display(),
                error
            ))
        })?;

        for run_id in &run_ids {
            let index_path = self.index_run_path(run_id)?;
            match fs::remove_file(&index_path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to delete agent run index {}: {}",
                        index_path.display(),
                        error
                    )));
                }
            }

            let summary_path = self.index_run_summary_path(run_id)?;
            match fs::remove_file(&summary_path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to delete agent run summary {}: {}",
                        summary_path.display(),
                        error
                    )));
                }
            }
        }

        Ok(AgentChatWorkspaceDeletion {
            workspace_id: workspace_id.to_string(),
            removed: true,
            run_ids,
        })
    }

    async fn prune_persistent_states(
        &self,
        workspace_id: &str,
        request: AgentPersistentStatePruneRequest,
    ) -> Result<AgentPersistentStatePrune, DomainError> {
        validate_segment(workspace_id, "workspace_id")?;
        let mut retained = BTreeSet::new();
        for state_id in &request.retained_state_ids {
            validate_segment(state_id, "persist_state_id")?;
            retained.insert(state_id.as_str());
        }
        let mut candidates = BTreeSet::new();
        for state_id in &request.candidate_state_ids {
            validate_segment(state_id, "persist_state_id")?;
            candidates.insert(state_id.as_str());
        }
        if candidates.is_empty() {
            return Ok(AgentPersistentStatePrune {
                workspace_id: workspace_id.to_string(),
                removed_state_ids: Vec::new(),
            });
        }

        let states_dir = self.chat_dir(workspace_id)?.join("persistent-states");
        let metadata = match fs::symlink_metadata(&states_dir).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AgentPersistentStatePrune {
                    workspace_id: workspace_id.to_string(),
                    removed_state_ids: Vec::new(),
                });
            }
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect persistent states directory {}: {}",
                    states_dir.display(),
                    error
                )));
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "Persistent states path is not a directory: {}",
                states_dir.display()
            )));
        }

        let mut removed_state_ids = Vec::new();
        for state_id in candidates {
            if retained.contains(state_id) {
                continue;
            }
            let state_dir = self.persistent_state_dir(workspace_id, state_id)?;
            let metadata = match fs::symlink_metadata(&state_dir).await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(DomainError::InternalError(format!(
                        "Failed to inspect persistent state {}: {}",
                        state_dir.display(),
                        error
                    )));
                }
            };
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(DomainError::InvalidData(format!(
                    "Persistent state path is not a directory: {}",
                    state_dir.display()
                )));
            }

            fs::remove_dir_all(&state_dir).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to delete persistent state {}: {}",
                    state_dir.display(),
                    error
                ))
            })?;
            removed_state_ids.push(state_id.to_string());
        }

        Ok(AgentPersistentStatePrune {
            workspace_id: workspace_id.to_string(),
            removed_state_ids,
        })
    }
}

impl FileAgentRepository {
    async fn run_ids_in_chat_workspace(&self, chat_dir: &Path) -> Result<Vec<String>, DomainError> {
        let runs_dir = chat_dir.join("runs");
        let metadata = match fs::symlink_metadata(&runs_dir).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(DomainError::InternalError(format!(
                    "Failed to inspect agent runs directory {}: {}",
                    runs_dir.display(),
                    error
                )));
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(DomainError::InvalidData(format!(
                "Agent runs path is not a directory: {}",
                runs_dir.display()
            )));
        }

        let mut entries = fs::read_dir(&runs_dir).await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read agent runs directory {}: {}",
                runs_dir.display(),
                error
            ))
        })?;
        let mut run_ids = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|error| {
            DomainError::InternalError(format!(
                "Failed to read agent runs directory entry {}: {}",
                runs_dir.display(),
                error
            ))
        })? {
            let metadata = fs::symlink_metadata(entry.path()).await.map_err(|error| {
                DomainError::InternalError(format!(
                    "Failed to inspect agent run directory {}: {}",
                    entry.path().display(),
                    error
                ))
            })?;
            if should_skip_platform_metadata_file(&entry.path(), &metadata)? {
                continue;
            }
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(DomainError::InvalidData(format!(
                    "Agent run path is not a directory: {}",
                    entry.path().display()
                )));
            }
            let run_id = entry
                .file_name()
                .into_string()
                .map_err(|_| DomainError::InvalidData("Agent run id is not UTF-8".to_string()))?;
            validate_segment(&run_id, "run_id")?;
            run_ids.push(run_id);
        }
        run_ids.sort();
        Ok(run_ids)
    }
}
