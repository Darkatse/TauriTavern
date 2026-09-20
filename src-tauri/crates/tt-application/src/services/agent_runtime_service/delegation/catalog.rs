use std::time::Instant;

use super::policy::AgentOperation;
use super::tool_error::tool_error_outcome;
use crate::errors::ApplicationError;
use crate::services::agent_runtime_service::AgentRuntimeService;
use crate::services::agent_tools::AgentToolDispatchOutcome;
use tt_domain::models::agent::AgentModelTool;
use tt_domain::models::agent::profile::ResolvedAgentProfile;
use tt_domain::models::tool::ToolInvocation;

impl AgentRuntimeService {
    pub(in crate::services::agent_runtime_service) async fn agent_catalog(
        &self,
        source: &ResolvedAgentProfile,
        tools: &[AgentModelTool],
    ) -> Result<String, ApplicationError> {
        let operations = [AgentOperation::Delegate, AgentOperation::Handoff]
            .into_iter()
            .filter(|operation| {
                tools.iter().any(|tool| {
                    tool.tool_id.is_builtin() && tool.tool_id.native_name() == operation.tool_name()
                })
            })
            .collect::<Vec<_>>();
        self.agent_catalog_for_operations(source, &operations).await
    }

    async fn agent_catalog_for_operations(
        &self,
        source: &ResolvedAgentProfile,
        operations: &[AgentOperation],
    ) -> Result<String, ApplicationError> {
        if operations.is_empty() {
            return Ok(String::new());
        }
        let mut targets = self
            .profile_service
            .list_resolved_profiles_for_discovery(self.tool_registry.catalog())
            .await?;
        targets.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));

        let mut entries = Vec::new();
        for target in targets {
            let allowed = operations
                .iter()
                .filter(|operation| operation.validate_target(source, &target).is_ok())
                .map(|operation| operation.name())
                .collect::<Vec<_>>();
            if allowed.is_empty() {
                continue;
            }
            let mut entry = format!("- {} [{}]", target.id.as_str(), allowed.join(", "));
            if let Some(description) = target
                .delegation
                .description_for_agents
                .as_deref()
                .or(target.description.as_deref())
                .filter(|description| !description.is_empty())
            {
                entry.push_str(": ");
                entry.push_str(description);
            }
            entries.push(entry);
        }
        if entries.is_empty() {
            return Ok("Available agents:\nNone.".to_string());
        }
        Ok(format!(
            "Available agents:\n{}\n\nUse the listed ID as agentId for the indicated operation.",
            entries.join("\n")
        ))
    }

    pub(super) async fn agent_target_error(
        &self,
        call: &ToolInvocation,
        source: &ResolvedAgentProfile,
        operation: AgentOperation,
        code: &str,
        message: &str,
        started: Instant,
    ) -> Result<AgentToolDispatchOutcome, ApplicationError> {
        // Refresh only when selecting a target fails. The original directory
        // stays in the prepared request, while calls resolve current targets.
        let catalog = self
            .agent_catalog_for_operations(source, &[operation])
            .await?;
        Ok(tool_error_outcome(
            call,
            code,
            &format!("{message}\n\n{catalog}"),
            started.elapsed().as_millis(),
        ))
    }
}
