use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::application::errors::ApplicationError;
use crate::application::services::agent_identity::{
    validate_stable_chat_id, workspace_id_for_stable_chat_id,
};
use crate::domain::models::agent::AgentChatRef;
use crate::domain::repositories::agent_workspace_lifecycle_repository::{
    AgentChatWorkspaceDeletion, AgentPersistentStatePrune, AgentPersistentStatePruneRequest,
    AgentWorkspaceLifecycleRepository,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentChatWorkspaceTarget {
    pub chat_ref: AgentChatRef,
    pub stable_chat_id: String,
}

#[async_trait]
pub trait AgentRunActivity: Send + Sync {
    async fn active_run_ids(&self) -> Result<Vec<String>, ApplicationError>;

    async fn active_run_ids_for_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<String>, ApplicationError>;
}

pub struct AgentWorkspaceLifecycleService {
    repository: Arc<dyn AgentWorkspaceLifecycleRepository>,
    run_activity: Arc<dyn AgentRunActivity>,
}

impl AgentWorkspaceLifecycleService {
    pub fn new(
        repository: Arc<dyn AgentWorkspaceLifecycleRepository>,
        run_activity: Arc<dyn AgentRunActivity>,
    ) -> Self {
        Self {
            repository,
            run_activity,
        }
    }

    pub fn character_target_from_metadata(
        character_id: &str,
        file_name: &str,
        metadata: &Value,
    ) -> Result<Option<AgentChatWorkspaceTarget>, ApplicationError> {
        let Some(value) = metadata.get("integrity") else {
            return Ok(None);
        };
        if value.is_null() {
            return Ok(None);
        }
        let Some(stable_chat_id) = value.as_str() else {
            return Err(ApplicationError::ValidationError(
                "agent.invalid_chat_integrity: chat_metadata.integrity must be a string"
                    .to_string(),
            ));
        };
        let stable_chat_id = stable_chat_id.trim();
        if stable_chat_id.is_empty() {
            return Ok(None);
        }

        Ok(Some(AgentChatWorkspaceTarget {
            chat_ref: AgentChatRef::Character {
                character_id: character_id.to_string(),
                file_name: file_name.to_string(),
            },
            stable_chat_id: validate_stable_chat_id(stable_chat_id)?,
        }))
    }

    pub fn group_target(chat_id: &str) -> Result<AgentChatWorkspaceTarget, ApplicationError> {
        let stable_chat_id = validate_stable_chat_id(chat_id)?;
        Ok(AgentChatWorkspaceTarget {
            chat_ref: AgentChatRef::Group {
                chat_id: stable_chat_id.clone(),
            },
            stable_chat_id,
        })
    }

    pub async fn ensure_chat_workspace_inactive(
        &self,
        target: &AgentChatWorkspaceTarget,
    ) -> Result<(), ApplicationError> {
        let workspace_id = self.workspace_id(target)?;
        let active_run_ids = self
            .run_activity
            .active_run_ids_for_workspace(&workspace_id)
            .await?;
        if !active_run_ids.is_empty() {
            return Err(ApplicationError::ValidationError(format!(
                "agent.workspace_in_use: workspace `{workspace_id}` has active runs: {}",
                active_run_ids.join(", ")
            )));
        }
        Ok(())
    }

    pub async fn ensure_chat_workspaces_inactive(
        &self,
        targets: &[AgentChatWorkspaceTarget],
    ) -> Result<(), ApplicationError> {
        for target in targets {
            self.ensure_chat_workspace_inactive(target).await?;
        }
        Ok(())
    }

    pub async fn delete_chat_workspace(
        &self,
        target: &AgentChatWorkspaceTarget,
    ) -> Result<AgentChatWorkspaceDeletion, ApplicationError> {
        self.ensure_chat_workspace_inactive(target).await?;
        let workspace_id = self.workspace_id(target)?;
        self.repository
            .delete_chat_workspace(&workspace_id)
            .await
            .map_err(Into::into)
    }

    pub async fn delete_chat_workspaces(
        &self,
        targets: &[AgentChatWorkspaceTarget],
    ) -> Result<Vec<AgentChatWorkspaceDeletion>, ApplicationError> {
        self.ensure_chat_workspaces_inactive(targets).await?;
        let mut deletions = Vec::with_capacity(targets.len());
        for target in targets {
            let workspace_id = self.workspace_id(target)?;
            deletions.push(
                self.repository
                    .delete_chat_workspace(&workspace_id)
                    .await
                    .map_err(ApplicationError::from)?,
            );
        }
        Ok(deletions)
    }

    pub async fn prune_persistent_states(
        &self,
        target: &AgentChatWorkspaceTarget,
        request: AgentPersistentStatePruneRequest,
    ) -> Result<AgentPersistentStatePrune, ApplicationError> {
        self.ensure_chat_workspace_inactive(target).await?;
        let workspace_id = self.workspace_id(target)?;
        self.repository
            .prune_persistent_states(&workspace_id, request)
            .await
            .map_err(Into::into)
    }

    fn workspace_id(&self, target: &AgentChatWorkspaceTarget) -> Result<String, ApplicationError> {
        workspace_id_for_stable_chat_id(&target.chat_ref, &target.stable_chat_id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::*;
    use crate::domain::errors::DomainError;

    struct MockLifecycleRepository {
        deleted: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl AgentWorkspaceLifecycleRepository for MockLifecycleRepository {
        async fn delete_chat_workspace(
            &self,
            workspace_id: &str,
        ) -> Result<AgentChatWorkspaceDeletion, DomainError> {
            self.deleted.lock().await.push(workspace_id.to_string());
            Ok(AgentChatWorkspaceDeletion {
                workspace_id: workspace_id.to_string(),
                removed: true,
                run_ids: Vec::new(),
            })
        }

        async fn prune_persistent_states(
            &self,
            workspace_id: &str,
            _request: AgentPersistentStatePruneRequest,
        ) -> Result<AgentPersistentStatePrune, DomainError> {
            Ok(AgentPersistentStatePrune {
                workspace_id: workspace_id.to_string(),
                removed_state_ids: Vec::new(),
            })
        }
    }

    struct MockRunActivity {
        active_run_ids: Vec<String>,
    }

    #[async_trait]
    impl AgentRunActivity for MockRunActivity {
        async fn active_run_ids(&self) -> Result<Vec<String>, ApplicationError> {
            Ok(self.active_run_ids.clone())
        }

        async fn active_run_ids_for_workspace(
            &self,
            _workspace_id: &str,
        ) -> Result<Vec<String>, ApplicationError> {
            Ok(self.active_run_ids.clone())
        }
    }

    #[test]
    fn character_target_uses_chat_integrity_when_present() {
        let target = AgentWorkspaceLifecycleService::character_target_from_metadata(
            "Alice",
            "session",
            &serde_json::json!({ "integrity": "stable-a" }),
        )
        .expect("target")
        .expect("present");

        assert_eq!(target.stable_chat_id, "stable-a");
        assert!(matches!(target.chat_ref, AgentChatRef::Character { .. }));
    }

    #[test]
    fn character_target_skips_untracked_legacy_chat() {
        let target = AgentWorkspaceLifecycleService::character_target_from_metadata(
            "Alice",
            "session",
            &serde_json::json!({}),
        )
        .expect("target");

        assert!(target.is_none());
    }

    #[tokio::test]
    async fn active_run_blocks_workspace_deletion() {
        let repository = Arc::new(MockLifecycleRepository {
            deleted: Mutex::new(Vec::new()),
        });
        let service = AgentWorkspaceLifecycleService::new(
            repository.clone(),
            Arc::new(MockRunActivity {
                active_run_ids: vec!["run_active".to_string()],
            }),
        );

        let target = AgentWorkspaceLifecycleService::group_target("group-chat").expect("target");
        let error = service
            .delete_chat_workspace(&target)
            .await
            .expect_err("active run should block deletion");

        assert!(error.to_string().contains("agent.workspace_in_use"));
        assert!(repository.deleted.lock().await.is_empty());
    }
}
