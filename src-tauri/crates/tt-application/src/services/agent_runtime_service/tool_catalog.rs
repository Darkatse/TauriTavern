use std::collections::HashSet;

use super::{AgentRuntimeService, PreparedInvocationTools};
use crate::dto::agent_dto::{
    AgentListToolsResultDto, AgentToolCatalogDiagnosticDto, AgentToolCatalogItemDto,
};
use crate::errors::ApplicationError;
use crate::services::agent_tools::{
    ExternalAgentTool, builtin_available_in_scope, compile_invocation_tool_snapshot,
    mcp_model_name, prepare_tool_bindings, project_agent_model_tools,
};
use tt_domain::models::agent::profile::{AgentProfileDefinition, ResolvedAgentProfile};
use tt_domain::models::agent::{AgentInvocationExitPolicy, AgentModelTool};
use tt_domain::models::tool::{
    AgentToolScope, ToolChoice, ToolDescriptor, ToolId, ToolSnapshotId, ToolTurnContract,
};

impl AgentRuntimeService {
    pub async fn tool_catalog_items(
        &self,
        context: Option<AgentToolScope>,
    ) -> Result<AgentListToolsResultDto, ApplicationError> {
        let mut tools = self
            .tool_registry
            .catalog()
            .iter()
            .filter(|tool| {
                context.is_none_or(|scope| builtin_available_in_scope(tool.id.native_name(), scope))
            })
            .map(|tool| catalog_item(tool.clone(), "builtin"))
            .collect::<Vec<_>>();
        let mcp = self.mcp_service.list_permitted_model_tools_cached().await?;
        tools.extend(mcp.tools.into_iter().map(|tool| {
            let mut item = catalog_item(tool.descriptor, "mcp");
            item.registration_id = Some(tool.registration_id.to_string());
            item.server_display_name = Some(tool.server_display_name);
            item.permission = Some(tool.permission);
            item
        }));
        tools.extend(
            self.extension_tools
                .list()?
                .into_iter()
                .filter(|tool| context.is_none_or(|scope| tool.contexts.contains(&scope)))
                .map(|tool| {
                    let mut item = catalog_item(tool.descriptor, "extension");
                    item.extension_id = item.id.extension_id().map(str::to_string);
                    item.contexts = Some(tool.contexts);
                    item.enabled = Some(tool.enabled);
                    item
                }),
        );
        Ok(AgentListToolsResultDto {
            tools,
            diagnostics: mcp
                .diagnostics
                .into_iter()
                .map(|issue| AgentToolCatalogDiagnosticDto {
                    tool_id: issue.tool_id,
                    code: issue.code,
                    message: issue.message,
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
                AgentToolScope::Chat,
                AgentInvocationExitPolicy::RunFinishAllowed,
                "profile_preview",
            )
            .await?
            .model_tools)
    }

    pub(super) async fn prepare_invocation_tools(
        &self,
        profile: &ResolvedAgentProfile,
        context: AgentToolScope,
        exit_policy: AgentInvocationExitPolicy,
        snapshot_id: &str,
    ) -> Result<PreparedInvocationTools, ApplicationError> {
        let selected = profile
            .tools
            .allow
            .iter()
            .filter(|id| !profile.tools.deny.contains(id))
            .collect::<Vec<_>>();
        let mcp_ids = selected
            .iter()
            .filter(|id| id.provider_id().starts_with("mcp/"))
            .map(|id| (*id).clone())
            .collect::<Vec<_>>();
        let mcp = self
            .mcp_service
            .resolve_permitted_model_tools_cached(&mcp_ids)
            .await?;
        let mut diagnostics = mcp
            .diagnostics
            .into_iter()
            .map(|issue| AgentToolCatalogDiagnosticDto {
                tool_id: issue.tool_id,
                code: issue.code,
                message: issue.message,
            })
            .collect::<Vec<_>>();
        let mut external = mcp
            .tools
            .into_iter()
            .map(|tool| ExternalAgentTool {
                model_name: mcp_model_name(
                    &tool.server_display_name,
                    tool.descriptor.id.native_name(),
                ),
                descriptor: tool.descriptor,
            })
            .collect::<Vec<_>>();
        let extensions = self.extension_tools.list()?;
        for id in selected
            .into_iter()
            .filter(|id| id.extension_id().is_some())
        {
            let Some(tool) = extensions.iter().find(|tool| tool.descriptor.id == *id) else {
                diagnostics.push(AgentToolCatalogDiagnosticDto {
                    tool_id: Some(id.clone()),
                    code: "extension.tool_unavailable".into(),
                    message: format!("Extension tool `{id}` is not loaded; this invocation will use the remaining tools."),
                });
                continue;
            };
            if tool.enabled && tool.contexts.contains(&context) {
                external.push(ExternalAgentTool {
                    descriptor: tool.descriptor.clone(),
                    model_name: id.native_name().to_string(),
                });
            }
        }
        external.retain_mut(|tool| {
            let Some(override_) = profile.tools.tool_descriptions.get(&tool.descriptor.id) else {
                return true;
            };
            match tool.descriptor.apply_description_override(override_) {
                Ok(()) => true,
                Err(error) => {
                    diagnostics.push(AgentToolCatalogDiagnosticDto {
                        tool_id: Some(tool.descriptor.id.clone()),
                        code: "agent.tool_override_invalid".into(),
                        message: error.to_string(),
                    });
                    false
                }
            }
        });
        let bindings = prepare_tool_bindings(&self.tool_registry, profile, context, &external)?;
        let snapshot = compile_invocation_tool_snapshot(
            &self.tool_registry,
            profile,
            exit_policy,
            ToolSnapshotId::parse(snapshot_id)?,
            bindings,
        )?;
        let turn = ToolTurnContract::all(&snapshot, ToolChoice::Auto)?;
        let model_tools = project_agent_model_tools(&snapshot, &turn)?;
        Ok(PreparedInvocationTools {
            snapshot,
            turn,
            model_tools,
            diagnostics,
        })
    }

    pub async fn save_profile(
        &self,
        profile: AgentProfileDefinition,
    ) -> Result<(), ApplicationError> {
        let previous = self
            .profile_service
            .load_profile(profile.id.as_str())
            .await?;
        self.validate_new_extension_tools(&profile, previous.as_ref())?;
        self.profile_service
            .save_profile(profile, self.tool_catalog())
            .await
    }

    /// Missing tools only block a new selection, not edits to an existing profile.
    pub(super) fn validate_new_extension_tools(
        &self,
        profile: &AgentProfileDefinition,
        previous: Option<&AgentProfileDefinition>,
    ) -> Result<(), ApplicationError> {
        let active_ids = |profile: &AgentProfileDefinition| {
            profile
                .tools
                .allow
                .iter()
                .filter(|id| !profile.tools.deny.contains(id))
                .cloned()
                .collect::<HashSet<_>>()
        };
        let before = previous.map(active_ids).unwrap_or_default();
        let after = active_ids(profile);
        let tools = self.extension_tools.list()?;
        for raw in after
            .difference(&before)
            .filter(|id| id.starts_with("extension/"))
        {
            let id = ToolId::parse(raw.clone())?;
            if id.extension_id().is_some() && !tools.iter().any(|tool| tool.descriptor.id == id) {
                return Err(ApplicationError::ValidationError(format!(
                    "extension.tool_unavailable: load the extension before enabling `{id}` in this Profile"
                )));
            }
        }
        Ok(())
    }
}

fn catalog_item(descriptor: ToolDescriptor, source: &str) -> AgentToolCatalogItemDto {
    AgentToolCatalogItemDto {
        native_name: descriptor.id.native_name().to_string(),
        title: descriptor
            .title
            .unwrap_or_else(|| descriptor.id.native_name().to_string()),
        description: descriptor.description.unwrap_or_default(),
        id: descriptor.id,
        input_schema: descriptor.input_schema,
        output_schema: descriptor.output_schema,
        annotations: descriptor.annotations,
        source: source.to_string(),
        registration_id: None,
        server_display_name: None,
        permission: None,
        extension_id: None,
        contexts: None,
        enabled: None,
    }
}
