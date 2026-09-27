use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{Mutex, MutexGuard};

use crate::errors::ApplicationError;
use crate::services::agent_identity::{validate_stable_chat_id, workspace_id_for_stable_chat_id};
use tt_domain::models::agent::AgentChatRef;
use tt_ports::repositories::agent_workspace_lifecycle_repository::{
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
    run_lifecycle_lock: Arc<Mutex<()>>,
}

impl AgentWorkspaceLifecycleService {
    pub fn new(
        repository: Arc<dyn AgentWorkspaceLifecycleRepository>,
        run_activity: Arc<dyn AgentRunActivity>,
        run_lifecycle_lock: Arc<Mutex<()>>,
    ) -> Self {
        Self {
            repository,
            run_activity,
            run_lifecycle_lock,
        }
    }

    pub(crate) async fn lock_run_lifecycle(&self) -> MutexGuard<'_, ()> {
        self.run_lifecycle_lock.lock().await
    }

    pub fn character_target_from_metadata(
        character_id: &str,
        file_name: &str,
        metadata: &Value,
    ) -> Result<Option<AgentChatWorkspaceTarget>, ApplicationError> {
        let Some(value) = metadata.get("integrity") else {
            return Ok(None);
        };
        let Some(stable_chat_id) = value.as_str() else {
            return Err(ApplicationError::ValidationError(
                "agent.invalid_chat_integrity: chat_metadata.integrity must be a string"
                    .to_string(),
            ));
        };
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

    /// The owning chat deletion holds the lifecycle lock through its entire operation.
    pub(crate) async fn delete_chat_workspace_locked(
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

    pub async fn copy_persistent_states(
        &self,
        source: &AgentChatWorkspaceTarget,
        target: &AgentChatWorkspaceTarget,
    ) -> Result<(), ApplicationError> {
        self.repository
            .copy_persistent_states(&self.workspace_id(source)?, &self.workspace_id(target)?)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn delete_chat_workspaces_locked(
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
        let _guard = self.lock_run_lifecycle().await;
        self.ensure_chat_workspace_inactive(target).await?;
        let workspace_id = self.workspace_id(target)?;
        self.repository
            .prune_persistent_states(&workspace_id, request)
            .await
            .map_err(Into::into)
    }

    fn workspace_id(&self, target: &AgentChatWorkspaceTarget) -> Result<String, ApplicationError> {
        workspace_id_for_stable_chat_id(
            &target.chat_ref,
            &validate_stable_chat_id(&target.stable_chat_id)?,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use tokio::sync::Mutex;

    use super::*;
    use tt_domain::errors::DomainError;

    struct MockLifecycleRepository {
        deleted: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl AgentWorkspaceLifecycleRepository for MockLifecycleRepository {
        async fn copy_persistent_states(
            &self,
            _source_workspace_id: &str,
            _target_workspace_id: &str,
        ) -> Result<(), DomainError> {
            Ok(())
        }

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

    #[tokio::test]
    async fn chat_workspace_cleanup_preserves_identity_and_waits_for_active_runs() {
        let repository = Arc::new(MockLifecycleRepository {
            deleted: Mutex::new(Vec::new()),
        });
        let integrity = " stable-a ";
        let target = AgentWorkspaceLifecycleService::character_target_from_metadata(
            "Alice",
            "session",
            &serde_json::json!({ "integrity": integrity }),
        )
        .unwrap()
        .unwrap();
        let expected_workspace =
            workspace_id_for_stable_chat_id(&target.chat_ref, integrity).unwrap();
        for active_run_ids in [vec!["run_active".into()], Vec::new()] {
            let busy = !active_run_ids.is_empty();
            let service = AgentWorkspaceLifecycleService::new(
                repository.clone(),
                Arc::new(MockRunActivity { active_run_ids }),
                Arc::new(Mutex::new(())),
            );
            let _guard = service.lock_run_lifecycle().await;
            let result = service.delete_chat_workspace_locked(&target).await;
            if busy {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("agent.workspace_in_use")
                );
                assert!(repository.deleted.lock().await.is_empty());
            } else {
                result.unwrap();
                assert_eq!(
                    repository.deleted.lock().await.as_slice(),
                    std::slice::from_ref(&expected_workspace)
                );
            }
        }
    }
}
