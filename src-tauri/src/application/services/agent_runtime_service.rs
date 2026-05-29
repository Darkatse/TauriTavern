use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, oneshot, watch};

use crate::application::dto::agent_dto::AgentPromptAssemblyBrokerRequestDto;
use crate::application::errors::ApplicationError;
use crate::application::services::agent_model_gateway::AgentModelGateway;
use crate::application::services::agent_profile_service::{
    AgentProfileResolveInput, AgentProfileService, materialize_agent_system_prompt,
};
use crate::application::services::agent_tools::{
    AGENT_AWAIT, AGENT_DELEGATE, AGENT_LIST, AgentToolDispatcher, BuiltinAgentToolRegistry,
    TASK_RETURN,
};
use crate::application::services::llm_connection_service::LlmConnectionService;
use crate::application::services::prompt_assembly_service::PromptAssemblyService;
use crate::application::services::skill_service::SkillService;
use crate::domain::models::agent::profile::ResolvedAgentProfile;
use crate::domain::models::agent::{AgentInvocationExitPolicy, AgentToolSpec};
use crate::domain::repositories::agent_invocation_repository::AgentInvocationRepository;
use crate::domain::repositories::agent_run_repository::AgentRunRepository;
use crate::domain::repositories::chat_repository::ChatRepository;
use crate::domain::repositories::checkpoint_repository::CheckpointRepository;
use crate::domain::repositories::group_chat_repository::GroupChatRepository;
use crate::domain::repositories::workspace_repository::WorkspaceRepository;

mod artifacts;
mod commit;
mod commit_ledger;
mod delegation;
mod error_payload;
mod executor;
mod input_context;
mod invocation;
mod journal;
mod lifecycle;
mod loop_runner;
mod model_response_store;
mod model_retry;
mod model_turn;
mod model_turn_display;
mod prompt_assembly;
mod prompt_snapshot;
mod scheduler;
mod tool_execution;

#[cfg(test)]
mod tests;

use scheduler::ActiveRunHandle;

pub(super) type AgentCancelReceiver = watch::Receiver<bool>;

pub(super) struct PendingHostChatCommit {
    pub(super) run_id: String,
    pub(super) sender: oneshot::Sender<Result<HostChatCommitResult, String>>,
}

pub(super) struct HostChatCommitResult {
    pub(super) message_id: Option<String>,
}

pub(super) struct PendingHostPromptAssembly {
    pub(super) run_id: String,
    pub(super) request: AgentPromptAssemblyBrokerRequestDto,
    pub(super) sender: oneshot::Sender<Result<HostPromptAssemblyResult, String>>,
}

pub(super) struct HostPromptAssemblyResult {
    pub(super) prompt_snapshot: serde_json::Value,
    pub(super) frozen_run_input_snapshot: Option<serde_json::Value>,
    pub(super) generation_intent: Option<serde_json::Value>,
    pub(super) assembly: Option<serde_json::Value>,
}

pub(super) struct PendingPersistentStateMetadataUpdate {
    pub(super) run_id: String,
    pub(super) sender: oneshot::Sender<Result<(), String>>,
}

pub struct AgentRuntimeService {
    run_repository: Arc<dyn AgentRunRepository>,
    invocation_repository: Arc<dyn AgentInvocationRepository>,
    workspace_repository: Arc<dyn WorkspaceRepository>,
    checkpoint_repository: Arc<dyn CheckpointRepository>,
    chat_repository: Arc<dyn ChatRepository>,
    group_chat_repository: Arc<dyn GroupChatRepository>,
    model_gateway: Arc<dyn AgentModelGateway>,
    profile_service: Arc<AgentProfileService>,
    llm_connection_service: Arc<LlmConnectionService>,
    prompt_assembly_service: Option<Arc<PromptAssemblyService>>,
    skill_service: Arc<SkillService>,
    tool_registry: BuiltinAgentToolRegistry,
    tool_dispatcher: AgentToolDispatcher,
    active_runs: RwLock<HashMap<String, Arc<ActiveRunHandle>>>,
    active_chat_commits: RwLock<HashMap<String, PendingHostChatCommit>>,
    active_prompt_assemblies: RwLock<HashMap<String, PendingHostPromptAssembly>>,
    active_persistent_state_metadata_updates:
        RwLock<HashMap<String, PendingPersistentStateMetadataUpdate>>,
}

impl AgentRuntimeService {
    #[cfg(test)]
    pub fn new(
        run_repository: Arc<dyn AgentRunRepository>,
        invocation_repository: Arc<dyn AgentInvocationRepository>,
        workspace_repository: Arc<dyn WorkspaceRepository>,
        checkpoint_repository: Arc<dyn CheckpointRepository>,
        chat_repository: Arc<dyn ChatRepository>,
        group_chat_repository: Arc<dyn GroupChatRepository>,
        skill_service: Arc<SkillService>,
        model_gateway: Arc<dyn AgentModelGateway>,
        profile_service: Arc<AgentProfileService>,
        llm_connection_service: Arc<LlmConnectionService>,
    ) -> Self {
        Self::new_internal(
            run_repository,
            invocation_repository,
            workspace_repository,
            checkpoint_repository,
            chat_repository,
            group_chat_repository,
            skill_service,
            model_gateway,
            profile_service,
            llm_connection_service,
            None,
        )
    }

    pub fn new_with_prompt_assembly_service(
        run_repository: Arc<dyn AgentRunRepository>,
        invocation_repository: Arc<dyn AgentInvocationRepository>,
        workspace_repository: Arc<dyn WorkspaceRepository>,
        checkpoint_repository: Arc<dyn CheckpointRepository>,
        chat_repository: Arc<dyn ChatRepository>,
        group_chat_repository: Arc<dyn GroupChatRepository>,
        skill_service: Arc<SkillService>,
        model_gateway: Arc<dyn AgentModelGateway>,
        profile_service: Arc<AgentProfileService>,
        llm_connection_service: Arc<LlmConnectionService>,
        prompt_assembly_service: Arc<PromptAssemblyService>,
    ) -> Self {
        Self::new_internal(
            run_repository,
            invocation_repository,
            workspace_repository,
            checkpoint_repository,
            chat_repository,
            group_chat_repository,
            skill_service,
            model_gateway,
            profile_service,
            llm_connection_service,
            Some(prompt_assembly_service),
        )
    }

    fn new_internal(
        run_repository: Arc<dyn AgentRunRepository>,
        invocation_repository: Arc<dyn AgentInvocationRepository>,
        workspace_repository: Arc<dyn WorkspaceRepository>,
        checkpoint_repository: Arc<dyn CheckpointRepository>,
        chat_repository: Arc<dyn ChatRepository>,
        group_chat_repository: Arc<dyn GroupChatRepository>,
        skill_service: Arc<SkillService>,
        model_gateway: Arc<dyn AgentModelGateway>,
        profile_service: Arc<AgentProfileService>,
        llm_connection_service: Arc<LlmConnectionService>,
        prompt_assembly_service: Option<Arc<PromptAssemblyService>>,
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
            invocation_repository,
            workspace_repository,
            checkpoint_repository,
            chat_repository,
            group_chat_repository,
            model_gateway,
            profile_service,
            llm_connection_service,
            prompt_assembly_service,
            skill_service,
            tool_registry,
            tool_dispatcher,
            active_runs: RwLock::new(HashMap::new()),
            active_chat_commits: RwLock::new(HashMap::new()),
            active_prompt_assemblies: RwLock::new(HashMap::new()),
            active_persistent_state_metadata_updates: RwLock::new(HashMap::new()),
        }
    }

    pub fn tool_specs(&self) -> &[AgentToolSpec] {
        self.tool_registry.specs()
    }

    pub fn visible_tool_specs(
        &self,
        profile: &ResolvedAgentProfile,
    ) -> Result<Vec<AgentToolSpec>, ApplicationError> {
        self.tool_registry.visible_specs(profile)
    }

    pub(super) fn visible_tool_specs_for_invocation(
        &self,
        profile: &ResolvedAgentProfile,
        exit_policy: AgentInvocationExitPolicy,
    ) -> Result<Vec<AgentToolSpec>, ApplicationError> {
        let mut tools = self.tool_registry.visible_specs(profile)?;
        if exit_policy == AgentInvocationExitPolicy::TaskReturnRequired {
            tools.retain(|tool| {
                !matches!(
                    tool.name.as_str(),
                    "workspace.commit"
                        | "workspace.finish"
                        | AGENT_LIST
                        | AGENT_DELEGATE
                        | AGENT_AWAIT
                )
            });
            if !tools.iter().any(|tool| tool.name == TASK_RETURN) {
                let task_return =
                    self.tool_registry
                        .spec_by_name(TASK_RETURN)
                        .ok_or_else(|| {
                            ApplicationError::ValidationError(
                                "agent.task_return_tool_missing: task.return is not registered"
                                    .to_string(),
                            )
                        })?;
                tools.push(task_return.clone());
            }
            self.tool_registry
                .apply_return_mode_context(&mut tools, profile)?;
        }
        Ok(tools)
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
