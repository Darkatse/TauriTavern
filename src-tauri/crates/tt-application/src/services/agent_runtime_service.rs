use std::collections::HashMap;
use std::sync::Arc;
use tt_ports::workspace_fs::WorkspaceFs;

use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, oneshot, watch};

use crate::dto::agent_dto::{AgentPromptAssemblyBrokerRequestDto, AgentToolCatalogDiagnosticDto};
use crate::errors::ApplicationError;
use crate::services::agent_model_gateway::AgentModelGateway;
use crate::services::agent_profile_service::{
    AgentProfileResolveInput, AgentProfileService, materialize_agent_system_prompt,
};
use crate::services::agent_tools::{AgentToolDispatcher, BuiltinAgentToolRegistry};
use crate::services::llm_connection_service::LlmConnectionService;
use crate::services::mcp_service::McpService;
use crate::services::prompt_assembly_service::PromptAssemblyService;
use crate::services::skill_service::SkillService;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentInvocation, AgentInvocationExitPolicy, AgentModelRequest, AgentModelTool,
};
use tt_domain::models::skill::SkillIndexEntry;
use tt_domain::models::tool::{InvocationToolSnapshot, ToolCatalog, ToolTurnContract};
use tt_ports::repositories::agent_invocation_repository::AgentInvocationRepository;
use tt_ports::repositories::agent_run_repository::AgentRunRepository;
use tt_ports::repositories::agent_session_repository::AgentSessionRepository;
use tt_ports::repositories::chat_repository::ChatRepository;
use tt_ports::repositories::group_chat_repository::GroupChatRepository;
use tt_ports::repositories::workspace_repository::WorkspaceRepository;
use tt_ports::workspace_shell::WorkspaceShell;

mod artifacts;
mod checkpoint;
mod commit;
mod commit_ledger;
mod continuation;
mod delegation;
mod error_payload;
mod executor;
mod guidance;
mod input_context;
mod invocation;
mod journal;
mod lifecycle;
mod loop_runner;
mod markdown;
mod model_response_store;
mod model_retry;
mod model_stream_projection;
mod model_turn_display;
mod prompt_assembly;
mod prompt_snapshot;
mod revision;
mod scheduler;
mod session;
mod skill_scope;
mod task_details;
mod timeline_projection;
mod tool_catalog;
mod tool_execution;
mod tool_results;
mod tool_snapshot;

#[cfg(test)]
mod tests;

pub use model_stream_projection::{
    AgentRunLiveCall, AgentRunLiveCallKey, AgentRunLiveProjection, AgentRunLiveResponse,
    ModelAttemptGeneration, ToolCallProjection,
};
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreparedInvocation {
    #[serde(skip)]
    runtime_context: Arc<tt_ports::workspace_shell::WorkspaceShellContext>,
    invocation: AgentInvocation,
    delegation_task_id: Option<String>,
    profile: ResolvedAgentProfile,
    tool_snapshot: InvocationToolSnapshot,
    tool_turn: ToolTurnContract,
    request: AgentModelRequest,
    effective_skills: Vec<SkillIndexEntry>,
}

struct PreparedInvocationTools {
    snapshot: InvocationToolSnapshot,
    turn: ToolTurnContract,
    model_tools: Vec<AgentModelTool>,
    diagnostics: Vec<AgentToolCatalogDiagnosticDto>,
}

pub struct AgentRuntimeService {
    run_repository: Arc<dyn AgentRunRepository>,
    session_repository: Arc<dyn AgentSessionRepository>,
    invocation_repository: Arc<dyn AgentInvocationRepository>,
    workspace_repository: Arc<dyn WorkspaceRepository>,
    chat_repository: Arc<dyn ChatRepository>,
    group_chat_repository: Arc<dyn GroupChatRepository>,
    model_gateway: Arc<dyn AgentModelGateway>,
    profile_service: Arc<AgentProfileService>,
    llm_connection_service: Arc<LlmConnectionService>,
    prompt_assembly_service: Arc<PromptAssemblyService>,
    skill_service: Arc<SkillService>,
    mcp_service: Arc<McpService>,
    extension_tools: Arc<dyn tt_ports::extension_tools::ExtensionTools>,
    tool_registry: BuiltinAgentToolRegistry,
    tool_dispatcher: AgentToolDispatcher,
    active_runs: RwLock<HashMap<String, Arc<ActiveRunHandle>>>,
    run_lifecycle_lock: Arc<tokio::sync::Mutex<()>>,
    active_chat_commits: RwLock<HashMap<String, PendingHostChatCommit>>,
    active_prompt_assemblies: RwLock<HashMap<String, PendingHostPromptAssembly>>,
    active_persistent_state_metadata_updates:
        RwLock<HashMap<String, PendingPersistentStateMetadataUpdate>>,
}

impl AgentRuntimeService {
    #[expect(
        clippy::too_many_arguments,
        reason = "composition boundary keeps concrete runtime dependencies explicit"
    )]
    pub fn new(
        run_repository: Arc<dyn AgentRunRepository>,
        session_repository: Arc<dyn AgentSessionRepository>,
        invocation_repository: Arc<dyn AgentInvocationRepository>,
        workspace_repository: Arc<dyn WorkspaceRepository>,
        chat_repository: Arc<dyn ChatRepository>,
        group_chat_repository: Arc<dyn GroupChatRepository>,
        skill_service: Arc<SkillService>,
        model_gateway: Arc<dyn AgentModelGateway>,
        profile_service: Arc<AgentProfileService>,
        llm_connection_service: Arc<LlmConnectionService>,
        prompt_assembly_service: Arc<PromptAssemblyService>,
        mcp_service: Arc<McpService>,
        workspace_shell: Arc<dyn WorkspaceShell>,
        extension_tools: Arc<dyn tt_ports::extension_tools::ExtensionTools>,
    ) -> Self {
        let tool_registry = BuiltinAgentToolRegistry::all();
        let tool_dispatcher = AgentToolDispatcher::new(
            run_repository.clone(),
            chat_repository.clone(),
            group_chat_repository.clone(),
            skill_service.clone(),
            workspace_shell,
        );
        Self {
            run_repository,
            session_repository,
            invocation_repository,
            workspace_repository,
            chat_repository,
            group_chat_repository,
            model_gateway,
            profile_service,
            llm_connection_service,
            prompt_assembly_service,
            skill_service,
            mcp_service,
            extension_tools,
            tool_registry,
            tool_dispatcher,
            active_runs: RwLock::new(HashMap::new()),
            run_lifecycle_lock: Arc::new(tokio::sync::Mutex::new(())),
            active_chat_commits: RwLock::new(HashMap::new()),
            active_prompt_assemblies: RwLock::new(HashMap::new()),
            active_persistent_state_metadata_updates: RwLock::new(HashMap::new()),
        }
    }

    pub fn run_lifecycle_lock(&self) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(&self.run_lifecycle_lock)
    }

    pub fn tool_catalog(&self) -> &ToolCatalog {
        self.tool_registry.catalog()
    }

    pub async fn resolve_agent_system_prompt(
        &self,
        profile_id: Option<&str>,
    ) -> Result<String, ApplicationError> {
        let profile = self
            .profile_service
            .resolve_profile_for_preview(AgentProfileResolveInput {
                profile_id,
                tool_catalog: self.tool_registry.catalog(),
            })
            .await?;
        let visible_tools = self.visible_model_tools(&profile).await?;

        Ok(materialize_agent_system_prompt(
            &visible_tools,
            &profile,
            AgentInvocationExitPolicy::RunFinishAllowed,
        ))
    }

    async fn workspace_files(
        &self,
        run_id: &str,
    ) -> Result<Arc<dyn WorkspaceFs>, ApplicationError> {
        if let Some(handle) = self.active_runs.read().await.get(run_id).cloned() {
            return Ok(handle.files.clone());
        }
        self.workspace_repository
            .open_filesystem(run_id)
            .await
            .map_err(Into::into)
    }
}
