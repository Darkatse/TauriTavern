use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, oneshot, watch};

use crate::dto::agent_dto::{
    AgentListToolsResultDto, AgentPromptAssemblyBrokerRequestDto, AgentToolCatalogDiagnosticDto,
    AgentToolCatalogItemDto,
};
use crate::errors::ApplicationError;
use crate::services::agent_model_gateway::AgentModelGateway;
use crate::services::agent_profile_service::{
    AgentProfileResolveInput, AgentProfileService, materialize_agent_system_prompt,
};
use crate::services::agent_tools::{
    AgentToolDispatcher, BuiltinAgentToolRegistry, compile_invocation_tool_snapshot,
    project_agent_model_tools,
};
use crate::services::llm_connection_service::LlmConnectionService;
use crate::services::mcp_service::{McpModelToolDiagnostic, McpService};
use crate::services::prompt_assembly_service::PromptAssemblyService;
use crate::services::skill_service::SkillService;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::agent::{
    AgentInvocation, AgentInvocationExitPolicy, AgentModelRequest, AgentModelTool,
};
use tt_domain::models::skill::SkillIndexEntry;
use tt_domain::models::tool::{
    InvocationToolSnapshot, ToolCatalog, ToolChoice, ToolSnapshotId, ToolTurnContract,
};
use tt_ports::repositories::agent_invocation_repository::AgentInvocationRepository;
use tt_ports::repositories::agent_run_repository::AgentRunRepository;
use tt_ports::repositories::chat_repository::ChatRepository;
use tt_ports::repositories::group_chat_repository::GroupChatRepository;
use tt_ports::repositories::workspace_repository::WorkspaceRepository;
use tt_ports::skill_script::SkillScriptEngine;

mod artifacts;
mod commit;
mod commit_ledger;
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
mod model_turn_display;
mod prompt_assembly;
mod prompt_snapshot;
mod scheduler;
mod skill_scope;
mod timeline_projection;
mod tool_call_projection;
mod tool_execution;
mod tool_snapshot;

#[cfg(test)]
mod tests;

use scheduler::ActiveRunHandle;
pub use tool_call_projection::{
    AgentRunLiveCall, AgentRunLiveCallKey, AgentRunLiveProjection, ModelAttemptGeneration,
    ToolCallProjection,
};

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

struct PreparedInvocation {
    frozen_macros: Arc<tt_domain::frozen_macros::FrozenMacros>,
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
    diagnostics: Vec<McpModelToolDiagnostic>,
}

pub struct AgentRuntimeService {
    run_repository: Arc<dyn AgentRunRepository>,
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
    tool_registry: BuiltinAgentToolRegistry,
    tool_dispatcher: AgentToolDispatcher,
    active_runs: RwLock<HashMap<String, Arc<ActiveRunHandle>>>,
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
        skill_script_engine: Arc<dyn SkillScriptEngine>,
    ) -> Self {
        let tool_registry = BuiltinAgentToolRegistry::all();
        let tool_dispatcher = AgentToolDispatcher::new(
            run_repository.clone(),
            chat_repository.clone(),
            group_chat_repository.clone(),
            workspace_repository.clone(),
            skill_service.clone(),
            skill_script_engine,
        );
        Self {
            run_repository,
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
            tool_registry,
            tool_dispatcher,
            active_runs: RwLock::new(HashMap::new()),
            active_chat_commits: RwLock::new(HashMap::new()),
            active_prompt_assemblies: RwLock::new(HashMap::new()),
            active_persistent_state_metadata_updates: RwLock::new(HashMap::new()),
        }
    }

    pub fn tool_catalog(&self) -> &ToolCatalog {
        self.tool_registry.catalog()
    }

    pub async fn tool_catalog_items(&self) -> Result<AgentListToolsResultDto, ApplicationError> {
        let mut tools = self
            .tool_registry
            .catalog()
            .iter()
            .map(|descriptor| {
                let title = descriptor.title.clone().ok_or_else(|| {
                    ApplicationError::InternalError(format!(
                        "agent.tool_title_required: builtin tool `{}` has no title",
                        descriptor.id
                    ))
                })?;
                let description = descriptor.description.clone().ok_or_else(|| {
                    ApplicationError::InternalError(format!(
                        "agent.tool_description_required: builtin tool `{}` has no description",
                        descriptor.id
                    ))
                })?;
                Ok(AgentToolCatalogItemDto {
                    id: descriptor.id.clone(),
                    native_name: descriptor.id.native_name().to_string(),
                    title,
                    description,
                    input_schema: descriptor.input_schema.clone(),
                    output_schema: descriptor.output_schema.clone(),
                    annotations: descriptor.annotations.clone(),
                    source: "builtin".to_string(),
                    registration_id: None,
                    server_display_name: None,
                    permission: None,
                })
            })
            .collect::<Result<Vec<_>, ApplicationError>>()?;
        let mcp = self.mcp_service.list_permitted_model_tools_cached().await?;
        tools.extend(mcp.tools.into_iter().map(|tool| {
            AgentToolCatalogItemDto {
                id: tool.descriptor.id.clone(),
                native_name: tool.descriptor.id.native_name().to_string(),
                title: tool
                    .descriptor
                    .title
                    .clone()
                    .unwrap_or_else(|| tool.descriptor.id.native_name().to_string()),
                description: tool.descriptor.description.clone().unwrap_or_default(),
                input_schema: tool.descriptor.input_schema.clone(),
                output_schema: tool.descriptor.output_schema.clone(),
                annotations: tool.descriptor.annotations.clone(),
                source: "mcp".to_string(),
                registration_id: Some(tool.registration_id.to_string()),
                server_display_name: Some(tool.server_display_name),
                permission: Some(tool.permission),
            }
        }));
        Ok(AgentListToolsResultDto {
            tools,
            diagnostics: mcp
                .diagnostics
                .into_iter()
                .map(|diagnostic| AgentToolCatalogDiagnosticDto {
                    tool_id: diagnostic.tool_id,
                    code: diagnostic.code,
                    message: diagnostic.message,
                })
                .collect(),
        })
    }

    pub async fn visible_model_tools(
        &self,
        profile: &ResolvedAgentProfile,
    ) -> Result<Vec<AgentModelTool>, ApplicationError> {
        Ok(self
            .prepare_invocation_tools(
                profile,
                AgentInvocationExitPolicy::RunFinishAllowed,
                "profile_preview",
            )
            .await?
            .model_tools)
    }

    async fn prepare_invocation_tools(
        &self,
        profile: &ResolvedAgentProfile,
        exit_policy: AgentInvocationExitPolicy,
        snapshot_id: &str,
    ) -> Result<PreparedInvocationTools, ApplicationError> {
        let selected = profile
            .tools
            .allow
            .iter()
            .filter(|id| !id.is_builtin() && !profile.tools.deny.iter().any(|denied| denied == *id))
            .cloned()
            .collect::<Vec<_>>();
        let mut mcp = self
            .mcp_service
            .resolve_permitted_model_tools_cached(&selected)
            .await?;
        let mut override_diagnostics = Vec::new();
        mcp.tools.retain_mut(|tool| {
            let Some(override_) = profile.tools.tool_descriptions.get(&tool.descriptor.id) else {
                return true;
            };
            match tool.descriptor.apply_description_override(override_) {
                Ok(()) => true,
                Err(error) => {
                    override_diagnostics.push(McpModelToolDiagnostic {
                        tool_id: Some(tool.descriptor.id.clone()),
                        code: "mcp.agent_tool_override_invalid".to_string(),
                        message: error.to_string(),
                    });
                    false
                }
            }
        });
        mcp.diagnostics.extend(override_diagnostics);
        let snapshot = compile_invocation_tool_snapshot(
            &self.tool_registry,
            profile,
            exit_policy,
            ToolSnapshotId::parse(snapshot_id.to_string())?,
            &mcp.tools,
        )?;
        let turn = ToolTurnContract::all(&snapshot, ToolChoice::Auto)?;
        let model_tools = project_agent_model_tools(&snapshot, &turn)?;
        Ok(PreparedInvocationTools {
            snapshot,
            turn,
            model_tools,
            diagnostics: mcp.diagnostics,
        })
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

        Ok(materialize_agent_system_prompt(&visible_tools, &profile))
    }
}
