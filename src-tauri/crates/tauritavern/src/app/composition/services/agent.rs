use std::sync::Arc;

use tt_application::services::agent_model_gateway::ChatCompletionAgentModelGateway;
use tt_application::services::agent_profile_diagnostic_service::AgentProfileDiagnosticService;
use tt_application::services::agent_profile_service::AgentProfileService;
use tt_application::services::agent_run_history_service::AgentRunHistoryService;
use tt_application::services::agent_run_retention_automation_service::AgentRunRetentionAutomationService;
use tt_application::services::agent_runtime_service::AgentRuntimeService;
use tt_application::services::agent_workspace_lifecycle_service::{
    AgentRunActivity, AgentWorkspaceLifecycleService,
};
use tt_application::services::chat_completion_service::ChatCompletionService;
use tt_application::services::llm_connection_service::LlmConnectionService;
use tt_application::services::mcp_service::McpService;
use tt_application::services::prompt_assembly_service::PromptAssemblyService;
use tt_application::services::skill_service::SkillService;
use tt_application::services::tokenization_service::TokenizationService;
use tt_ports::extension_tools::ExtensionTools;
use tt_ports::workspace_shell::WorkspaceShell;

use super::super::repositories::AppRepositories;

pub(super) struct AgentServices {
    pub(super) agent_profile_service: Arc<AgentProfileService>,
    pub(super) agent_profile_diagnostic_service: Arc<AgentProfileDiagnosticService>,
    pub(super) prompt_assembly_service: Arc<PromptAssemblyService>,
    pub(super) agent_run_history_service: Arc<AgentRunHistoryService>,
    pub(super) agent_run_retention_automation_service: Arc<AgentRunRetentionAutomationService>,
    pub(super) agent_runtime_service: Arc<AgentRuntimeService>,
    pub(super) agent_workspace_lifecycle_service: Arc<AgentWorkspaceLifecycleService>,
}

pub(super) struct AgentDependencies {
    pub(super) skill_service: Arc<SkillService>,
    pub(super) chat_completion_service: Arc<ChatCompletionService>,
    pub(super) llm_connection_service: Arc<LlmConnectionService>,
    pub(super) mcp_service: Arc<McpService>,
    pub(super) tokenization_service: Arc<TokenizationService>,
    pub(super) workspace_shell: Arc<dyn WorkspaceShell>,
    pub(super) extension_tools: Arc<dyn ExtensionTools>,
}

pub(super) fn build(
    repositories: &AppRepositories,
    dependencies: AgentDependencies,
) -> AgentServices {
    let AgentDependencies {
        skill_service,
        chat_completion_service,
        llm_connection_service,
        mcp_service,
        tokenization_service,
        workspace_shell,
        extension_tools,
    } = dependencies;
    let agent_profile_service = Arc::new(AgentProfileService::new(
        repositories.agent_profile_repository.clone(),
        repositories.agent_profile_storage_health_repository.clone(),
        repositories.preset_repository.clone(),
        repositories.agent_session_repository.clone(),
    ));
    let agent_profile_diagnostic_service = Arc::new(AgentProfileDiagnosticService::new(
        agent_profile_service.clone(),
        repositories.preset_repository.clone(),
        llm_connection_service.clone(),
    ));
    let prompt_assembly_service = Arc::new(PromptAssemblyService::new(
        agent_profile_service.clone(),
        repositories.preset_repository.clone(),
        llm_connection_service.clone(),
    ));
    let agent_runtime_service = Arc::new(AgentRuntimeService::new(
        repositories.agent_run_repository.clone(),
        repositories.agent_session_repository.clone(),
        repositories.agent_invocation_repository.clone(),
        repositories.workspace_repository.clone(),
        repositories.chat_repository.clone(),
        repositories.group_chat_repository.clone(),
        skill_service,
        Arc::new(ChatCompletionAgentModelGateway::new(
            chat_completion_service,
        )),
        agent_profile_service.clone(),
        llm_connection_service,
        prompt_assembly_service.clone(),
        mcp_service,
        workspace_shell,
        extension_tools,
        tokenization_service,
    ));
    let agent_run_history_service = Arc::new(AgentRunHistoryService::new(
        repositories.agent_run_repository.clone(),
        repositories.settings_repository.clone(),
        agent_runtime_service.clone() as Arc<dyn AgentRunActivity>,
        agent_runtime_service.run_lifecycle_lock(),
    ));
    let agent_run_retention_automation_service = Arc::new(AgentRunRetentionAutomationService::new(
        repositories.settings_repository.clone(),
        agent_run_history_service.clone(),
    ));
    let agent_workspace_lifecycle_service = Arc::new(AgentWorkspaceLifecycleService::new(
        repositories.agent_workspace_lifecycle_repository.clone(),
        agent_runtime_service.clone() as Arc<dyn AgentRunActivity>,
        agent_runtime_service.run_lifecycle_lock(),
    ));

    AgentServices {
        agent_profile_service,
        agent_profile_diagnostic_service,
        prompt_assembly_service,
        agent_run_history_service,
        agent_run_retention_automation_service,
        agent_runtime_service,
        agent_workspace_lifecycle_service,
    }
}
