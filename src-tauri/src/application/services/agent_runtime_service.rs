use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, oneshot, watch};

use crate::application::errors::ApplicationError;
use crate::application::services::agent_model_gateway::AgentModelGateway;
use crate::application::services::agent_profile_service::{
    AgentProfileResolveInput, AgentProfileService, materialize_agent_system_prompt,
};
use crate::application::services::agent_tools::{AgentToolDispatcher, BuiltinAgentToolRegistry};
use crate::application::services::skill_service::SkillService;
use crate::domain::models::agent::AgentToolSpec;
use crate::domain::repositories::agent_run_repository::AgentRunRepository;
use crate::domain::repositories::chat_repository::ChatRepository;
use crate::domain::repositories::checkpoint_repository::CheckpointRepository;
use crate::domain::repositories::group_chat_repository::GroupChatRepository;
use crate::domain::repositories::workspace_repository::WorkspaceRepository;

mod artifacts;
mod commit;
mod commit_ledger;
mod error_payload;
mod executor;
mod input_context;
mod journal;
mod lifecycle;
mod loop_runner;
mod model_response_store;
mod model_retry;
mod model_turn;
mod model_turn_display;
mod prompt_snapshot;
mod tool_execution;

#[cfg(test)]
mod tests;

pub(super) type AgentCancelReceiver = watch::Receiver<bool>;

pub(super) struct PendingHostChatCommit {
    pub(super) run_id: String,
    pub(super) sender: oneshot::Sender<Result<HostChatCommitResult, String>>,
}

pub(super) struct HostChatCommitResult {
    pub(super) message_id: Option<String>,
}

pub(super) struct PendingPersistentStateMetadataUpdate {
    pub(super) run_id: String,
    pub(super) sender: oneshot::Sender<Result<(), String>>,
}

pub struct AgentRuntimeService {
    run_repository: Arc<dyn AgentRunRepository>,
    workspace_repository: Arc<dyn WorkspaceRepository>,
    checkpoint_repository: Arc<dyn CheckpointRepository>,
    chat_repository: Arc<dyn ChatRepository>,
    group_chat_repository: Arc<dyn GroupChatRepository>,
    model_gateway: Arc<dyn AgentModelGateway>,
    profile_service: Arc<AgentProfileService>,
    skill_service: Arc<SkillService>,
    tool_registry: BuiltinAgentToolRegistry,
    tool_dispatcher: AgentToolDispatcher,
    active_runs: RwLock<HashMap<String, watch::Sender<bool>>>,
    active_chat_commits: RwLock<HashMap<String, PendingHostChatCommit>>,
    active_persistent_state_metadata_updates:
        RwLock<HashMap<String, PendingPersistentStateMetadataUpdate>>,
}

impl AgentRuntimeService {
    pub fn new(
        run_repository: Arc<dyn AgentRunRepository>,
        workspace_repository: Arc<dyn WorkspaceRepository>,
        checkpoint_repository: Arc<dyn CheckpointRepository>,
        chat_repository: Arc<dyn ChatRepository>,
        group_chat_repository: Arc<dyn GroupChatRepository>,
        skill_service: Arc<SkillService>,
        model_gateway: Arc<dyn AgentModelGateway>,
        profile_service: Arc<AgentProfileService>,
    ) -> Self {
        let tool_registry = BuiltinAgentToolRegistry::phase2c();
        let tool_dispatcher = AgentToolDispatcher::new(
            run_repository.clone(),
            chat_repository.clone(),
            group_chat_repository.clone(),
            workspace_repository.clone(),
            skill_service.clone(),
        );
        Self {
            run_repository,
            workspace_repository,
            checkpoint_repository,
            chat_repository,
            group_chat_repository,
            model_gateway,
            profile_service,
            skill_service,
            tool_registry,
            tool_dispatcher,
            active_runs: RwLock::new(HashMap::new()),
            active_chat_commits: RwLock::new(HashMap::new()),
            active_persistent_state_metadata_updates: RwLock::new(HashMap::new()),
        }
    }

    pub fn tool_specs(&self) -> &[AgentToolSpec] {
        self.tool_registry.specs()
    }

    pub async fn resolve_agent_system_prompt(
        &self,
        profile_id: Option<&str>,
    ) -> Result<String, ApplicationError> {
        let profile = self
            .profile_service
            .resolve_profile(AgentProfileResolveInput {
                profile_id,
                known_tools: self.tool_registry.specs(),
            })
            .await?;
        let visible_tools = self.tool_registry.visible_specs(&profile)?;

        Ok(materialize_agent_system_prompt(&visible_tools, &profile))
    }
}
